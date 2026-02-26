pub mod config;
pub mod error;

pub use config::Config;
pub use error::WolfeError;

/// BitcoinWolfe version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// User agent string for P2P network identification.
pub fn user_agent() -> String {
    format!("/BitcoinWolfe:{}/", VERSION)
}

/// Serializable snapshot of peer info for RPC/API consumption.
/// This lives here so wolfe-rpc doesn't need to depend on wolfe-p2p.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerInfoSnapshot {
    pub addr: std::net::SocketAddr,
    pub user_agent: String,
    pub version: u32,
    pub inbound: bool,
    pub v2_transport: bool,
    pub start_height: i32,
}
