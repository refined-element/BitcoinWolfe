use bitcoin::block::Header;
use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use redb::{
    ReadTransaction, ReadableTable, ReadableTableMetadata, TableDefinition, WriteTransaction,
};
use tracing::{debug, warn};

use crate::error::{StoreError, StoreResult};

// ── Table definitions ────────────────────────────────────────────────────────
//
// HEADERS: block_hash (32 bytes) -> serialized Header (80 bytes)
// HEIGHT_TO_HASH: height (u32 big-endian, 4 bytes) -> block_hash (32 bytes)
// HASH_TO_HEIGHT: block_hash (32 bytes) -> height (u32 big-endian, 4 bytes)

const HEADERS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("headers");
const HEIGHT_TO_HASH: TableDefinition<&[u8], &[u8]> = TableDefinition::new("height_to_hash");
const HASH_TO_HEIGHT: TableDefinition<&[u8], &[u8]> = TableDefinition::new("hash_to_height");

/// A stored block header together with its chain height.
#[derive(Debug, Clone)]
pub struct StoredHeader {
    pub header: Header,
    pub height: u32,
    pub hash: BlockHash,
}

/// Sub-store responsible for block header persistence and best-chain tracking.
///
/// All public methods accept either a `ReadTransaction` or `WriteTransaction`
/// so that callers (the top-level `NodeStore`) can compose operations within a
/// single transaction when needed.
pub struct HeaderStore;

impl HeaderStore {
    // ── Write helpers ────────────────────────────────────────────────────

    /// Insert a block header at the given height.
    ///
    /// This updates all three index tables atomically (must be called inside a
    /// write transaction that the caller commits).
    pub fn insert(write_txn: &WriteTransaction, header: &Header, height: u32) -> StoreResult<()> {
        let hash = header.block_hash();
        let hash_bytes: &[u8] = hash.as_ref();
        let header_bytes = serialize(header);
        let height_bytes = height.to_be_bytes();

        {
            let mut table = write_txn.open_table(HEADERS)?;
            table.insert(hash_bytes, header_bytes.as_slice())?;
        }
        {
            let mut table = write_txn.open_table(HEIGHT_TO_HASH)?;
            table.insert(height_bytes.as_slice(), hash_bytes)?;
        }
        {
            let mut table = write_txn.open_table(HASH_TO_HEIGHT)?;
            table.insert(hash_bytes, height_bytes.as_slice())?;
        }

        debug!(
            %hash,
            height,
            "stored block header"
        );

        Ok(())
    }

    /// Remove a header by its hash. Used during chain reorganisation to roll
    /// back blocks that are no longer on the best chain.
    pub fn remove(write_txn: &WriteTransaction, hash: &BlockHash) -> StoreResult<()> {
        let hash_bytes: &[u8] = hash.as_ref();

        // Look up the height first so we can clean up both index tables.
        let height_bytes = {
            let table = write_txn.open_table(HASH_TO_HEIGHT)?;
            let result = match table.get(hash_bytes)? {
                Some(guard) => {
                    let val = guard.value();
                    let mut buf = [0u8; 4];
                    buf.copy_from_slice(val);
                    Some(buf)
                }
                None => None,
            };
            result
        };

        {
            let mut table = write_txn.open_table(HEADERS)?;
            table.remove(hash_bytes)?;
        }
        {
            let mut table = write_txn.open_table(HASH_TO_HEIGHT)?;
            table.remove(hash_bytes)?;
        }

        if let Some(hb) = height_bytes {
            let mut table = write_txn.open_table(HEIGHT_TO_HASH)?;
            table.remove(hb.as_slice())?;
        }

        debug!(%hash, "removed block header");
        Ok(())
    }

    /// Disconnect blocks from `from_height` (inclusive) down to `to_height`
    /// (exclusive). Returns the hashes that were removed, most-recent first.
    ///
    /// This is the core primitive behind reorg handling: the caller first
    /// disconnects the stale chain segment and then connects the new one.
    pub fn disconnect_range(
        write_txn: &WriteTransaction,
        from_height: u32,
        to_height: u32,
    ) -> StoreResult<Vec<BlockHash>> {
        let mut removed = Vec::new();

        for h in (to_height..=from_height).rev() {
            if let Some(stored) = Self::get_by_height_write(write_txn, h)? {
                Self::remove(write_txn, &stored.hash)?;
                removed.push(stored.hash);
            } else {
                warn!(
                    height = h,
                    "expected header at height during disconnect, but not found"
                );
            }
        }

        Ok(removed)
    }

    // ── Read helpers (ReadTransaction) ───────────────────────────────────

