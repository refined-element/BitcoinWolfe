//! # wolfe-store
//!
//! Persistent storage layer for the BitcoinWolfe node, built on top of
//! [redb](https://docs.rs/redb) -- a pure-Rust, ACID, embedded key-value
//! store.
//!
//! The main entry point is [`NodeStore`], which owns the underlying
//! `redb::Database` and exposes the three domain-specific sub-stores:
//!
//! * [`HeaderStore`] -- block headers indexed by hash and height.
//! * [`PeerStore`]   -- known / banned peer addresses.
//! * [`MetaStore`]   -- sync progress, node identity, and general KV metadata.
//!
//! Each sub-store is a zero-sized struct whose methods accept a transaction
//! reference (`ReadTransaction` or `WriteTransaction`), letting callers
//! compose multiple operations atomically.

pub mod error;
pub mod headers;
pub mod meta;
pub mod peers;

pub use error::{StoreError, StoreResult};
pub use headers::{HeaderStore, StoredHeader};
pub use meta::MetaStore;
pub use peers::{PeerRecord, PeerStore};

use std::path::Path;

use redb::Database;
use tracing::info;

/// Top-level store that owns the redb database and provides access to
/// domain-specific sub-stores.
pub struct NodeStore {
    db: Database,
}

impl NodeStore {
    /// Open (or create) the database at the given filesystem path.
    ///
    /// On first creation the meta table is initialised with the current schema
    /// version.
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        let path = path.as_ref();
        info!(path = %path.display(), "opening node store");

        let db = Database::create(path)?;

        let store = Self { db };

        // Ensure all tables exist and initialise metadata on first run.
        {
            let write_txn = store.db.begin_write()?;
            MetaStore::init_if_needed(&write_txn)?;
            write_txn.commit()?;
        }

        Ok(store)
    }

    // ── Transaction factories ────────────────────────────────────────────

    /// Begin a read-only transaction.
    pub fn read_txn(&self) -> StoreResult<redb::ReadTransaction> {
        Ok(self.db.begin_read()?)
    }

    /// Begin a read-write transaction.
    pub fn write_txn(&self) -> StoreResult<redb::WriteTransaction> {
        Ok(self.db.begin_write()?)
    }

    // ── Convenience accessors ────────────────────────────────────────────
    //
    // The sub-stores are stateless (zero-sized) so there is nothing to store;
    // these associated functions simply return the unit structs for
    // discoverability and to keep a clear API surface.

    /// Access the header sub-store.
    pub fn headers(&self) -> HeaderStore {
        HeaderStore
    }

    /// Access the peer sub-store.
    pub fn peers(&self) -> PeerStore {
        PeerStore
    }

    /// Access the metadata sub-store.
    pub fn meta(&self) -> MetaStore {
        MetaStore
    }

    // ── High-level helpers ───────────────────────────────────────────────

    /// Store a block header and update sync progress in a single atomic
    /// transaction.
    pub fn insert_header_and_update_sync(
        &self,
        header: &bitcoin::block::Header,
        height: u32,
    ) -> StoreResult<()> {
        let write_txn = self.write_txn()?;
        HeaderStore::insert(&write_txn, header, height)?;
        let hash = header.block_hash();
        MetaStore::set_sync_progress(&write_txn, height, hash.as_ref())?;
        write_txn.commit()?;
        Ok(())
    }

    /// Perform a chain reorganisation: disconnect headers from
    /// `current_height` down to `fork_height` (exclusive), then connect the
    /// new headers starting at `fork_height + 1`.
    ///
    /// Both the disconnect and connect phases happen in a single atomic
    /// transaction. Returns the list of disconnected block hashes.
    pub fn reorganize(
        &self,
        current_height: u32,
        fork_height: u32,
        new_headers: &[(bitcoin::block::Header, u32)],
    ) -> StoreResult<Vec<bitcoin::BlockHash>> {
        let write_txn = self.write_txn()?;

        // Disconnect the stale segment.
        let disconnected =
            HeaderStore::disconnect_range(&write_txn, current_height, fork_height + 1)?;

        // Connect the new segment.
        let mut new_tip_height = fork_height;
        let mut new_tip_hash = [0u8; 32];

        for (header, height) in new_headers {
            HeaderStore::insert(&write_txn, header, *height)?;
            new_tip_height = *height;
            let h = header.block_hash();
            new_tip_hash.copy_from_slice(h.as_ref());
        }

        // Update sync progress to the new tip.
        MetaStore::set_sync_progress(&write_txn, new_tip_height, &new_tip_hash)?;

        write_txn.commit()?;

        info!(
            disconnected = disconnected.len(),
            connected = new_headers.len(),
            new_tip_height,
            "chain reorganisation complete"
        );

        Ok(disconnected)
    }

    /// Return a reference to the underlying redb `Database` for advanced use
    /// cases (e.g. creating savepoints).
    pub fn raw_db(&self) -> &Database {
        &self.db
    }
}
