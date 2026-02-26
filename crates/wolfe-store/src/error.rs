use thiserror::Error;

/// Errors that can occur in the storage layer.
#[derive(Error, Debug)]
pub enum StoreError {
    #[error("redb database error: {0}")]
    Database(#[from] redb::DatabaseError),

    #[error("redb storage error: {0}")]
    Storage(#[from] redb::StorageError),

    #[error("redb table error: {0}")]
    Table(#[from] redb::TableError),

    #[error("redb transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),

    #[error("redb commit error: {0}")]
    Commit(#[from] redb::CommitError),

    #[error("bitcoin consensus encoding error: {0}")]
    BitcoinEncode(#[from] bitcoin::consensus::encode::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("header not found for hash: {0}")]
    HeaderNotFound(String),

    #[error("header not found at height: {0}")]
    HeaderNotFoundAtHeight(u32),

    #[error("peer not found: {0}")]
    PeerNotFound(String),

    #[error("metadata key not found: {0}")]
    MetaKeyNotFound(String),

    #[error("data corruption: {0}")]
    Corruption(String),
}

/// Convenience Result type for the store crate.
pub type StoreResult<T> = Result<T, StoreError>;