    /// Retrieve a header by its block hash.
    pub fn get_by_hash(
        read_txn: &ReadTransaction,
        hash: &BlockHash,
    ) -> StoreResult<Option<StoredHeader>> {
        let hash_bytes: &[u8] = hash.as_ref();

        let table = read_txn.open_table(HEADERS)?;
        let header_guard = match table.get(hash_bytes)? {
            Some(g) => g,
            None => return Ok(None),
        };
        let header: Header = deserialize(header_guard.value())?;
        drop(header_guard);

        let height = {
            let ht_table = read_txn.open_table(HASH_TO_HEIGHT)?;
            match ht_table.get(hash_bytes)? {
                Some(g) => {
                    let mut buf = [0u8; 4];
                    buf.copy_from_slice(g.value());
                    u32::from_be_bytes(buf)
                }
                None => {
                    return Err(StoreError::Corruption(format!(
                        "header {} exists but height index missing",
                        hash
                    )));
                }
            }
        };

        Ok(Some(StoredHeader {
            header,
            height,
            hash: *hash,
        }))
    }

    /// Retrieve a header by chain height.
    pub fn get_by_height(
        read_txn: &ReadTransaction,
        height: u32,
    ) -> StoreResult<Option<StoredHeader>> {
        let height_bytes = height.to_be_bytes();

        let table = read_txn.open_table(HEIGHT_TO_HASH)?;
        let hash_guard = match table.get(height_bytes.as_slice())? {
            Some(g) => g,
            None => return Ok(None),
        };

        let mut hash_arr = [0u8; 32];
        hash_arr.copy_from_slice(hash_guard.value());
        drop(hash_guard);

        let hash =
            BlockHash::from_raw_hash(bitcoin::hashes::sha256d::Hash::from_byte_array(hash_arr));
        Self::get_by_hash(read_txn, &hash)
    }

    /// Return the best (highest) stored header, if any.
    ///
    /// This scans the height-to-hash table in reverse to find the maximum
    /// height, which is efficient because redb stores keys in sorted order
    /// and the table is relatively small (one entry per block).
    pub fn tip(read_txn: &ReadTransaction) -> StoreResult<Option<StoredHeader>> {
        let table = read_txn.open_table(HEIGHT_TO_HASH)?;

        // `iter()` returns entries in sorted key order; we reverse to get the
        // highest height first. Because keys are big-endian u32, lexicographic
        // order matches numeric order.
        let mut iter = table.iter()?;
        let last = iter.next_back();

        match last {
            Some(Ok((key_guard, val_guard))) => {
                let mut height_buf = [0u8; 4];
                height_buf.copy_from_slice(key_guard.value());
                let _height = u32::from_be_bytes(height_buf);

                let mut hash_arr = [0u8; 32];
                hash_arr.copy_from_slice(val_guard.value());
                drop(key_guard);
                drop(val_guard);

                let hash = BlockHash::from_raw_hash(
                    bitcoin::hashes::sha256d::Hash::from_byte_array(hash_arr),
                );
                Self::get_by_hash(read_txn, &hash)
            }
            Some(Err(e)) => Err(StoreError::Storage(e)),
            None => Ok(None),
        }
    }

    /// Count the total number of stored headers.
    pub fn count(read_txn: &ReadTransaction) -> StoreResult<u64> {
        let table = read_txn.open_table(HEADERS)?;
        Ok(table.len()?)
    }

    // ── Read helpers (WriteTransaction) ──────────────────────────────────
    //
    // During a write transaction we sometimes need to read data (e.g. to
    // resolve the hash at a given height before removing it). These helpers
    // duplicate the read logic but operate on a WriteTransaction, which also
    // supports `open_table` in read mode.

    fn get_by_height_write(
        write_txn: &WriteTransaction,
        height: u32,
    ) -> StoreResult<Option<StoredHeader>> {
        let height_bytes = height.to_be_bytes();

        let table = write_txn.open_table(HEIGHT_TO_HASH)?;
        let hash_guard = match table.get(height_bytes.as_slice())? {
            Some(g) => g,
            None => return Ok(None),
        };

        let mut hash_arr = [0u8; 32];
        hash_arr.copy_from_slice(hash_guard.value());
        drop(hash_guard);

        let hash =
            BlockHash::from_raw_hash(bitcoin::hashes::sha256d::Hash::from_byte_array(hash_arr));

        // Read header bytes
        let headers_table = write_txn.open_table(HEADERS)?;
        let header_guard = match headers_table.get(hash_arr.as_slice())? {
            Some(g) => g,
            None => {
                return Err(StoreError::Corruption(format!(
                    "height {} maps to hash {} but header not found",
                    height, hash
                )));
            }
        };
        let header: Header = deserialize(header_guard.value())?;

        Ok(Some(StoredHeader {
            header,
            height,
            hash,
        }))
    }
}
