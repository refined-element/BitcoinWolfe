use redb::{
    ReadTransaction, ReadableTable, ReadableTableMetadata, TableDefinition, WriteTransaction,
};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::error::{StoreError, StoreResult};

// ── Table definitions ────────────────────────────────────────────────────────
//
// PEERS: addr_key (string, e.g. "127.0.0.1:8333") -> JSON-encoded PeerRecord
// BANNED_PEERS: addr_key -> ban-expiry unix timestamp (8 bytes, big-endian u64)

const PEERS: TableDefinition<&str, &[u8]> = TableDefinition::new("peers");
const BANNED_PEERS: TableDefinition<&str, &[u8]> = TableDefinition::new("banned_peers");

/// Metadata stored alongside a known peer address.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRecord {
    /// The socket address string (e.g. "127.0.0.1:8333").
    pub addr: String,
    /// Services bitfield advertised by the peer.
    pub services: u64,
    /// Unix timestamp of when we last successfully connected / received a
    /// message from this peer.
    pub last_seen: u64,
    /// Unix timestamp of when this address was first learned.
    pub first_seen: u64,
    /// How many times we have successfully connected to this peer.
    pub connection_count: u64,
    /// How many consecutive connection failures we have observed.
    pub fail_count: u32,
    /// User-agent string (if known).
    pub user_agent: String,
}

/// Sub-store for peer address management.
///
/// All methods accept a transaction reference so callers can compose multiple
/// operations atomically.
pub struct PeerStore;

impl PeerStore {
    // ── Write helpers ────────────────────────────────────────────────────

    /// Upsert a peer record. If the peer already exists, the record is
    /// overwritten (the caller is expected to merge fields as needed before
    /// calling this).
    pub fn upsert(write_txn: &WriteTransaction, record: &PeerRecord) -> StoreResult<()> {
        let json = serde_json::to_vec(record)?;
        let mut table = write_txn.open_table(PEERS)?;
        table.insert(record.addr.as_str(), json.as_slice())?;
        debug!(addr = %record.addr, "upserted peer record");
        Ok(())
    }

    /// Remove a peer by address.
    pub fn remove(write_txn: &WriteTransaction, addr: &str) -> StoreResult<()> {
        let mut table = write_txn.open_table(PEERS)?;
        table.remove(addr)?;
        debug!(addr, "removed peer record");
        Ok(())
    }

    /// Ban a peer until `until_unix` (seconds since epoch).
    pub fn ban(write_txn: &WriteTransaction, addr: &str, until_unix: u64) -> StoreResult<()> {
        let ts_bytes = until_unix.to_be_bytes();
        let mut table = write_txn.open_table(BANNED_PEERS)?;
        table.insert(addr, ts_bytes.as_slice())?;
        debug!(addr, until_unix, "banned peer");
        Ok(())
    }

    /// Remove a ban for the given address.
    pub fn unban(write_txn: &WriteTransaction, addr: &str) -> StoreResult<()> {
        let mut table = write_txn.open_table(BANNED_PEERS)?;
        table.remove(addr)?;
        debug!(addr, "unbanned peer");
        Ok(())
    }

    /// Purge all bans that have expired relative to `now_unix`.
    pub fn purge_expired_bans(write_txn: &WriteTransaction, now_unix: u64) -> StoreResult<u64> {
        let expired: Vec<String> = {
            let table = write_txn.open_table(BANNED_PEERS)?;
            let mut expired = Vec::new();
            let iter = table.iter()?;
            for entry in iter {
                let (key_guard, val_guard) = entry.map_err(StoreError::Storage)?;
                let addr = key_guard.value().to_owned();
                let mut buf = [0u8; 8];
                buf.copy_from_slice(val_guard.value());
                let until = u64::from_be_bytes(buf);
                if until <= now_unix {
                    expired.push(addr);
                }
            }
            expired
        };

        let count = expired.len() as u64;
        let mut table = write_txn.open_table(BANNED_PEERS)?;
        for addr in &expired {
            table.remove(addr.as_str())?;
        }

        if count > 0 {
            debug!(count, "purged expired peer bans");
        }
        Ok(count)
    }

