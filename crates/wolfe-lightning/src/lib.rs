pub mod broadcaster;
pub mod error;
pub mod event_handler;
pub mod fee_estimator;
pub mod logger;
pub mod persister;
pub mod types;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use bitcoin::block::Header;
use bitcoin::BlockHash;
use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::{BestBlock, Confirm};
use lightning::ln::channelmanager::{
    Bolt11InvoiceParameters, ChainParameters, ChannelManager, ChannelManagerReadArgs, PaymentId,
    Retry,
};
use lightning::ln::peer_handler::{IgnoringMessageHandler, MessageHandler};
use lightning::onion_message::messenger::{DefaultMessageRouter, OnionMessenger};
use lightning::routing::gossip::NetworkGraph;
use lightning::routing::router::{DefaultRouter, RouteParametersConfig};
use lightning::routing::scoring::{
    ProbabilisticScorer, ProbabilisticScoringDecayParameters, ProbabilisticScoringFeeParameters,
};
use lightning::sign::{KeysManager, NodeSigner};
use lightning::util::config::UserConfig;
use lightning::util::persist::{KVStoreSync, MonitorUpdatingPersister};
use lightning::util::ser::ReadableArgs;
use lightning_invoice::{Bolt11Invoice, Description};
use tokio::sync::mpsc;
use tracing::{info, warn};

use wolfe_mempool::Mempool;
use wolfe_store::NodeStore;
use wolfe_types::config::LightningConfig;

use crate::broadcaster::WolfeBroadcaster;
use crate::error::LightningError;
use crate::event_handler::LightningEvent;
use crate::fee_estimator::WolfeFeeEstimator;
use crate::logger::WolfeLogger;
use crate::persister::WolfeKVStore;
use crate::types::*;

/// Handle for sending events from Lightning to the main event loop.
#[derive(Clone)]
pub struct LightningSender {
    tx: mpsc::Sender<LightningEvent>,
}

impl LightningSender {
    pub async fn send(&self, event: LightningEvent) {
        if let Err(e) = self.tx.send(event).await {
            tracing::debug!("lightning event channel closed: {}", e);
        }
    }
}

/// The Lightning Network manager.
///
/// Follows the same bridge pattern as `NostrBridge`: constructed with shared
/// dependencies, returns a sender handle for the main loop, and runs as an
/// independent tokio task.
pub struct LightningManager {
    config: LightningConfig,
    channel_manager: Arc<WolfeChannelManager>,
    chain_monitor: Arc<WolfeChainMonitor>,
    peer_manager: Arc<WolfePeerManager>,
    network_graph: Arc<WolfeNetworkGraph>,
    scorer: Arc<Mutex<WolfeScorer>>,
    _keys_manager: Arc<KeysManager>,
    kv_store: Arc<WolfeKVStore>,
    _event_tx: mpsc::Sender<LightningEvent>,
    has_channels: AtomicBool,
}

