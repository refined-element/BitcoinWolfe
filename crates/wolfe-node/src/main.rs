mod metrics;
mod sync;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

use wolfe_lightning::LightningManager;
use wolfe_mempool::Mempool;
use wolfe_nostr::{NostrBridge, NostrEvent, NostrSender};
use wolfe_p2p::manager::PeerEvent;
use wolfe_p2p::PeerManager;
use wolfe_rpc::server::NodeState;
use wolfe_rpc::RpcServer;
use wolfe_store::NodeStore;
use wolfe_types::Config;
use wolfe_wallet::NodeWallet;

use std::collections::HashMap;
use std::sync::Mutex;

use crate::metrics::NodeMetrics;
use crate::sync::SyncEngine;

#[derive(Parser)]
#[command(name = "wolfe")]
#[command(about = "BitcoinWolfe — A modern Bitcoin full node")]
#[command(version)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "wolfe.toml")]
    config: PathBuf,

    /// Bitcoin network (overrides config file)
    #[arg(short, long)]
    network: Option<String>,

    /// Data directory (overrides config file)
    #[arg(short, long)]
    datadir: Option<PathBuf>,

    /// Connect to a specific peer (overrides config and DNS seeds)
    #[arg(long)]
    connect: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the node (default)
    Start,
    /// Print the default configuration
    DefaultConfig,
    /// Print node version and build info
    Info,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command.as_ref().unwrap_or(&Commands::Start) {
        Commands::DefaultConfig => {
            let config = Config::default();
            println!("{}", toml::to_string_pretty(&config)?);
            return Ok(());
        }
        Commands::Info => {
            println!("BitcoinWolfe v{}", wolfe_types::VERSION);
            println!("User-Agent: {}", wolfe_types::user_agent());
            println!();
            println!("Architecture:");
            println!("  Consensus:  libbitcoinkernel (Bitcoin Core kernel)");
            println!("  Wallet:     BDK (Bitcoin Dev Kit) with SQLite");
            println!("  Storage:    redb (pure Rust ACID key-value store)");
            println!("  P2P:        Tokio async with BIP324 support");
            println!("  API:        REST + JSON-RPC (Bitcoin Core compatible)");
            println!("  Nostr:      Block announcements, fee oracle, NIP-98 auth");
            println!("  Metrics:    Prometheus-native");
            return Ok(());
        }
        Commands::Start => {}
    }

    // ── Load configuration ──────────────────────────────────────────────
    let mut config = Config::load(&cli.config)?;

    if let Some(network) = &cli.network {
        config.network.chain = network.clone();
    }
    if let Some(datadir) = &cli.datadir {
        config.storage.data_dir = datadir.clone();
    }
    if let Some(connect) = &cli.connect {
        config.p2p.connect = vec![connect.clone()];
    }

    // ── Initialize logging ──────────────────────────────────────────────
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    if let Some(ref log_file) = config.logging.file {
        // Log to both stdout and file
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)
            .expect("failed to open log file");

        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::sync::Mutex::new(file))
            .with_target(true)
            .with_ansi(false);

        let stdout_layer = tracing_subscriber::fmt::layer().with_target(true);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(stdout_layer)
            .with(file_layer)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .init();
    }

    let network = config.network.bitcoin_network()?;
    let data_dir = config.data_dir();

    info!(
        version = wolfe_types::VERSION,
        chain = config.network.chain,
        data_dir = %data_dir.display(),
        "starting BitcoinWolfe"
    );

    // ── Shared shutdown signal ──────────────────────────────────────────
    let shutdown = Arc::new(AtomicBool::new(false));

    // ── Initialize storage ──────────────────────────────────────────────
    let store_path = data_dir.join("nodestore.redb");
    let store = Arc::new(NodeStore::open(&store_path)?);
    info!(path = %store_path.display(), "storage initialized");

    // Check existing sync progress
    {
        let txn = store.read_txn()?;
        if let Some(height) = wolfe_store::MetaStore::sync_height(&txn)? {
            info!(height, "resuming from stored sync progress");
        }
    }

    // ── Initialize mempool ──────────────────────────────────────────────
    let mempool = Arc::new(Mempool::new(config.mempool.clone()));
    info!(
        max_mb = config.mempool.max_size_mb,
        min_fee = config.mempool.min_fee_rate,
        rbf = config.mempool.full_rbf,
        "mempool initialized"
    );

    // ── Initialize metrics ──────────────────────────────────────────────
    let node_metrics = Arc::new(NodeMetrics::new());

    if config.metrics.enabled {
        let metrics_addr = config.metrics.listen.clone();
        let metrics_clone = node_metrics.clone();
        tokio::spawn(async move {
            metrics::serve_metrics(metrics_addr, metrics_clone).await;
        });
    }

    // ── Initialize consensus engine ─────────────────────────────────────
    let consensus_engine = {
        let chain_type = wolfe_consensus::chain_type_from_network(network);
        let kernel_dir = data_dir.join("kernel");
        match wolfe_consensus::ConsensusEngine::new(&kernel_dir, chain_type) {
            Ok(engine) => {
                let height = engine.chain_height();
                info!(
                    kernel_height = height,
                    chain = ?chain_type,
                    "consensus engine initialized"
                );
                Some(Arc::new(engine))
            }
            Err(e) => {
                warn!(
                    ?e,
                    "consensus engine failed to initialize — running in header-only mode"
                );
                None
            }
        }
    };

    // ── Initialize sync engine ──────────────────────────────────────────
    let mut sync_engine = SyncEngine::new(store.clone(), network, shutdown.clone());
    if let Some(ref engine) = consensus_engine {
        sync_engine.set_consensus(engine.clone());
    }
    info!(
        headers = sync_engine.tip_height(),
        validated = sync_engine.validated_height(),
        "sync engine initialized"
    );

    // ── Initialize wallet (optional) ────────────────────────────────────
    let wallet: Option<Arc<Mutex<NodeWallet>>> = if config.wallet.enabled {
        if config.wallet.external_descriptor.is_empty()
            || config.wallet.internal_descriptor.is_empty()
        {
            return Err(anyhow::anyhow!(
                "wallet.enabled=true but no descriptors provided. \
                 Set wallet.external_descriptor and wallet.internal_descriptor in your config file. \
                 Example: wpkh(tprv.../84'/1'/0'/0/*) for testnet, wpkh(xprv.../84'/0'/0'/0/*) for mainnet."
            ));
        }

        let wallet_db = data_dir.join(&config.wallet.db_path);
        let ext_desc = config.wallet.external_descriptor.clone();
        let int_desc = config.wallet.internal_descriptor.clone();

        match NodeWallet::open_with_encryption(
            &wallet_db,
            network,
            ext_desc,
            int_desc,
            config.wallet.encryption_key.as_deref(),
        ) {
            Ok(w) => {
                let balance = w.balance();
                info!(
                    confirmed = balance.confirmed,
                    pending = balance.trusted_pending,
                    "wallet initialized"
                );
                Some(Arc::new(Mutex::new(w)))
            }
            Err(e) => {
                warn!(?e, "wallet failed to initialize — running without wallet");
                None
            }
        }
    } else {
        None
    };

    // ── Initialize Lightning manager (optional) ────────────────────────
    let lightning_manager: Option<Arc<LightningManager>> = if config.lightning.enabled {
        let best_hash = sync_engine.tip_hash();
        let best_height = sync_engine.validated_height() as u32;
        match LightningManager::new(
            config.lightning.clone(),
            network,
            &data_dir,
            store.clone(),
            mempool.clone(),
            best_hash,
            best_height,
        ) {
            Ok((manager, _sender, _broadcast_rx)) => {
                let manager = Arc::new(manager);
                let node_id = manager.node_id();
                info!(
                    node_id = %node_id,
                    port = config.lightning.listen_port,
                    "Lightning manager initialized"
                );

                // Start Lightning P2P listener
                let mgr = manager.clone();
                tokio::spawn(async move {
                    if let Err(e) = mgr.start().await {
                        error!(?e, "Lightning P2P listener failed");
                    }
                });

                Some(manager)
            }
            Err(e) => {
                warn!(?e, "Lightning manager failed to initialize");
                None
            }
        }
    } else {
        None
    };

    // ── Catch up Lightning with blocks missed during shutdown ─────────
    if let (Some(ref ln), Some(ref engine)) = (&lightning_manager, &consensus_engine) {
        let ldk_height = ln.best_block_height();
        let chain_height = engine.chain_height();

        if chain_height > 0 && (ldk_height as i32) < chain_height {
            let from = ldk_height + 1;
            let to = chain_height as u32;
            let gap = to - from + 1;

            if !ln.channel_manager().list_channels().is_empty() {
                // Has channels — feed every missed block so LDK can confirm funding txs
                info!(
                    ldk_height,
                    chain_height,
                    blocks_to_feed = gap,
                    "catching up Lightning with missed blocks"
                );
                for height in from..=to {
                    match engine.read_block_data_at_height(height) {
                        Ok(kernel_block) => match kernel_block.consensus_encode() {
                            Ok(bytes) => {
                                match bitcoin::consensus::deserialize::<bitcoin::Block>(&bytes) {
                                    Ok(block) => ln.block_connected(&block, height),
                                    Err(e) => {
                                        warn!(height, ?e, "LDK catch-up: deserialize failed");
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(height, ?e, "LDK catch-up: encode failed");
                                break;
                            }
                        },
                        Err(e) => {
                            warn!(height, ?e, "LDK catch-up: block read failed");
                            break;
                        }
                    }
                }
                info!("Lightning catch-up complete");
            } else {
                // No channels — just update LDK's best block to the tip
                info!(
                    ldk_height,
                    chain_height, "no channels — fast-forwarding Lightning to chain tip"
                );
                if let Ok(kernel_block) = engine.read_block_data_at_height(to) {
                    if let Ok(bytes) = kernel_block.consensus_encode() {
                        if let Ok(block) = bitcoin::consensus::deserialize::<bitcoin::Block>(&bytes)
                        {
                            ln.block_connected(&block, to);
                        }
                    }
                }
            }
        }
    }

    // ── Initialize RPC server ───────────────────────────────────────────
    let mut node_state = NodeState::new(config.network.chain.clone(), mempool.clone());
    node_state.set_shutdown_flag(shutdown.clone());
    if let Some(ref engine) = consensus_engine {
        node_state.set_consensus(engine.clone());
    }
    if let Some(ref w) = wallet {
        node_state.set_wallet(w.clone());
    }
    if let Some(ref ln) = lightning_manager {
        node_state.set_lightning(ln.clone());
    }
    let rpc_state = Arc::new(node_state);

    if config.rpc.enabled {
        let rpc_server = RpcServer::new(config.rpc.clone(), rpc_state.clone())
            .with_nostr_config(config.nostr.clone());
        tokio::spawn(async move {
            if let Err(e) = rpc_server.start().await {
                error!(?e, "RPC server failed");
            }
        });
        info!(addr = config.rpc.listen, "RPC server started");
    }

    // ── Initialize Nostr bridge (optional) ────────────────────────────────
    let nostr_sender: Option<NostrSender> = if config.nostr.enabled {
        match NostrBridge::new(
            config.nostr.secret_key.as_deref(),
            &config.nostr.relays,
            config.network.chain.clone(),
            mempool.clone(),
            config.nostr.fee_oracle_interval_secs,
        )
        .await
        {
            Ok((bridge, sender)) => {
                info!(
                    relays = config.nostr.relays.len(),
                    block_announcements = config.nostr.block_announcements,
                    fee_oracle = config.nostr.fee_oracle,
                    "Nostr bridge initialized"
                );
                tokio::spawn(async move {
                    bridge.run().await;
                });
                Some(sender)
            }
            Err(e) => {
                warn!(?e, "Nostr bridge failed to initialize");
                None
            }
        }
    } else {
        None
    };

    // ── Initialize P2P manager ──────────────────────────────────────────
    let mut peer_manager = PeerManager::new(config.p2p.clone(), network);
    let mut event_rx = peer_manager
        .take_event_rx()
        .expect("event_rx already taken");

    let peer_manager = Arc::new(peer_manager);
    peer_manager.set_best_height(sync_engine.tip_height());

    let pm = peer_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = pm.start().await {
            error!(?e, "P2P manager failed to start");
        }
    });

    info!(
        listen = config.p2p.listen,
        max_inbound = config.p2p.max_inbound,
        max_outbound = config.p2p.max_outbound,
        "P2P manager started"
    );

    // ── Block pruning ──────────────────────────────────────────────────
    // libbitcoinkernel v0.2 doesn't expose pruning through its API, so we
    // implement file-level pruning: periodically delete the oldest blk*.dat
    // and rev*.dat files when total block storage exceeds the target.
    // The UTXO set and block index are never pruned.
    if config.storage.prune_target_mb > 0 {
        let prune_target_bytes = config.storage.prune_target_mb * 1024 * 1024;
        let blocks_dir = data_dir.join("kernel").join("blocks");
        let prune_shutdown = shutdown.clone();
        info!(
            target_mb = config.storage.prune_target_mb,
            "block pruning enabled"
        );
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                if prune_shutdown.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(e) = prune_block_files(&blocks_dir, prune_target_bytes) {
                    warn!(?e, "block file pruning failed");
                }
            }
        });
    }

    // ── Mempool maintenance (trim + expiry) ─────────────────────────────
    let mempool_maint = mempool.clone();
    let mempool_maint_shutdown = shutdown.clone();
    let mempool_expiry_hours = config.mempool.expiry_hours;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if mempool_maint_shutdown.load(Ordering::Relaxed) {
                break;
            }
            mempool_maint.trim();
            mempool_maint.expire(std::time::Duration::from_secs(mempool_expiry_hours * 3600));
        }
    });

    // ── Progress reporter ───────────────────────────────────────────────
    let progress_headers = sync_engine.progress().headers_height.clone();
    let progress_blocks = sync_engine.progress().blocks_height.clone();
    let progress_peers = sync_engine.progress().peer_count.clone();
    let progress_shutdown = shutdown.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            if progress_shutdown.load(Ordering::Relaxed) {
                break;
            }
            let h = progress_headers.load(Ordering::Relaxed);
            let b = progress_blocks.load(Ordering::Relaxed);
            let p = progress_peers.load(Ordering::Relaxed);
            if h > 0 {
                info!(headers = h, blocks = b, peers = p, "sync progress");
            }
        }
    });

    info!("BitcoinWolfe is running. Press Ctrl+C to stop.");

    // Track peer_id → (addr, inbound) so we can clean up metrics and RPC state on disconnect
    let mut peer_addrs: HashMap<u64, (std::net::SocketAddr, bool)> = HashMap::new();

    // ── Main event loop ─────────────────────────────────────────────────
    loop {
        tokio::select! {
            // Handle P2P events
            Some(event) = event_rx.recv() => {
                match event {
                    PeerEvent::Connected(info) => {
                        info!(
                            addr = %info.addr,
                            user_agent = %info.user_agent,
                            height = info.start_height,
                            "peer connected"
                        );

                        node_metrics.peers_connected.fetch_add(1, Ordering::Relaxed);
                        if info.inbound {
                            node_metrics.peers_inbound.fetch_add(1, Ordering::Relaxed);
                        } else {
                            node_metrics.peers_outbound.fetch_add(1, Ordering::Relaxed);
                        }

                        // Track addr and direction for cleanup on disconnect
                        peer_addrs.insert(info.id.0, (info.addr, info.inbound));

                        // Persist peer to store
                        if let Ok(write_txn) = store.write_txn() {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let record = wolfe_store::PeerRecord {
                                addr: info.addr.to_string(),
                                services: 0,
                                last_seen: now,
                                first_seen: now,
                                connection_count: 1,
                                fail_count: 0,
                                user_agent: info.user_agent.clone(),
                            };
                            let _ = wolfe_store::PeerStore::upsert(&write_txn, &record);
                            let _ = write_txn.commit();
                        }

                        // Update RPC state with new peer
                        let best_h = consensus_engine
                            .as_ref()
                            .map(|e| e.chain_height().max(0) as u64)
                            .unwrap_or(sync_engine.tip_height());
                        rpc_state.set_best_height(best_h);
                        rpc_state.add_peer_info(wolfe_types::PeerInfoSnapshot {
                            addr: info.addr,
                            user_agent: info.user_agent.clone(),
                            version: info.version,
                            inbound: info.inbound,
                            v2_transport: info.v2_transport,
                            start_height: info.start_height,
                        });

                        // Tell sync engine about the new peer
                        if let Some(msg) = sync_engine.on_peer_connected(info.id, info.start_height) {
                            let _ = peer_manager.send_to_peer(info.id, msg).await;
                        }
                    }

                    PeerEvent::Disconnected(peer_id) => {
                        info!(peer = ?peer_id, "peer disconnected");

                        // Safely decrement counters (saturating to avoid underflow)
                        node_metrics.peers_connected.fetch_update(
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                            |v| Some(v.saturating_sub(1)),
                        ).ok();

                        sync_engine.on_peer_disconnected(peer_id);

                        // If the sync peer disconnected, try to pick a replacement
                        if let Some((new_peer, msg)) = sync_engine.try_select_sync_peer() {
                            let _ = peer_manager.send_to_peer(new_peer, msg).await;
                        }

                        // Remove from RPC state and decrement direction-specific counter
                        if let Some((addr, was_inbound)) = peer_addrs.remove(&peer_id.0) {
                            rpc_state.remove_peer_info(addr);
                            let counter = if was_inbound {
                                &node_metrics.peers_inbound
                            } else {
                                &node_metrics.peers_outbound
                            };
                            counter.fetch_update(
                                Ordering::Relaxed,
                                Ordering::Relaxed,
                                |v| Some(v.saturating_sub(1)),
                            ).ok();
                        }
                    }

                    PeerEvent::Message(peer_id, msg) => {
                        node_metrics.messages_received.fetch_add(1, Ordering::Relaxed);

                        // Let the sync engine process the message
                        if let Some((target_peer, response)) = sync_engine.handle_message(peer_id, msg) {
                            let _ = peer_manager.send_to_peer(target_peer, response).await;
                        }

                        // Remove confirmed txs from mempool
                        let confirmed = sync_engine.take_confirmed_txids();
                        if !confirmed.is_empty() {
                            mempool.remove_for_block(&confirmed);
                        }

                        // Add received transactions to mempool and wallet
                        let pending_txs = sync_engine.take_pending_txs();
                        if !pending_txs.is_empty() {
                            // Feed unconfirmed txs to wallet
                            if let Some(ref wallet) = wallet {
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs();
                                let unconfirmed: Vec<_> = pending_txs
                                    .iter()
                                    .map(|tx| (tx.clone(), now))
                                    .collect();
                                if let Ok(mut w) = wallet.lock() {
                                    if let Err(e) = w.apply_unconfirmed_txs(unconfirmed) {
                                        debug!(?e, "wallet failed to process unconfirmed txs");
                                    }
                                }
                            }

                            // Add to mempool
                            for tx in pending_txs {
                                let txid = tx.compute_txid();
                                // Fee is 0 since we lack UTXO set for input lookup.
                                // TODO (HIGH-007): validate inputs against UTXO set for proper fee calculation.
                                if let Err(e) = mempool.add(tx, 0) {
                                    debug!(%txid, ?e, "tx rejected from mempool");
                                }
                            }
                        }

                        // Feed validated blocks to the wallet and Nostr bridge
                        if let Some((block, height)) = sync_engine.take_validated_block() {
                            // Feed to wallet
                            if let Some(ref wallet) = wallet {
                                if let Ok(mut w) = wallet.lock() {
                                    if let Err(e) = w.apply_block(&block, height) {
                                        warn!(height, ?e, "wallet failed to process block");
                                    }
                                }
                            }

                            // Feed to Lightning manager
                            if let Some(ref ln) = lightning_manager {
                                ln.block_connected(&block, height);
                            }

                            // Publish block announcement to Nostr
                            if let Some(ref sender) = nostr_sender {
                                if config.nostr.block_announcements {
                                    let hash = block.block_hash().to_string();
                                    let timestamp = block.header.time as u64;
                                    let tx_count = block.txdata.len();
                                    let size = bitcoin::consensus::serialize(&block).len();
                                    sender.send(NostrEvent::BlockValidated {
                                        height: height as u64,
                                        hash,
                                        timestamp,
                                        tx_count,
                                        size,
                                    }).await;
                                }
                            }
                        }

                        // Update shared metrics
                        node_metrics.headers_height.store(
                            sync_engine.tip_height(),
                            Ordering::Relaxed,
                        );
                        node_metrics.mempool_txs.store(
                            mempool.len() as u64,
                            Ordering::Relaxed,
                        );

                        // Update Lightning metrics
                        if let Some(ref ln) = lightning_manager {
                            let channels = ln.channel_manager().list_channels();
                            let active = channels.iter().filter(|c| c.is_usable).count();
                            let capacity: u64 = channels.iter().map(|c| c.channel_value_satoshis).sum();
                            node_metrics.ln_channels_active.store(active as u64, Ordering::Relaxed);
                            node_metrics.ln_capacity_sat.store(capacity, Ordering::Relaxed);
                            node_metrics.ln_peers_connected.store(
                                ln.peer_manager().list_peers().len() as u64,
                                Ordering::Relaxed,
                            );
                        }

                        // Keep RPC state in sync — report kernel's actual
                        // validated chain height, not the header tip which
                        // resets on resync and misleads progress tracking.
                        let best_h = consensus_engine
                            .as_ref()
                            .map(|e| e.chain_height().max(0) as u64)
                            .unwrap_or(sync_engine.tip_height());
                        rpc_state.set_best_height(best_h);
                        rpc_state.set_headers_height(sync_engine.tip_height());
                        rpc_state.set_best_hash(sync_engine.tip_hash().to_string());
                        rpc_state.set_syncing(
                            sync_engine.progress().state != sync::SyncState::Synced
                        );
                    }

                    PeerEvent::Banned(peer_id, reason) => {
                        warn!(peer = ?peer_id, %reason, "peer banned");

                        // Persist ban to store
                        if let Some((addr, _)) = peer_addrs.get(&peer_id.0) {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let ban_until = now + config.p2p.ban_duration_secs;
                            if let Ok(write_txn) = store.write_txn() {
                                let _ = wolfe_store::PeerStore::ban(
                                    &write_txn,
                                    &addr.to_string(),
                                    ban_until,
                                );
                                let _ = write_txn.commit();
                            }
                        }
                    }
                }
            }

            // Shutdown on Ctrl+C
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                shutdown.store(true, Ordering::Relaxed);
                break;
            }

            // Check for RPC-triggered shutdown and stalled block downloads
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                if shutdown.load(Ordering::Relaxed) {
                    info!("shutdown triggered via RPC stop command");
                    break;
                }
                // Detect and recover from stalled block downloads
                if let Some((stall_peer, stall_msg)) = sync_engine.check_stall() {
                    let _ = peer_manager.send_to_peer(stall_peer, stall_msg).await;
                }
            }
        }
    }

    // ── Graceful shutdown ───────────────────────────────────────────────
    if let Some(ref ln) = lightning_manager {
        ln.shutdown();
    }

    if let Some(ref engine) = consensus_engine {
        if let Err(e) = engine.interrupt() {
            warn!(?e, "failed to interrupt consensus engine");
        }
    }

    info!(
        headers = sync_engine.tip_height(),
        validated = sync_engine.validated_height(),
        peers = peer_manager.peer_count(),
        "BitcoinWolfe shutting down"
    );

    Ok(())
}

