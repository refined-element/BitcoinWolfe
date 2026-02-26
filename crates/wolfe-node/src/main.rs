mod metrics;
mod sync;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use wolfe_mempool::Mempool;
use wolfe_p2p::manager::PeerEvent;
use wolfe_p2p::PeerManager;
use wolfe_rpc::server::NodeState;
use wolfe_rpc::RpcServer;
use wolfe_store::NodeStore;
use wolfe_types::Config;

use std::collections::HashMap;

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

    // ── Initialize logging ──────────────────────────────────────────────
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .init();

    let network = config.network.bitcoin_network();
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

    // ── Initialize sync engine ──────────────────────────────────────────
    let mut sync_engine = SyncEngine::new(store.clone(), network, shutdown.clone());
    info!(tip = sync_engine.tip_height(), "sync engine initialized");

    // ── Initialize RPC server ───────────────────────────────────────────
    let rpc_state = Arc::new(NodeState::new(
        config.network.chain.clone(),
        mempool.clone(),
    ));

    if config.rpc.enabled {
        let rpc_server = RpcServer::new(config.rpc.clone(), rpc_state.clone());
        tokio::spawn(async move {
            if let Err(e) = rpc_server.start().await {
                error!(?e, "RPC server failed");
            }
        });
        info!(addr = config.rpc.listen, "RPC server started");
    }

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

    // ── Progress reporter ───────────────────────────────────────────────
    let progress_headers = sync_engine.progress().headers_height.clone();
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
            let p = progress_peers.load(Ordering::Relaxed);
            if h > 0 {
                info!(headers = h, peers = p, "sync progress");
            }
        }
    });

    info!("BitcoinWolfe is running. Press Ctrl+C to stop.");

    // Track peer_id → addr so we can clean up RPC state on disconnect
    let mut peer_addrs: HashMap<u64, std::net::SocketAddr> = HashMap::new();

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

                        // Track addr for cleanup on disconnect
                        peer_addrs.insert(info.id.0, info.addr);

                        // Update RPC state with new peer
                        rpc_state.set_best_height(sync_engine.tip_height());
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
                        node_metrics.peers_connected.fetch_sub(1, Ordering::Relaxed);
                        sync_engine.on_peer_disconnected(peer_id);

                        // Remove from RPC state
                        if let Some(addr) = peer_addrs.remove(&peer_id.0) {
                            rpc_state.remove_peer_info(addr);
                        }
                    }

                    PeerEvent::Message(peer_id, msg) => {
                        node_metrics.messages_received.fetch_add(1, Ordering::Relaxed);

                        // Let the sync engine process the message
                        if let Some((target_peer, response)) = sync_engine.handle_message(peer_id, msg) {
                            let _ = peer_manager.send_to_peer(target_peer, response).await;
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

                        // Keep RPC state in sync
                        rpc_state.set_best_height(sync_engine.tip_height());
                        rpc_state.set_best_hash(sync_engine.tip_hash().to_string());
                        rpc_state.set_syncing(
                            sync_engine.progress().state != sync::SyncState::Synced
                        );
                    }

                    PeerEvent::Banned(peer_id, reason) => {
                        warn!(peer = ?peer_id, %reason, "peer banned");
                    }
                }
            }

            // Shutdown on Ctrl+C
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                shutdown.store(true, Ordering::Relaxed);
                break;
            }
        }
    }

    // ── Graceful shutdown ───────────────────────────────────────────────
    info!(
        headers = sync_engine.tip_height(),
        peers = peer_manager.peer_count(),
        "BitcoinWolfe shutting down"
    );

    Ok(())
}
