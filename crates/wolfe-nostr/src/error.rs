use thiserror::Error;

#[derive(Debug, Error)]
pub enum NostrError {
    #[error("relay connection failed: {0}")]
    RelayConnection(String),

    #[error("event signing failed: {0}")]
    Signing(String),

    #[error("event publishing failed: {0}")]
    Publishing(String),

    #[error("invalid key: {0}")]
    InvalidKey(String),

    #[error("NIP-98 auth failed: {0}")]
    Nip98(String),
}
