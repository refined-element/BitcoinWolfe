use thiserror::Error;

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("wallet not enabled in configuration")]
    Disabled,

    #[error("BDK wallet error: {0}")]
    Bdk(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("invalid address: {0}")]
    InvalidAddress(String),

    #[error("insufficient funds: need {needed} sat, have {available} sat")]
    InsufficientFunds { needed: u64, available: u64 },

    #[error("signing failed: {0}")]
    Signing(String),

    #[error("no wallet loaded")]
    NotLoaded,
}
