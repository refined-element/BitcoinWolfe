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

use wolfe_wallet::NodeWallet;

use bitcoin::block::Header;
use bitcoin::BlockHash;
use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::{BestBlock, Confirm, Watch};
use lightning::ln::channelmanager::{
    Bolt11InvoiceParameters, ChainParameters, ChannelManager, ChannelManagerReadArgs, PaymentId,
    Retry,
};
use lightning::ln::peer_handler::{IgnoringMessageHandler, MessageHandler};
use lightning::onion_message::messenger::{DefaultMessageRouter, OnionMessenger};
use lightning::routing::gossip::NetworkGraph;
use lightning::routing::gossip::P2PGossipSync;
use lightning::routing::router::{DefaultRouter, RouteParametersConfig};
use lightning::routing::scoring::{
    ProbabilisticScorer, ProbabilisticScoringDecayParameters, ProbabilisticScoringFeeParameters,
};
use lightning::routing::utxo::UtxoLookup;
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
    keys_manager: Arc<KeysManager>,
    broadcaster: Arc<WolfeBroadcaster>,
    fee_estimator: Arc<WolfeFeeEstimator>,
    kv_store: Arc<WolfeKVStore>,
    event_tx: mpsc::Sender<LightningEvent>,
    has_channels: AtomicBool,
    network: bitcoin::Network,
    wallet: Mutex<Option<Arc<Mutex<NodeWallet>>>>,
    seed: [u8; 32],
    /// Shared set of claimed payment hashes for L402 verification.
    paid_invoices: Arc<dashmap::DashMap<[u8; 32], u64>>,
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
            monitor_persister.clone(),
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
            &monitor_persister,
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

        // ── P2P Gossip Sync ──────────────────────────────────────────────
        let gossip_sync = Arc::new(P2PGossipSync::new(
            network_graph.clone(),
            None::<Arc<dyn UtxoLookup + Send + Sync>>,
            logger.clone(),
        ));

        // ── LDK Peer Manager ───────────────────────────────────────────
        let ephemeral_bytes: [u8; 32] = rand::random();
        let lightning_msg_handler = MessageHandler {
            chan_handler: channel_manager.clone(),
            route_handler: gossip_sync,
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
                keys_manager,
                broadcaster,
                fee_estimator,
                kv_store,
                event_tx,
                has_channels,
                network,
                wallet: Mutex::new(None),
                seed,
                paid_invoices: Arc::new(dashmap::DashMap::new()),
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

    /// Handle a chain reorganization.
    ///
    /// Rewinds LDK's best-block to the fork height so that re-fed blocks
    /// after the reorg update channel state correctly. Called from the main
    /// loop when the sync engine detects a reorg.
    pub fn handle_reorg(&self, fork_height: u32, fork_header: &Header) {
        info!(fork_height, "notifying LDK of chain reorganization");
        self.channel_manager
            .best_block_updated(fork_header, fork_height);
        self.chain_monitor
            .best_block_updated(fork_header, fork_height);
    }

    /// Get the height of LDK's current best block.
    ///
    /// After restart, this tells us where LDK left off so we can feed
    /// missed blocks via `block_connected`.
    pub fn best_block_height(&self) -> u32 {
        self.channel_manager.current_best_block().height
    }

    /// Get the node's public key.
    pub fn node_id(&self) -> bitcoin::secp256k1::PublicKey {
        self.channel_manager.get_our_node_id()
    }

    /// Get the Lightning seed (for L402 secret derivation).
    pub fn seed(&self) -> Option<[u8; 32]> {
        Some(self.seed)
    }

    /// Get the shared paid_invoices map for L402 verification.
    pub fn paid_invoices(&self) -> Arc<dashmap::DashMap<[u8; 32], u64>> {
        self.paid_invoices.clone()
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

    /// Inject a wallet for channel funding. Called after both wallet and
    /// Lightning are initialized (they have independent lifetimes).
    pub fn set_wallet(&self, wallet: Arc<Mutex<NodeWallet>>) {
        *self.wallet.lock().unwrap() = Some(wallet);
        info!("wallet injected into Lightning manager");
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
        // Guard: don't open channels during IBD — fee estimates are unreliable
        // and the funding tx confirmation won't be tracked properly.
        if self.best_block_height() < 100 {
            return Err(LightningError::Channel(
                "node still syncing — cannot open channel during IBD".into(),
            ));
        }
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

    /// Close a channel cooperatively or force-close.
    pub fn close_channel(
        &self,
        channel_id: lightning::ln::types::ChannelId,
        counterparty_node_id: bitcoin::secp256k1::PublicKey,
        force: bool,
    ) -> Result<(), LightningError> {
        if force {
            self.channel_manager
                .force_close_broadcasting_latest_txn(
                    &channel_id,
                    &counterparty_node_id,
                    "user-requested force close".to_string(),
                )
                .map_err(|e| LightningError::Channel(format!("{:?}", e)))?;
            info!(%counterparty_node_id, "force-closing channel");
        } else {
            self.channel_manager
                .close_channel(&channel_id, &counterparty_node_id)
                .map_err(|e| LightningError::Channel(format!("{:?}", e)))?;
            info!(%counterparty_node_id, "cooperative close initiated");
        }
        Ok(())
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

    /// Process pending LDK events and run periodic maintenance.
    ///
    /// Must be called regularly (every ~1s) from a background task.
    /// Handles: event processing, timer ticks, and periodic persistence.
    pub async fn tick(&self) {
        use crate::event_handler::{handle_ldk_event, EventContext};

        let wallet = self.wallet.lock().unwrap().clone();
        let ctx = EventContext {
            channel_manager: self.channel_manager.clone(),
            keys_manager: self.keys_manager.clone(),
            broadcaster: self.broadcaster.clone(),
            fee_estimator: self.fee_estimator.clone(),
            kv_store: self.kv_store.clone(),
            config: self.config.clone(),
            wallet,
            network: self.network,
            paid_invoices: self.paid_invoices.clone(),
        };

        // Process channel manager events
        let ctx = Arc::new(ctx);
        let tx = self.event_tx.clone();
        let ctx_cm = ctx.clone();
        self.channel_manager
            .process_pending_events_async(|event| {
                let tx = tx.clone();
                let ctx = ctx_cm.clone();
                async move {
                    info!("processing LDK channel_manager event: {:?}", event);
                    handle_ldk_event(event, &ctx, &tx).await;
                    Ok(())
                }
            })
            .await;

        // Process chain monitor events
        let tx = self.event_tx.clone();
        let ctx_mon = ctx.clone();
        self.chain_monitor
            .process_pending_events_async(|event| {
                let tx = tx.clone();
                let ctx = ctx_mon.clone();
                async move {
                    info!("processing LDK chain_monitor event: {:?}", event);
                    handle_ldk_event(event, &ctx, &tx).await;
                    Ok(())
                }
            })
            .await;

        // Process pending HTLC forwards — this is what generates
        // PaymentClaimable events for incoming payments. Must be called
        // regularly or incoming HTLCs will never be settled.
        self.channel_manager.process_pending_htlc_forwards();

        // LDK timer ticks (manages retries, channel state, etc.)
        self.channel_manager.timer_tick_occurred();
        self.peer_manager.timer_tick_occurred();
        // Process pending peer manager events (sends gossip queries, etc.)
        self.peer_manager.process_events();
    }

    /// Persist all LDK state. Called periodically and on shutdown.
    pub fn persist_state(&self) {
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

    /// Graceful shutdown.
    pub fn shutdown(&self) {
        info!("lightning manager shutting down");
        self.persist_state();
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
    monitor_persister: &Arc<WolfeMonitorPersister>,
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

            // Restore channel monitors via MonitorUpdatingPersister (reads from "monitors" namespace)
            let mut channel_monitors =
                match monitor_persister.read_all_channel_monitors_with_updates() {
                    Ok(monitors) => {
                        info!(count = monitors.len(), "loaded channel monitors");
                        monitors
                    }
                    Err(e) => {
                        warn!(?e, "failed to read channel monitors — starting with none");
                        Vec::new()
                    }
                };

            let monitor_refs: Vec<
                &lightning::chain::channelmonitor::ChannelMonitor<lightning::sign::InMemorySigner>,
            > = channel_monitors.iter().map(|(_bh, m)| m).collect();

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
                    // Register all monitors with chain_monitor (C3)
                    for (_blockhash, monitor) in channel_monitors.drain(..) {
                        let channel_id = monitor.channel_id();
                        if chain_monitor.watch_channel(channel_id, monitor).is_err() {
                            warn!(%channel_id, "failed to register channel monitor with chain_monitor");
                        }
                    }

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
