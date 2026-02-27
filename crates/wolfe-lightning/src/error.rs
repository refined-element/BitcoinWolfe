use thiserror::Error;

#[derive(Debug, Error)]
pub enum LightningError {
    #[error("persistence error: {0}")]
    Persistence(String),

    #[error("channel error: {0}")]
    Channel(String),

    #[error("payment error: {0}")]
    Payment(String),

    #[error("peer connection error: {0}")]
    PeerConnection(String),

    #[error("key management error: {0}")]
    KeyManagement(String),

    #[error("invoice error: {0}")]
    Invoice(String),

    #[error("not initialized")]
    NotInitialized,

    #[error("store error: {0}")]
    Store(#[from] wolfe_store::StoreError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
