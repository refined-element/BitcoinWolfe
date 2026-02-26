use thiserror::Error;

#[derive(Error, Debug)]
pub enum MempoolError {
    #[error("transaction rejected: {reason}")]
    Rejected { reason: String },

    #[error("fee too low: {fee_rate:.1} sat/vB < minimum {min_fee_rate:.1} sat/vB")]
    FeeTooLow { fee_rate: f64, min_fee_rate: f64 },

    #[error("transaction too large: {size} bytes")]
    TooLarge { size: usize },

    #[error("mempool full ({size_mb:.1} MB >= {max_mb} MB)")]
    Full { size_mb: f64, max_mb: usize },

    #[error("OP_RETURN output exceeds limit: {size} > {max} bytes")]
    DatacarrierTooLarge { size: usize, max: usize },

    #[error("OP_RETURN rejected by policy")]
    DatacarrierDisabled,

    #[error("too many ancestors: {count} > {max}")]
    TooManyAncestors { count: usize, max: usize },

    #[error("too many descendants: {count} > {max}")]
    TooManyDescendants { count: usize, max: usize },

    #[error("duplicate transaction: {0}")]
    Duplicate(bitcoin::Txid),

    #[error("missing inputs")]
    MissingInputs,
}
