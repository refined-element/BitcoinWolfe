use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level configuration for a BitcoinWolfe node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    pub nostr: NostrConfig,
    pub lightning: LightningConfig,
    pub l402: L402Config,
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
    pub fn bitcoin_network(&self) -> Result<bitcoin::Network, crate::WolfeError> {
        match self.chain.as_str() {
            "mainnet" | "main" => Ok(bitcoin::Network::Bitcoin),
            "testnet" | "testnet3" => Ok(bitcoin::Network::Testnet),
            "testnet4" => Ok(bitcoin::Network::Testnet),
            "signet" => Ok(bitcoin::Network::Signet),
            "regtest" => Ok(bitcoin::Network::Regtest),
            other => Err(crate::WolfeError::Config(format!(
                "unknown network '{}'. Valid options: mainnet, testnet, testnet4, signet, regtest",
                other
            ))),
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
    /// Custom user-agent string (empty = use default).
    pub user_agent: String,
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
            user_agent: String::new(),
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
    /// External (receiving) descriptor (e.g., "wpkh(tprv.../84'/1'/0'/0/*)").
    /// If empty, the wallet will generate a new random descriptor on first run.
    pub external_descriptor: String,
    /// Internal (change) descriptor.
    pub internal_descriptor: String,
    /// Encryption passphrase for the wallet database.
    /// When set, the wallet database is encrypted at rest using SQLCipher.
    /// Requires the node to be built with the `sqlcipher` feature.
    /// WARNING: If you lose this passphrase, wallet data is irrecoverable.
    pub encryption_key: Option<String>,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            db_path: "wallet.sqlite3".to_string(),
            external_descriptor: String::new(),
            internal_descriptor: String::new(),
            encryption_key: None,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NostrConfig {
    /// Enable Nostr integration (block announcements, fee oracle, NIP-98 auth).
    pub enabled: bool,
    /// Nostr secret key (hex or nsec). If empty, an ephemeral key is generated.
    pub secret_key: Option<String>,
    /// Profile display name (NIP-01 kind 0 metadata).
    pub name: Option<String>,
    /// Profile about/bio text.
    pub about: Option<String>,
    /// Profile picture URL (NIP-01 kind 0 metadata).
    pub picture: Option<String>,
    /// Relay URLs to publish events to.
    pub relays: Vec<String>,
    /// Publish new block announcements to relays.
    pub block_announcements: bool,
    /// Publish mempool fee oracle events to relays.
    pub fee_oracle: bool,
    /// Fee oracle publishing interval in seconds.
    pub fee_oracle_interval_secs: u64,
    /// Enable NIP-98 HTTP Auth for RPC (alternative to Basic auth).
    pub nip98_auth: bool,
    /// Nostr public keys (hex or npub) allowed to authenticate via NIP-98.
    /// If empty, any valid NIP-98 event is accepted.
    pub allowed_pubkeys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LightningConfig {
    /// Enable the embedded Lightning node.
    pub enabled: bool,
    /// TCP port for Lightning P2P connections.
    pub listen_port: u16,
    /// Node alias visible to the network.
    pub alias: String,
    /// Node color in hex (RGB).
    pub color: String,
    /// Addresses to announce for inbound connections.
    pub announced_listen_addrs: Vec<String>,
    /// Accept inbound channel open requests.
    pub accept_inbound_channels: bool,
    /// Minimum channel size in satoshis.
    pub min_channel_size_sat: u64,
    /// Maximum channel size in satoshis (wumbo = 16,777,215).
    pub max_channel_size_sat: u64,
    /// URL for Rapid Gossip Sync server (optional, speeds up initial gossip).
    pub rapid_gossip_sync_url: Option<String>,
    /// Peers to keep connected to, periodically reconnecting as needed (pubkey@host:port).
    pub persistent_peers: Vec<String>,
}

impl Default for LightningConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_port: 9735,
            alias: "BitcoinWolfe".to_string(),
            color: "ff9900".to_string(),
            announced_listen_addrs: vec![],
            accept_inbound_channels: true,
            min_channel_size_sat: 20_000,
            max_channel_size_sat: 16_777_215,
            rapid_gossip_sync_url: None,
            persistent_peers: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct L402Config {
    /// Enable L402 Lightning-gated API endpoints.
    pub enabled: bool,
    /// Price per API request in satoshis.
    pub price_sats: u64,
    /// How long a paid token remains valid (seconds).
    pub token_expiry_secs: u64,
    /// Invoice expiry time (seconds).
    pub invoice_expiry_secs: u32,
    /// Invoice description shown to payers.
    pub invoice_description: String,
}

impl Default for L402Config {
    fn default() -> Self {
        Self {
            enabled: false,
            price_sats: 10,
            token_expiry_secs: 3600,
            invoice_expiry_secs: 600,
            invoice_description: "BitcoinWolfe API access".to_string(),
        }
    }
}

impl Default for NostrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            secret_key: None,
            name: None,
            about: None,
            picture: None,
            relays: vec![
                "wss://relay.damus.io".to_string(),
                "wss://nos.lol".to_string(),
            ],
            block_announcements: true,
            fee_oracle: true,
            fee_oracle_interval_secs: 60,
            nip98_auth: false,
            allowed_pubkeys: vec![],
        }
    }
}
