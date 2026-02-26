use thiserror::Error;

/// Errors specific to the consensus validation layer.
///
/// These errors wrap the underlying `bitcoinkernel` errors and add
/// BitcoinWolfe-specific failure modes related to consensus engine
/// initialization and operation.
#[derive(Error, Debug)]
pub enum ConsensusError {
    /// The consensus engine failed to initialize. This typically indicates
    /// a problem with the data directory, blocks directory, or corrupted
    /// database files.
    #[error("consensus engine initialization failed: {0}")]
    InitializationFailed(String),

    /// A block failed consensus validation. The string contains details
    /// about the specific validation rule that was violated.
    #[error("block validation failed: {0}")]
    BlockValidationFailed(String),

    /// A block header failed consensus validation. The string contains
    /// details about the specific validation rule that was violated.
    #[error("block header validation failed: {0}")]
    HeaderValidationFailed(String),

    /// The requested block was not found in the block tree. This can occur
    /// when querying by hash or by height if the block has not been processed
    /// or the height exceeds the current chain tip.
    #[error("block not found: {0}")]
    BlockNotFound(String),

    /// Failed to read block data from disk. This may indicate pruned blocks,
    /// corrupted block files, or I/O errors.
    #[error("failed to read block data: {0}")]
    BlockReadFailed(String),

    /// Failed to import blocks or complete reindexing. This is typically
    /// returned when `import_blocks` encounters an error during initialization.
    #[error("block import failed: {0}")]
    ImportFailed(String),

    /// The data directory path is invalid, inaccessible, or cannot be created.
    #[error("invalid data directory: {0}")]
    InvalidDataDir(String),

    /// An error was forwarded from the underlying `bitcoinkernel` library.
    #[error("kernel error: {0}")]
    Kernel(#[from] bitcoinkernel::KernelError),

    /// A fatal error was reported by the kernel through its callback interface.
    /// When this occurs, the consensus engine should be considered in an
    /// unrecoverable state and should be dropped.
    #[error("fatal kernel error: {0}")]
    FatalError(String),

    /// A flush error was reported by the kernel, indicating disk I/O problems
    /// during state persistence.
    #[error("flush error: {0}")]
    FlushError(String),

    /// The chain type string from configuration could not be mapped to a
    /// valid `bitcoinkernel::ChainType`.
    #[error("unknown chain type: {0}")]
    UnknownChainType(String),

    /// An I/O error occurred while accessing the filesystem (e.g., creating
    /// directories for the data path).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<ConsensusError> for wolfe_types::WolfeError {
    fn from(err: ConsensusError) -> Self {
        wolfe_types::WolfeError::Consensus(err.to_string())
    }
}
