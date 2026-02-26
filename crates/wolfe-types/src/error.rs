use thiserror::Error;

#[derive(Error, Debug)]
pub enum WolfeError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("consensus error: {0}")]
    Consensus(String),

    #[error("P2P network error: {0}")]
    P2p(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("RPC error: {0}")]
    Rpc(String),

    #[error("wallet error: {0}")]
    Wallet(String),

    #[error("mempool error: {0}")]
    Mempool(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("bitcoin encoding error: {0}")]
    BitcoinEncode(#[from] bitcoin::consensus::encode::Error),
}
