use redb::{ReadTransaction, ReadableTable, TableDefinition, WriteTransaction};
use tracing::debug;

use crate::error::{StoreError, StoreResult};

// ── Table definition ─────────────────────────────────────────────────────────
//
// META: key (string) -> value (bytes, typically UTF-8 or JSON)
//
// Well-known keys:
//   "sync_height"   -> u32 big-endian (highest fully validated block height)
//   "sync_hash"     -> 32-byte block hash of highest fully validated block
//   "node_id"       -> arbitrary node identity bytes (e.g. 8-byte random nonce)
//   "user_agent"    -> UTF-8 user-agent string
//   "network"       -> UTF-8 network name ("mainnet", "testnet", ...)
//   "db_version"    -> u32 big-endian schema version for forward compatibility

const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

// Well-known key constants exposed to callers.
pub const KEY_SYNC_HEIGHT: &str = "sync_height";
pub const KEY_SYNC_HASH: &str = "sync_hash";
pub const KEY_NODE_ID: &str = "node_id";
pub const KEY_USER_AGENT: &str = "user_agent";
pub const KEY_NETWORK: &str = "network";
pub const KEY_DB_VERSION: &str = "db_version";

/// The current database schema version. Bump this when the on-disk format
/// changes in a backwards-incompatible way.
pub const CURRENT_DB_VERSION: u32 = 1;

/// Sub-store for general node metadata and sync progress tracking.
pub struct MetaStore;

impl MetaStore {
    // ── Generic get / set ────────────────────────────────────────────────

    /// Store an arbitrary key-value pair.
    pub fn set(write_txn: &WriteTransaction, key: &str, value: &[u8]) -> StoreResult<()> {
        let mut table = write_txn.open_table(META)?;
        table.insert(key, value)?;
        debug!(key, "set metadata");
        Ok(())
    }

    /// Retrieve an arbitrary value by key.
    pub fn get(read_txn: &ReadTransaction, key: &str) -> StoreResult<Option<Vec<u8>>> {
        let table = read_txn.open_table(META)?;
        match table.get(key)? {
            Some(guard) => Ok(Some(guard.value().to_vec())),
            None => Ok(None),
        }
    }

    /// Retrieve a value, returning an error if the key does not exist.
    pub fn get_required(read_txn: &ReadTransaction, key: &str) -> StoreResult<Vec<u8>> {
        Self::get(read_txn, key)?.ok_or_else(|| StoreError::MetaKeyNotFound(key.to_string()))
    }

    /// Remove a key-value pair.
    pub fn remove(write_txn: &WriteTransaction, key: &str) -> StoreResult<()> {
        let mut table = write_txn.open_table(META)?;
        table.remove(key)?;
        Ok(())
    }

    // ── Typed helpers: sync progress ─────────────────────────────────────

    /// Record the current sync progress (height + hash of the last fully
    /// validated block).
    pub fn set_sync_progress(
        write_txn: &WriteTransaction,
        height: u32,
        hash: &[u8; 32],
    ) -> StoreResult<()> {
        Self::set(write_txn, KEY_SYNC_HEIGHT, &height.to_be_bytes())?;
        Self::set(write_txn, KEY_SYNC_HASH, hash)?;
        debug!(height, "updated sync progress");
        Ok(())
    }

    /// Retrieve the current sync height, or `None` if the node has never
    /// synced.
    pub fn sync_height(read_txn: &ReadTransaction) -> StoreResult<Option<u32>> {
        match Self::get(read_txn, KEY_SYNC_HEIGHT)? {
            Some(bytes) if bytes.len() == 4 => {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(&bytes);
                Ok(Some(u32::from_be_bytes(buf)))
            }
            _ => Ok(None),
        }
    }

    /// Retrieve the current sync hash, or `None` if never synced.
    pub fn sync_hash(read_txn: &ReadTransaction) -> StoreResult<Option<[u8; 32]>> {
        match Self::get(read_txn, KEY_SYNC_HASH)? {
            Some(bytes) if bytes.len() == 32 => {
                let mut buf = [0u8; 32];
                buf.copy_from_slice(&bytes);
                Ok(Some(buf))
            }
            _ => Ok(None),
        }
    }

    // ── Typed helpers: node identity ─────────────────────────────────────

    /// Store the node identity (a random nonce used in P2P version messages).
    pub fn set_node_id(write_txn: &WriteTransaction, id: &[u8]) -> StoreResult<()> {
        Self::set(write_txn, KEY_NODE_ID, id)
    }

    /// Retrieve the node identity, if previously stored.
    pub fn node_id(read_txn: &ReadTransaction) -> StoreResult<Option<Vec<u8>>> {
        Self::get(read_txn, KEY_NODE_ID)
    }

    /// Store a UTF-8 string under `KEY_USER_AGENT`.
    pub fn set_user_agent(write_txn: &WriteTransaction, ua: &str) -> StoreResult<()> {
        Self::set(write_txn, KEY_USER_AGENT, ua.as_bytes())
    }

    /// Retrieve the stored user-agent string.
    pub fn user_agent(read_txn: &ReadTransaction) -> StoreResult<Option<String>> {
        match Self::get(read_txn, KEY_USER_AGENT)? {
            Some(bytes) => Ok(Some(
                String::from_utf8(bytes)
                    .map_err(|e| StoreError::Corruption(format!("user_agent is not valid UTF-8: {e}")))?,
            )),
            None => Ok(None),
        }
    }

    /// Store the network name (e.g. "mainnet").
    pub fn set_network(write_txn: &WriteTransaction, network: &str) -> StoreResult<()> {
        Self::set(write_txn, KEY_NETWORK, network.as_bytes())
    }

    /// Retrieve the stored network name.
    pub fn network(read_txn: &ReadTransaction) -> StoreResult<Option<String>> {
        match Self::get(read_txn, KEY_NETWORK)? {
            Some(bytes) => Ok(Some(
                String::from_utf8(bytes)
                    .map_err(|e| StoreError::Corruption(format!("network is not valid UTF-8: {e}")))?,
            )),
            None => Ok(None),
        }
    }

    // ── Typed helpers: DB version ────────────────────────────────────────

    /// Write the database schema version.
    pub fn set_db_version(write_txn: &WriteTransaction, version: u32) -> StoreResult<()> {
        Self::set(write_txn, KEY_DB_VERSION, &version.to_be_bytes())
    }

    /// Read the database schema version.
    pub fn db_version(read_txn: &ReadTransaction) -> StoreResult<Option<u32>> {
        match Self::get(read_txn, KEY_DB_VERSION)? {
            Some(bytes) if bytes.len() == 4 => {
                let mut buf = [0u8; 4];
                buf.copy_from_slice(&bytes);
                Ok(Some(u32::from_be_bytes(buf)))
            }
            _ => Ok(None),
        }
    }

    /// Initialise the meta table with default values if it has not been
    /// initialised yet (i.e. `db_version` is absent). Returns `true` if
    /// initialisation was performed.
    pub fn init_if_needed(write_txn: &WriteTransaction) -> StoreResult<bool> {
        // Open the table to ensure it exists, then check for the version key.
        let table = write_txn.open_table(META)?;
        let has_version = table.get(KEY_DB_VERSION)?.is_some();
        drop(table);

        if has_version {
            return Ok(false);
        }

        Self::set_db_version(write_txn, CURRENT_DB_VERSION)?;
        debug!(version = CURRENT_DB_VERSION, "initialised meta store");
        Ok(true)
    }
}