impl LightningManager {
    /// Create a new LightningManager.
    ///
    /// Returns `(manager, event_sender, broadcast_receiver)`:
    /// - `event_sender`: clone-able handle to receive Lightning events in main loop
    /// - `broadcast_receiver`: drain this in main loop to broadcast txs via P2P
    pub fn new(
        config: LightningConfig,
        network: bitcoin::Network,
        data_dir: &Path,
        store: Arc<NodeStore>,
        mempool: Arc<Mempool>,
        best_block_hash: BlockHash,
        best_block_height: u32,
    ) -> Result<
        (
            Self,
            LightningSender,
            mpsc::UnboundedReceiver<bitcoin::Transaction>,
        ),
        LightningError,
    > {
        let logger = Arc::new(WolfeLogger);

        // ── Seed / Key Management ───────────────────────────────────────
        let seed = load_or_create_seed(&store)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let keys_manager = Arc::new(KeysManager::new(
            &seed,
            now.as_secs(),
            now.subsec_nanos(),
            false,
        ));

        // ── Broadcaster ─────────────────────────────────────────────────
        let (broadcast_tx, broadcast_rx) = mpsc::unbounded_channel();
        let broadcaster = Arc::new(WolfeBroadcaster::new(broadcast_tx));

        // ── Fee Estimator ───────────────────────────────────────────────
        let fee_estimator = Arc::new(WolfeFeeEstimator::new(mempool));

        // ── KV Store (separate redb for Lightning data) ─────────────────
        let ln_db_path = data_dir.join("lightning.redb");
        let ln_db = redb::Database::create(&ln_db_path)
            .map_err(|e| LightningError::Persistence(e.to_string()))?;
        let kv_store = Arc::new(WolfeKVStore::new(Arc::new(ln_db)));

        // ── Network Graph ───────────────────────────────────────────────
        let network_graph = match read_network_graph(&kv_store, network, logger.clone()) {
            Some(ng) => {
                info!("loaded persisted network graph");
                Arc::new(ng)
            }
            None => {
                info!("creating fresh network graph");
                Arc::new(NetworkGraph::new(network, logger.clone()))
            }
        };

        // ── Scorer ──────────────────────────────────────────────────────
        let scorer = match read_scorer(&kv_store, network_graph.clone(), logger.clone()) {
            Some(s) => {
                info!("loaded persisted scorer");
                Arc::new(Mutex::new(s))
            }
            None => {
                info!("creating fresh scorer");
                Arc::new(Mutex::new(ProbabilisticScorer::new(
                    ProbabilisticScoringDecayParameters::default(),
                    network_graph.clone(),
                    logger.clone(),
                )))
            }
        };

        // ── Router ──────────────────────────────────────────────────────
        let router = Arc::new(DefaultRouter::new(
            network_graph.clone(),
            logger.clone(),
            keys_manager.clone(),
            scorer.clone(),
            ProbabilisticScoringFeeParameters::default(),
        ));

        // ── Message Router ──────────────────────────────────────────────
        let message_router = Arc::new(DefaultMessageRouter::new(
            network_graph.clone(),
            keys_manager.clone(),
        ));

        // ── Chain Monitor ───────────────────────────────────────────────
        let peer_storage_key = keys_manager.get_peer_storage_key();

        let monitor_persister = Arc::new(MonitorUpdatingPersister::new(
            kv_store.clone(),
            logger.clone(),
            1000, // maximum pending updates before full persist
            keys_manager.clone(),
            keys_manager.clone(),
            broadcaster.clone(),
            fee_estimator.clone(),
        ));

        let chain_monitor: Arc<WolfeChainMonitor> = Arc::new(ChainMonitor::new(
            None, // no chain filter
            broadcaster.clone(),
            logger.clone(),
            fee_estimator.clone(),
            monitor_persister,
            keys_manager.clone(),
            peer_storage_key,
        ));

        // ── Channel Manager ─────────────────────────────────────────────
        let user_config = UserConfig {
            accept_inbound_channels: config.accept_inbound_channels,
            channel_handshake_limits: lightning::util::config::ChannelHandshakeLimits {
                min_funding_satoshis: config.min_channel_size_sat,
                force_announced_channel_preference: false,
                ..Default::default()
            },
            channel_handshake_config: lightning::util::config::ChannelHandshakeConfig {
                max_inbound_htlc_value_in_flight_percent_of_channel: 100,
                announce_for_forwarding: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let channel_manager = load_or_create_channel_manager(
            &kv_store,
            keys_manager.clone(),
            fee_estimator.clone(),
            chain_monitor.clone(),
            broadcaster.clone(),
            router.clone(),
            message_router.clone(),
            logger.clone(),
            user_config,
            network,
            best_block_hash,
            best_block_height,
        )?;

        let channel_manager = Arc::new(channel_manager);

        // ── Onion Messenger ─────────────────────────────────────────────
        let onion_messenger = Arc::new(OnionMessenger::new(
            keys_manager.clone(),
            keys_manager.clone(),
            logger.clone(),
            channel_manager.clone(),
            message_router.clone(),
            channel_manager.clone(),
            IgnoringMessageHandler {},
            IgnoringMessageHandler {},
            IgnoringMessageHandler {},
        ));

        // ── LDK Peer Manager ───────────────────────────────────────────
        let ephemeral_bytes: [u8; 32] = rand::random();
        let lightning_msg_handler = MessageHandler {
            chan_handler: channel_manager.clone(),
            route_handler: IgnoringMessageHandler {},
            onion_message_handler: onion_messenger,
            custom_message_handler: IgnoringMessageHandler {},
            send_only_message_handler: chain_monitor.clone(),
        };

        let peer_manager = Arc::new(WolfePeerManager::new(
            lightning_msg_handler,
            now.as_secs() as u32,
            &ephemeral_bytes,
            logger.clone(),
            keys_manager.clone(),
        ));

        // ── Event channel ───────────────────────────────────────────────
        let (event_tx, _event_rx) = mpsc::channel(256);
        let sender = LightningSender {
            tx: event_tx.clone(),
        };

        let has_channels = AtomicBool::new(!channel_manager.list_channels().is_empty());

        Ok((
            Self {
                config,
                channel_manager,
                chain_monitor,
                peer_manager,
                network_graph,
                scorer,
                _keys_manager: keys_manager,
                kv_store,
                _event_tx: event_tx,
                has_channels,
            },
            sender,
            broadcast_rx,
        ))
    }

    /// Start the Lightning P2P listener. Call after construction.
    pub async fn start(&self) -> Result<(), LightningError> {
        let listen_port = self.config.listen_port;
        let peer_manager = self.peer_manager.clone();

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", listen_port))
            .await
            .map_err(|e| LightningError::PeerConnection(e.to_string()))?;

        info!(port = listen_port, "Lightning P2P listener started");

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        info!(%addr, "inbound Lightning connection");
                        let pm = peer_manager.clone();
                        tokio::spawn(async move {
                            lightning_net_tokio::setup_inbound(pm, stream.into_std().unwrap())
                                .await;
                        });
                    }
                    Err(e) => {
                        warn!(?e, "failed to accept Lightning connection");
                    }
                }
            }
        });

        Ok(())
    }

    /// Feed a validated block to LDK.
    ///
    /// During IBD with no open channels, this is a fast no-op.
    pub fn block_connected(&self, block: &bitcoin::Block, height: u32) {
        // Update has_channels flag if channels appeared since startup
        if !self.has_channels.load(Ordering::Relaxed) {
            if !self.channel_manager.list_channels().is_empty() {
                self.has_channels.store(true, Ordering::Relaxed);
                info!("channel detected — enabling full block processing for LDK");
            } else if !height.is_multiple_of(10000) {
                // Skip during IBD if no channels exist (optimization)
                return;
            }
        }

        let header = &block.header;
        let txdata: Vec<_> = block.txdata.iter().enumerate().collect();

        self.channel_manager
            .transactions_confirmed(header, &txdata, height);
        self.channel_manager.best_block_updated(header, height);

        self.chain_monitor
            .transactions_confirmed(header, &txdata, height);
        self.chain_monitor.best_block_updated(header, height);
    }

    /// Notify LDK of a disconnected block (reorg).
    pub fn block_disconnected(&self, header: &Header, height: u32) {
        self.channel_manager
            .best_block_updated(header, height.saturating_sub(1));
        self.chain_monitor
            .best_block_updated(header, height.saturating_sub(1));
    }

    /// Get the node's public key.
    pub fn node_id(&self) -> bitcoin::secp256k1::PublicKey {
        self.channel_manager.get_our_node_id()
    }

    /// Get the channel manager (for RPC handlers).
    pub fn channel_manager(&self) -> &Arc<WolfeChannelManager> {
        &self.channel_manager
    }

    /// Get the peer manager (for RPC handlers).
    pub fn peer_manager(&self) -> &Arc<WolfePeerManager> {
        &self.peer_manager
    }

    /// Get the network graph (for RPC handlers).
    pub fn network_graph(&self) -> &Arc<WolfeNetworkGraph> {
        &self.network_graph
    }

    /// Connect to a Lightning peer.
    pub async fn connect_peer(
        &self,
        pubkey: bitcoin::secp256k1::PublicKey,
        addr: SocketAddr,
    ) -> Result<(), LightningError> {
        info!(%pubkey, %addr, "connecting to Lightning peer");
        let pm = self.peer_manager.clone();
        match lightning_net_tokio::connect_outbound(pm.clone(), pubkey, addr).await {
            Some(connection_future) => {
                tokio::spawn(connection_future);
                info!(%pubkey, "Lightning peer connected");
                Ok(())
            }
            None => Err(LightningError::PeerConnection(format!(
                "failed to connect to {}@{}",
                pubkey, addr
            ))),
        }
    }

    /// Open a channel with a connected peer.
    pub fn open_channel(
        &self,
        pubkey: bitcoin::secp256k1::PublicKey,
        amount_sat: u64,
        push_msat: u64,
    ) -> Result<String, LightningError> {
        let user_channel_id: u128 = rand::random();
        match self.channel_manager.create_channel(
            pubkey,
            amount_sat,
            push_msat,
            user_channel_id,
            None,
            None,
        ) {
            Ok(channel_id) => {
                info!(%pubkey, amount_sat, "channel open initiated");
                Ok(hex::encode(channel_id.0))
            }
            Err(e) => Err(LightningError::Channel(format!("{:?}", e))),
        }
    }

    /// Create a BOLT11 invoice.
    pub fn create_invoice(
        &self,
        amount_msat: Option<u64>,
        description: &str,
        expiry_secs: Option<u32>,
    ) -> Result<String, LightningError> {
        let desc = Description::new(description.to_string())
            .map_err(|e| LightningError::Invoice(format!("invalid description: {:?}", e)))?;
        let params = Bolt11InvoiceParameters {
            amount_msats: amount_msat,
            description: lightning_invoice::Bolt11InvoiceDescription::Direct(desc),
            invoice_expiry_delta_secs: expiry_secs,
            ..Default::default()
        };
        match self.channel_manager.create_bolt11_invoice(params) {
            Ok(invoice) => {
                info!("created BOLT11 invoice");
                Ok(invoice.to_string())
            }
            Err(e) => Err(LightningError::Invoice(format!("{:?}", e))),
        }
    }

    /// Pay a BOLT11 invoice.
    pub fn pay_invoice(&self, invoice_str: &str) -> Result<String, LightningError> {
        let invoice: Bolt11Invoice = invoice_str
            .parse()
            .map_err(|e| LightningError::Invoice(format!("invalid invoice: {:?}", e)))?;

        let payment_id_bytes: [u8; 32] = rand::random();
        let payment_id = PaymentId(payment_id_bytes);

        self.channel_manager
            .pay_for_bolt11_invoice(
                &invoice,
                payment_id,
                None,
                RouteParametersConfig::default(),
                Retry::Attempts(3),
            )
            .map_err(|e| LightningError::Payment(format!("{:?}", e)))?;

        info!(
            payment_id = hex::encode(payment_id_bytes),
            "payment initiated"
        );
        Ok(hex::encode(payment_id_bytes))
    }

    /// Graceful shutdown.
    pub fn shutdown(&self) {
        info!("lightning manager shutting down");
        self.persist_state();
    }

    fn persist_state(&self) {
        use lightning::util::ser::Writeable;

        // Persist channel manager
        let buf = self.channel_manager.encode();
        if let Err(e) = self.kv_store.write("channel_manager", "", "manager", buf) {
            warn!(?e, "failed to persist channel manager");
        }

        // Persist network graph
        let buf = self.network_graph.encode();
        if let Err(e) = self.kv_store.write("network_graph", "", "graph", buf) {
            warn!(?e, "failed to persist network graph");
        }

        // Persist scorer
        if let Ok(scorer) = self.scorer.lock() {
            let buf = scorer.encode();
            if let Err(e) = self.kv_store.write("scorer", "", "scorer", buf) {
                warn!(?e, "failed to persist scorer");
            }
        }
    }
}

