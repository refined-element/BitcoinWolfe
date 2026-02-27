pub mod config;
pub mod error;

pub use config::Config;
pub use error::WolfeError;

/// Canonical block height type used throughout BitcoinWolfe.
///
/// Internal representation is u64 for maximum range. Conversion notes:
/// - P2P protocol uses i32 (VersionMessage.start_height) — use `height as i32` for outbound
/// - Consensus engine (libbitcoinkernel) uses i32 — use `height as i32`
/// - Store (redb) uses u32 — use `height as u32` (sufficient for ~4 billion blocks)
/// - BDK wallet uses u32 — use `height as u32`
pub type BlockHeight = u64;

/// BitcoinWolfe version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// User agent string for P2P network identification.
/// If a custom user_agent is configured, use that instead.
pub fn user_agent() -> String {
    format!("/BitcoinWolfe:{}/", VERSION)
}

/// User agent string with optional custom override.
pub fn user_agent_or(custom: &str) -> String {
    if custom.is_empty() {
        user_agent()
    } else {
        custom.to_string()
    }
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