/// Prune the oldest blk*.dat / rev*.dat files when total block storage
/// exceeds the prune target. Files are **truncated to zero bytes** rather
/// than deleted so that the kernel's block index (which still references
/// them) passes its "all blk files present" startup check.
///
/// Keeps the newest 10 file pairs untouched to avoid interfering with
/// the kernel's active write file.
fn prune_block_files(blocks_dir: &std::path::Path, target_bytes: u64) -> Result<()> {
    use std::fs;

    // Collect blk*.dat files sorted by number (ascending = oldest first).
    // Only count files with actual content (size > 0) toward the total.
    let mut blk_files: Vec<(u32, u64)> = Vec::new();
    let mut total_bytes: u64 = 0;

    for entry in fs::read_dir(blocks_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let size = entry.metadata()?.len();
        total_bytes += size;

        // Match blk00000.dat pattern — skip already-pruned (0-byte) files.
        if name_str.starts_with("blk") && name_str.ends_with(".dat") {
            if let Ok(num) = name_str[3..name_str.len() - 4].parse::<u32>() {
                if size > 0 {
                    blk_files.push((num, size));
                }
            }
        }
    }

    if total_bytes <= target_bytes {
        return Ok(());
    }

    blk_files.sort_by_key(|(num, _)| *num);

    // Never prune the last 10 file pairs (kernel writes to the newest).
    let prunable = blk_files.len().saturating_sub(10);
    if prunable == 0 {
        return Ok(());
    }

    let excess = total_bytes - target_bytes;
    let mut freed: u64 = 0;
    let mut pruned_count = 0;

    for &(num, blk_size) in blk_files.iter().take(prunable) {
        if freed >= excess {
            break;
        }

        let blk_path = blocks_dir.join(format!("blk{:05}.dat", num));
        let rev_path = blocks_dir.join(format!("rev{:05}.dat", num));

        // Truncate to zero bytes instead of deleting. This frees disk
        // space while keeping the file entry for the kernel's block index.
        let mut pair_freed = 0u64;
        if blk_path.exists() {
            pair_freed += blk_size;
            fs::File::create(&blk_path)?; // truncates to 0 bytes
        }
        if rev_path.exists() {
            let rev_size = rev_path.metadata().map(|m| m.len()).unwrap_or(0);
            if rev_size > 0 {
                pair_freed += rev_size;
                fs::File::create(&rev_path)?; // truncates to 0 bytes
            }
        }

        freed += pair_freed;
        pruned_count += 1;
    }

    if pruned_count > 0 {
        info!(
            pruned_files = pruned_count,
            freed_mb = freed / (1024 * 1024),
            remaining_mb = (total_bytes - freed) / (1024 * 1024),
            "pruned old block files"
        );
    }

    Ok(())
}
