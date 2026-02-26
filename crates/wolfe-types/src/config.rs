use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level configuration for a BitcoinWolfe node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub network: NetworkConfig,
    pub p2p: P2pConfig,
    pub rpc: RpcConfig,
    pub mempool: MempoolConfig,
    pub wallet: WalletConfig,
    pub storage: StorageConfig,
    pub logging: LoggingConfig,
    pub metrics: MetricsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: NetworkConfig::default(),
            p2p: P2pConfig::default(),
            rpc: RpcConfig::default(),
            mempool: MempoolConfig::default(),
            wallet: WalletConfig::default(),
            storage: StorageConfig::default(),
            logging: LoggingConfig::default(),
            metrics: MetricsConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from a TOML file, falling back to defaults.
    pub fn load(path: &std::path::Path) -> Result<Self, crate::WolfeError> {
        if path.exists() {
            let contents = std::fs::read_to_string(path)?;
            let config: Config = toml::from_str(&contents)?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    /// Resolve the data directory, creating it if needed.
    pub fn data_dir(&self) -> PathBuf {
        let dir = self.storage.data_dir.clone();
        std::fs::create_dir_all(&dir).ok();
        dir
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// Bitcoin network: "mainnet", "testnet", "testnet4", "signet", "regtest"
    pub chain: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            chain: "mainnet".to_string(),
        }
    }
}

impl NetworkConfig {
    pub fn bitcoin_network(&self) -> bitcoin::Network {
        match self.chain.as_str() {
            "mainnet" | "main" => bitcoin::Network::Bitcoin,
            "testnet" | "testnet3" => bitcoin::Network::Testnet,
            "testnet4" => bitcoin::Network::Testnet,
            "signet" => bitcoin::Network::Signet,
            "regtest" => bitcoin::Network::Regtest,
            _ => bitcoin::Network::Bitcoin,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct P2pConfig {
    /// Listen address for incoming P2P connections.
    pub listen: String,
    /// Maximum number of inbound peer connections.
    pub max_inbound: usize,
    /// Maximum number of outbound peer connections.
    pub max_outbound: usize,
    /// Prefer BIP324 encrypted transport when available.
    pub prefer_v2_transport: bool,
    /// DNS seeds to use for peer discovery (empty = use defaults).
    pub dns_seeds: Vec<String>,
    /// Manually specified peer addresses to connect to.
    pub connect: Vec<String>,
    /// Ban duration in seconds for misbehaving peers.
    pub ban_duration_secs: u64,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            listen: "0.0.0.0:8333".to_string(),
            max_inbound: 125,
            max_outbound: 10,
            prefer_v2_transport: true,
            dns_seeds: vec![],
            connect: vec![],
            ban_duration_secs: 86400,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RpcConfig {
    /// Enable the JSON-RPC server.
    pub enabled: bool,
    /// RPC listen address.
    pub listen: String,
    /// RPC authentication user.
    pub user: Option<String>,
    /// RPC authentication password.
    pub password: Option<String>,
    /// Enable the REST API alongside JSON-RPC.
    pub rest_enabled: bool,
    /// CORS allowed origins for REST API.
    pub cors_origins: Vec<String>,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen: "127.0.0.1:8332".to_string(),
            user: None,
            password: None,
            rest_enabled: true,
            cors_origins: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MempoolConfig {
    /// Maximum mempool size in megabytes.
    pub max_size_mb: usize,
    /// Minimum fee rate (sat/vB) to accept transactions.
    pub min_fee_rate: f64,
    /// Maximum OP_RETURN data size in bytes (0 = disable).
    pub max_datacarrier_bytes: usize,
    /// Accept OP_RETURN outputs.
    pub datacarrier: bool,
    /// Enable full Replace-By-Fee.
    pub full_rbf: bool,
    /// Maximum number of ancestors for a transaction.
    pub max_ancestors: usize,
    /// Maximum number of descendants for a transaction.
    pub max_descendants: usize,
    /// Mempool expiry time in hours.
    pub expiry_hours: u64,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_size_mb: 300,
            min_fee_rate: 1.0,
            max_datacarrier_bytes: 80,
            datacarrier: true,
            full_rbf: true,
            max_ancestors: 25,
            max_descendants: 25,
            expiry_hours: 336,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WalletConfig {
    /// Enable the built-in wallet.
    pub enabled: bool,
    /// Path to wallet database (relative to data_dir).
    pub db_path: String,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            db_path: "wallet.sqlite3".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    /// Root data directory for all node data.
    pub data_dir: PathBuf,
    /// Enable block pruning. 0 = no pruning.
    pub prune_target_mb: u64,
    /// Database cache size in megabytes.
    pub db_cache_mb: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        let default_dir = dirs_or_default();
        Self {
            data_dir: default_dir,
            prune_target_mb: 0,
            db_cache_mb: 450,
        }
    }
}

fn dirs_or_default() -> PathBuf {
    if let Some(home) = dirs_home() {
        home.join(".bitcoinwolfe")
    } else {
        PathBuf::from(".bitcoinwolfe")
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Log level: "trace", "debug", "info", "warn", "error"
    pub level: String,
    /// Output format: "text" or "json"
    pub format: String,
    /// Log to file (in addition to stdout).
    pub file: Option<String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "text".to_string(),
            file: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MetricsConfig {
    /// Enable Prometheus metrics endpoint.
    pub enabled: bool,
    /// Metrics listen address.
    pub listen: String,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen: "127.0.0.1:9332".to_string(),
        }
    }
}
