use thiserror::Error;

#[derive(Error, Debug)]
pub enum P2pError {
    #[error("connection failed to {addr}: {source}")]
    Connection {
        addr: String,
        source: std::io::Error,
    },

    #[error("handshake failed with {addr}: {reason}")]
    Handshake { addr: String, reason: String },

    #[error("peer {addr} sent invalid message: {reason}")]
    InvalidMessage { addr: String, reason: String },

    #[error("peer {addr} misbehaving: {reason} (score: {score})")]
    Misbehavior {
        addr: String,
        reason: String,
        score: u32,
    },

    #[error("message serialization error: {0}")]
    Encode(#[from] bitcoin::consensus::encode::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("peer disconnected")]
    Disconnected,

    #[error("channel closed")]
    ChannelClosed,

    #[error("DNS resolution failed: {0}")]
    DnsResolution(String),
}