    // ── Read helpers ─────────────────────────────────────────────────────

    /// Retrieve a peer record by address.
    pub fn get(read_txn: &ReadTransaction, addr: &str) -> StoreResult<Option<PeerRecord>> {
        let table = read_txn.open_table(PEERS)?;
        match table.get(addr)? {
            Some(guard) => {
                let record: PeerRecord = serde_json::from_slice(guard.value())?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }

    /// Return all known (non-banned) peers, ordered by `last_seen` descending
    /// (most recently seen first).
    pub fn list_all(read_txn: &ReadTransaction) -> StoreResult<Vec<PeerRecord>> {
        let table = read_txn.open_table(PEERS)?;
        let mut records = Vec::new();
        let iter = table.iter()?;
        for entry in iter {
            let (_key_guard, val_guard) = entry.map_err(StoreError::Storage)?;
            let record: PeerRecord = serde_json::from_slice(val_guard.value())?;
            records.push(record);
        }
        records.sort_by_key(|r| std::cmp::Reverse(r.last_seen));
        Ok(records)
    }

    /// Check whether `addr` is currently banned. Returns the ban expiry
    /// timestamp if banned and the ban has not yet expired relative to
    /// `now_unix`.
    pub fn is_banned(
        read_txn: &ReadTransaction,
        addr: &str,
        now_unix: u64,
    ) -> StoreResult<Option<u64>> {
        let table = read_txn.open_table(BANNED_PEERS)?;
        match table.get(addr)? {
            Some(guard) => {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(guard.value());
                let until = u64::from_be_bytes(buf);
                if until > now_unix {
                    Ok(Some(until))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    /// Get up to `count` random peers suitable for connection. This filters
    /// out banned peers (relative to `now_unix`) and returns a shuffled
    /// selection.
    pub fn get_random(
        read_txn: &ReadTransaction,
        count: usize,
        now_unix: u64,
    ) -> StoreResult<Vec<PeerRecord>> {
        let all = Self::list_all(read_txn)?;

        // Collect addresses that are currently banned so we can skip them.
        let banned_table = read_txn.open_table(BANNED_PEERS)?;
        let mut candidates: Vec<PeerRecord> = Vec::new();
        for record in all {
            let is_banned = match banned_table.get(record.addr.as_str())? {
                Some(guard) => {
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(guard.value());
                    u64::from_be_bytes(buf) > now_unix
                }
                None => false,
            };
            if !is_banned {
                candidates.push(record);
            }
        }

        // Simple deterministic-ish shuffle: sort by a cheap hash of the
        // address XOR'd with the current timestamp, then truncate. This is not
        // cryptographically random, but sufficient for peer selection.
        candidates.sort_by(|a, b| {
            let ha = simple_hash(a.addr.as_bytes(), now_unix);
            let hb = simple_hash(b.addr.as_bytes(), now_unix);
            ha.cmp(&hb)
        });

        candidates.truncate(count);
        Ok(candidates)
    }

    /// Count total known peers.
    pub fn count(read_txn: &ReadTransaction) -> StoreResult<u64> {
        let table = read_txn.open_table(PEERS)?;
        Ok(table.len()?)
    }

    /// Count currently banned peers (including expired bans still in storage).
    pub fn banned_count(read_txn: &ReadTransaction) -> StoreResult<u64> {
        let table = read_txn.open_table(BANNED_PEERS)?;
        Ok(table.len()?)
    }
}

/// Cheap non-cryptographic hash for shuffling. Produces a u64 from a byte
/// slice and a salt.
fn simple_hash(data: &[u8], salt: u64) -> u64 {
    let mut h: u64 = salt;
    for &b in data {
        h = h.wrapping_mul(6364136223846793005).wrapping_add(b as u64);
    }
    h
}