// ── Helper: read network graph / scorer from KVStore ────────────────────

fn read_network_graph(
    kv_store: &WolfeKVStore,
    _network: bitcoin::Network,
    logger: Arc<WolfeLogger>,
) -> Option<WolfeNetworkGraph> {
    let data = kv_store.read("network_graph", "", "graph").ok()?;
    let mut reader = lightning::io::Cursor::new(&data);
    NetworkGraph::read(&mut reader, logger).ok()
}

fn read_scorer(
    kv_store: &WolfeKVStore,
    network_graph: Arc<WolfeNetworkGraph>,
    logger: Arc<WolfeLogger>,
) -> Option<WolfeScorer> {
    let data = kv_store.read("scorer", "", "scorer").ok()?;
    let mut reader = lightning::io::Cursor::new(&data);
    ProbabilisticScorer::read(
        &mut reader,
        (
            ProbabilisticScoringDecayParameters::default(),
            network_graph,
            logger,
        ),
    )
    .ok()
}

// ── Channel Manager load/create ─────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn load_or_create_channel_manager(
    kv_store: &Arc<WolfeKVStore>,
    keys_manager: Arc<KeysManager>,
    fee_estimator: Arc<WolfeFeeEstimator>,
    chain_monitor: Arc<WolfeChainMonitor>,
    broadcaster: Arc<WolfeBroadcaster>,
    router: Arc<WolfeRouter>,
    message_router: Arc<WolfeMessageRouter>,
    logger: Arc<WolfeLogger>,
    user_config: UserConfig,
    network: bitcoin::Network,
    best_block_hash: BlockHash,
    best_block_height: u32,
) -> Result<WolfeChannelManager, LightningError> {
    // Try to load persisted channel manager
    let serialized = kv_store.read("channel_manager", "", "manager");

    match serialized {
        Ok(data) => {
            info!("loading persisted channel manager");

            // First, restore channel monitors
            let monitor_keys = kv_store.list("channel_monitors", "").unwrap_or_default();

            let mut channel_monitors = Vec::new();
            for key in &monitor_keys {
                match kv_store.read("channel_monitors", "", key) {
                    Ok(monitor_data) => {
                        let mut reader = lightning::io::Cursor::new(&monitor_data);
                        match <(
                            BlockHash,
                            lightning::chain::channelmonitor::ChannelMonitor<
                                lightning::sign::InMemorySigner,
                            >,
                        )>::read(
                            &mut reader, (keys_manager.as_ref(), keys_manager.as_ref())
                        ) {
                            Ok((_blockhash, monitor)) => {
                                channel_monitors.push(monitor);
                            }
                            Err(e) => {
                                warn!(key, ?e, "failed to deserialize channel monitor");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(key, ?e, "failed to read channel monitor");
                    }
                }
            }

            let monitor_refs: Vec<
                &lightning::chain::channelmonitor::ChannelMonitor<lightning::sign::InMemorySigner>,
            > = channel_monitors.iter().collect();

            let read_args = ChannelManagerReadArgs::new(
                keys_manager.clone(),
                keys_manager.clone(),
                keys_manager.clone(),
                fee_estimator.clone(),
                chain_monitor.clone(),
                broadcaster.clone(),
                router.clone(),
                message_router.clone(),
                logger.clone(),
                user_config.clone(),
                monitor_refs,
            );

            let mut reader = lightning::io::Cursor::new(&data);
            match <(BlockHash, WolfeChannelManager)>::read(&mut reader, read_args) {
                Ok((_blockhash, manager)) => {
                    info!(
                        channels = manager.list_channels().len(),
                        "channel manager restored"
                    );
                    Ok(manager)
                }
                Err(e) => {
                    warn!(?e, "failed to deserialize channel manager — creating fresh");
                    Ok(create_fresh_channel_manager(
                        keys_manager,
                        fee_estimator,
                        chain_monitor,
                        broadcaster,
                        router,
                        message_router,
                        logger,
                        user_config,
                        network,
                        best_block_hash,
                        best_block_height,
                    ))
                }
            }
        }
        Err(_) => {
            info!("no persisted channel manager found — creating fresh");
            Ok(create_fresh_channel_manager(
                keys_manager,
                fee_estimator,
                chain_monitor,
                broadcaster,
                router,
                message_router,
                logger,
                user_config,
                network,
                best_block_hash,
                best_block_height,
            ))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn create_fresh_channel_manager(
    keys_manager: Arc<KeysManager>,
    fee_estimator: Arc<WolfeFeeEstimator>,
    chain_monitor: Arc<WolfeChainMonitor>,
    broadcaster: Arc<WolfeBroadcaster>,
    router: Arc<WolfeRouter>,
    message_router: Arc<WolfeMessageRouter>,
    logger: Arc<WolfeLogger>,
    user_config: UserConfig,
    network: bitcoin::Network,
    best_block_hash: BlockHash,
    best_block_height: u32,
) -> WolfeChannelManager {
    ChannelManager::new(
        fee_estimator,
        chain_monitor,
        broadcaster,
        router,
        message_router,
        logger,
        keys_manager.clone(),
        keys_manager.clone(),
        keys_manager,
        user_config,
        ChainParameters {
            network,
            best_block: BestBlock::new(best_block_hash, best_block_height),
        },
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32,
    )
}

// ── Seed management ─────────────────────────────────────────────────────

const LN_SEED_KEY: &str = "ln_seed";

fn load_or_create_seed(store: &NodeStore) -> Result<[u8; 32], LightningError> {
    let txn = store.read_txn()?;
    if let Some(bytes) = wolfe_store::MetaStore::get(&txn, LN_SEED_KEY)? {
        if bytes.len() == 32 {
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&bytes);
            info!("loaded existing Lightning seed");
            return Ok(seed);
        }
    }
    drop(txn);

    // Generate new seed
    let seed: [u8; 32] = rand::random();
    let write_txn = store.write_txn()?;
    wolfe_store::MetaStore::set(&write_txn, LN_SEED_KEY, &seed)?;
    write_txn
        .commit()
        .map_err(|e| LightningError::Persistence(e.to_string()))?;

    info!("generated and persisted new Lightning seed");
    Ok(seed)
}
