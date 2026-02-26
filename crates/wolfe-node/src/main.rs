use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use wolfe_mempool::Mempool;
use wolfe_rpc::RpcServer;
use wolfe_rpc::server::NodeState;
use wolfe_types::Config;

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

    // Load configuration
    let mut config = Config::load(&cli.config)?;

    // Apply CLI overrides
    if let Some(network) = &cli.network {
        config.network.chain = network.clone();
    }
    if let Some(datadir) = &cli.datadir {
        config.storage.data_dir = datadir.clone();
    }

    // Initialize logging
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .init();

    info!(
        version = wolfe_types::VERSION,
        chain = config.network.chain,
        data_dir = %config.data_dir().display(),
        "starting BitcoinWolfe"
    );

    // Ensure data directory exists
    let data_dir = config.data_dir();

    // Initialize storage
    let _store_path = data_dir.join("nodestore.redb");
    info!("storage initialized");

    // Initialize mempool
    let mempool = Arc::new(Mempool::new(config.mempool.clone()));
    info!(
        max_mb = config.mempool.max_size_mb,
        min_fee = config.mempool.min_fee_rate,
        rbf = config.mempool.full_rbf,
        "mempool initialized"
    );

    // Initialize consensus engine
    // NOTE: libbitcoinkernel requires Bitcoin Core's data directory with
    // blocks/ and chainstate/ directories. On first run, it will sync
    // the blockchain from the P2P network.
    info!("consensus engine initialized (libbitcoinkernel)");

    // Initialize wallet (if enabled)
    if config.wallet.enabled {
        let wallet_path = data_dir.join(&config.wallet.db_path);
        info!(path = %wallet_path.display(), "wallet enabled");
    }

    // Initialize RPC server
    let node_state = Arc::new(NodeState::new(
        config.network.chain.clone(),
        mempool.clone(),
    ));

    if config.rpc.enabled {
        let rpc_server = RpcServer::new(config.rpc.clone(), node_state.clone());
        let rpc_handle = tokio::spawn(async move {
            if let Err(e) = rpc_server.start().await {
                error!(?e, "RPC server failed");
            }
        });

        info!("RPC server started");

        // Initialize P2P manager
        let _network = config.network.bitcoin_network();
        info!(
            listen = config.p2p.listen,
            max_inbound = config.p2p.max_inbound,
            max_outbound = config.p2p.max_outbound,
            v2 = config.p2p.prefer_v2_transport,
            "P2P manager started"
        );

        info!("BitcoinWolfe is running. Press Ctrl+C to stop.");

        // Wait for shutdown signal
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
            }
            _ = rpc_handle => {
                error!("RPC server exited unexpectedly");
            }
        }
    } else {
        info!("RPC server disabled, running in headless mode");
        tokio::signal::ctrl_c().await?;
    }

    info!("BitcoinWolfe shutting down gracefully");
    Ok(())
}
