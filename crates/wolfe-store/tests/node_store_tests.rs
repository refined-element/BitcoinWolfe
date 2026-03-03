//! Comprehensive tests for the top-level `NodeStore` and its high-level
//! helper methods: `insert_header_and_update_sync`, `insert_headers_batch`,
//! and `reorganize`.

use bitcoin::block::Header;
use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use tempfile::TempDir;
use wolfe_store::meta::MetaStore;
use wolfe_store::{HeaderStore, NodeStore};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_store() -> (NodeStore, TempDir) {
    let dir = TempDir::new().expect("create temp dir");
    let db_path = dir.path().join("test.redb");
    let store = NodeStore::open(&db_path).expect("open store");
    (store, dir)
}

fn make_header(height: u32, prev_hash: BlockHash) -> Header {
    Header {
        version: bitcoin::block::Version::from_consensus(0x2000_0000),
        prev_blockhash: prev_hash,
        merkle_root: bitcoin::TxMerkleNode::from_raw_hash(
            bitcoin::hashes::sha256d::Hash::from_byte_array([0xab; 32]),
        ),
        time: 1_700_000_000 + height,
        bits: bitcoin::CompactTarget::from_consensus(0x1d00ffff),
        nonce: height,
    }
}

fn make_chain(start_height: u32, count: u32, prev_hash: BlockHash) -> Vec<(Header, u32)> {
    let mut chain = Vec::with_capacity(count as usize);
    let mut prev = prev_hash;
    for i in 0..count {
        let h = start_height + i;
        let header = make_header(h, prev);
        prev = header.block_hash();
        chain.push((header, h));
    }
    chain
}

fn zero_hash() -> BlockHash {
    BlockHash::from_raw_hash(bitcoin::hashes::sha256d::Hash::from_byte_array([0u8; 32]))
}

// ---------------------------------------------------------------------------
// NodeStore::open
// ---------------------------------------------------------------------------

#[test]
fn open_creates_database_file() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("brand_new.redb");
    assert!(!db_path.exists());

    let _store = NodeStore::open(&db_path).unwrap();
    assert!(db_path.exists());
}

#[test]
fn open_is_reentrant() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.redb");

    // First open
    {
        let store = NodeStore::open(&db_path).unwrap();
        let wtx = store.write_txn().unwrap();
        MetaStore::set_network(&wtx, "mainnet").unwrap();
        wtx.commit().unwrap();
    }

    // Second open: should not lose data
    {
        let store = NodeStore::open(&db_path).unwrap();
        let rtx = store.read_txn().unwrap();
        let net = MetaStore::network(&rtx).unwrap().unwrap();
        assert_eq!(net, "mainnet");
    }
}

#[test]
fn open_initialises_meta_on_first_run() {
    let (store, _dir) = temp_store();

    let rtx = store.read_txn().unwrap();
    let version = MetaStore::db_version(&rtx).unwrap();
    assert!(
        version.is_some(),
        "db_version should be set after opening a new store"
    );
    assert_eq!(version.unwrap(), wolfe_store::meta::CURRENT_DB_VERSION);
}

// ---------------------------------------------------------------------------
// insert_header_and_update_sync
// ---------------------------------------------------------------------------

#[test]
fn insert_header_and_update_sync_stores_header_and_progress() {
    let (store, _dir) = temp_store();
    let header = make_header(0, zero_hash());
    let hash = header.block_hash();

    store.insert_header_and_update_sync(&header, 0).unwrap();

    let rtx = store.read_txn().unwrap();

    // Header should be stored
    let stored = HeaderStore::get_by_hash(&rtx, &hash)
        .unwrap()
        .expect("header should exist");
    assert_eq!(stored.height, 0);
    assert_eq!(stored.header, header);

    // Sync progress should be updated
    let sync_h = MetaStore::sync_height(&rtx).unwrap().unwrap();
    assert_eq!(sync_h, 0);

    let sync_hash = MetaStore::sync_hash(&rtx).unwrap().unwrap();
    assert_eq!(&sync_hash[..], AsRef::<[u8]>::as_ref(&hash));
}

#[test]
fn insert_header_and_update_sync_sequential() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 5, zero_hash());

    for (header, height) in &chain {
        store
            .insert_header_and_update_sync(header, *height)
            .unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 5);

    // Sync progress should be at the last inserted header
    let sync_h = MetaStore::sync_height(&rtx).unwrap().unwrap();
    assert_eq!(sync_h, 4);
}

// ---------------------------------------------------------------------------
// insert_headers_batch
// ---------------------------------------------------------------------------

#[test]
fn insert_headers_batch_empty_is_noop() {
    let (store, _dir) = temp_store();

    // Should not error
    store.insert_headers_batch(&[]).unwrap();

    // No headers should have been stored (we need tables to exist to query)
    // We verify by checking that no sync progress was set
    let rtx = store.read_txn().unwrap();
    let sync_h = MetaStore::sync_height(&rtx).unwrap();
    assert!(sync_h.is_none());
}

#[test]
fn insert_headers_batch_stores_all_headers() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 20, zero_hash());

    store.insert_headers_batch(&chain).unwrap();

    let rtx = store.read_txn().unwrap();
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 20);

    // Every header should be retrievable by height
    for (header, height) in &chain {
        let stored = HeaderStore::get_by_height(&rtx, *height)
            .unwrap()
            .unwrap_or_else(|| panic!("header at height {} should exist", height));
        assert_eq!(stored.header, *header);
    }
}

#[test]
fn insert_headers_batch_updates_sync_to_last() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 10, zero_hash());

    store.insert_headers_batch(&chain).unwrap();

    let rtx = store.read_txn().unwrap();
    let sync_h = MetaStore::sync_height(&rtx).unwrap().unwrap();
    assert_eq!(sync_h, 9);

    let sync_hash = MetaStore::sync_hash(&rtx).unwrap().unwrap();
    let expected_hash = chain.last().unwrap().0.block_hash();
    assert_eq!(&sync_hash[..], AsRef::<[u8]>::as_ref(&expected_hash));
}

#[test]
fn insert_headers_batch_single_element() {
    let (store, _dir) = temp_store();
    let header = make_header(0, zero_hash());

    store.insert_headers_batch(&[(header, 0)]).unwrap();

    let rtx = store.read_txn().unwrap();
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 1);
    let sync_h = MetaStore::sync_height(&rtx).unwrap().unwrap();
    assert_eq!(sync_h, 0);
}

#[test]
fn insert_headers_batch_multiple_batches() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 30, zero_hash());

    // Insert in three batches of 10
    store.insert_headers_batch(&chain[0..10]).unwrap();
    store.insert_headers_batch(&chain[10..20]).unwrap();
    store.insert_headers_batch(&chain[20..30]).unwrap();

    let rtx = store.read_txn().unwrap();
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 30);

    let tip = HeaderStore::tip(&rtx).unwrap().unwrap();
    assert_eq!(tip.height, 29);

    let sync_h = MetaStore::sync_height(&rtx).unwrap().unwrap();
    assert_eq!(sync_h, 29);
}

#[test]
fn insert_headers_batch_is_atomic() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 5, zero_hash());

    store.insert_headers_batch(&chain).unwrap();

    // All 5 should be present -- atomicity means either all or none
    let rtx = store.read_txn().unwrap();
    let count = HeaderStore::count(&rtx).unwrap();
    assert_eq!(count, 5);
}

// ---------------------------------------------------------------------------
// reorganize
// ---------------------------------------------------------------------------

#[test]
fn reorganize_disconnects_old_and_connects_new() {
    let (store, _dir) = temp_store();

    // Build initial chain: heights 0..9
    let chain = make_chain(0, 10, zero_hash());
    store.insert_headers_batch(&chain).unwrap();

    // Fork at height 6: new chain replaces heights 7, 8, 9
    // New headers at heights 7, 8, 9 but with different content
    let fork_prev = chain[6].0.block_hash();
    let new_chain: Vec<(Header, u32)> = (7..=9)
        .map(|h| {
            let header = Header {
                nonce: h + 1_000_000, // Different nonce -> different hash
                ..make_header(h, if h == 7 { fork_prev } else { zero_hash() })
            };
            (header, h)
        })
        .collect();

    let disconnected = store.reorganize(9, 6, &new_chain).unwrap();

    // Should have disconnected 3 headers (heights 9, 8, 7)
    assert_eq!(disconnected.len(), 3);
    assert_eq!(disconnected[0], chain[9].0.block_hash());
    assert_eq!(disconnected[1], chain[8].0.block_hash());
    assert_eq!(disconnected[2], chain[7].0.block_hash());

    let rtx = store.read_txn().unwrap();

    // Heights 0..6 should still have the original headers
    for h in 0..=6 {
        let stored = HeaderStore::get_by_height(&rtx, h).unwrap().unwrap();
        assert_eq!(stored.header, chain[h as usize].0);
    }

    // Heights 7..9 should have the new headers
    for (new_header, height) in &new_chain {
        let stored = HeaderStore::get_by_height(&rtx, *height).unwrap().unwrap();
        assert_eq!(stored.header, *new_header);
        assert_eq!(stored.hash, new_header.block_hash());
    }

    // Old hashes should no longer be retrievable
    for h in 7..=9 {
        let old_hash = chain[h as usize].0.block_hash();
        let result = HeaderStore::get_by_hash(&rtx, &old_hash).unwrap();
        assert!(
            result.is_none(),
            "old header at height {} should be gone",
            h
        );
    }

    // Count should still be 10
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 10);
}

#[test]
fn reorganize_updates_sync_progress_to_new_tip() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 10, zero_hash());
    store.insert_headers_batch(&chain).unwrap();

    // Fork at height 5: new chain replaces heights 6..9
    let fork_prev = chain[5].0.block_hash();
    let new_chain: Vec<(Header, u32)> = (6..=9)
        .map(|h| {
            let header = Header {
                nonce: h + 2_000_000,
                ..make_header(h, if h == 6 { fork_prev } else { zero_hash() })
            };
            (header, h)
        })
        .collect();

    store.reorganize(9, 5, &new_chain).unwrap();

    let rtx = store.read_txn().unwrap();
    let sync_h = MetaStore::sync_height(&rtx).unwrap().unwrap();
    assert_eq!(sync_h, 9);

    let sync_hash = MetaStore::sync_hash(&rtx).unwrap().unwrap();
    let expected_hash = new_chain.last().unwrap().0.block_hash();
    assert_eq!(&sync_hash[..], AsRef::<[u8]>::as_ref(&expected_hash));
}

#[test]
fn reorganize_with_shorter_new_chain() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 10, zero_hash());
    store.insert_headers_batch(&chain).unwrap();

    // Fork at height 3: disconnect heights 4..9, connect only 4..5
    let fork_prev = chain[3].0.block_hash();
    let new_chain: Vec<(Header, u32)> = (4..=5)
        .map(|h| {
            let header = Header {
                nonce: h + 3_000_000,
                ..make_header(h, if h == 4 { fork_prev } else { zero_hash() })
            };
            (header, h)
        })
        .collect();

    let disconnected = store.reorganize(9, 3, &new_chain).unwrap();
    assert_eq!(disconnected.len(), 6); // heights 9, 8, 7, 6, 5, 4

    let rtx = store.read_txn().unwrap();

    // Heights 0..3 should survive
    for h in 0..=3 {
        assert!(HeaderStore::get_by_height(&rtx, h).unwrap().is_some());
    }

    // Heights 4..5 should have new headers
    for (new_header, height) in &new_chain {
        let stored = HeaderStore::get_by_height(&rtx, *height).unwrap().unwrap();
        assert_eq!(stored.header, *new_header);
    }

    // Heights 6..9 should not exist
    for h in 6..=9 {
        assert!(HeaderStore::get_by_height(&rtx, h).unwrap().is_none());
    }

    // Total count: 4 original (0..3) + 2 new (4..5) = 6
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 6);

    // Sync progress should point to height 5
    let sync_h = MetaStore::sync_height(&rtx).unwrap().unwrap();
    assert_eq!(sync_h, 5);
}

#[test]
fn reorganize_with_longer_new_chain() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 5, zero_hash());
    store.insert_headers_batch(&chain).unwrap();

    // Fork at height 2: disconnect heights 3..4, connect heights 3..7
    let fork_prev = chain[2].0.block_hash();
    let new_chain: Vec<(Header, u32)> = (3..=7)
        .map(|h| {
            let header = Header {
                nonce: h + 4_000_000,
                ..make_header(h, if h == 3 { fork_prev } else { zero_hash() })
            };
            (header, h)
        })
        .collect();

    let disconnected = store.reorganize(4, 2, &new_chain).unwrap();
    assert_eq!(disconnected.len(), 2); // heights 4, 3

    let rtx = store.read_txn().unwrap();

    // Total: 3 original (0..2) + 5 new (3..7) = 8
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 8);

    let tip = HeaderStore::tip(&rtx).unwrap().unwrap();
    assert_eq!(tip.height, 7);

    let sync_h = MetaStore::sync_height(&rtx).unwrap().unwrap();
    assert_eq!(sync_h, 7);
}

#[test]
fn reorganize_with_empty_new_chain() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 5, zero_hash());
    store.insert_headers_batch(&chain).unwrap();

    // Disconnect heights 3..4 but provide no new headers
    let disconnected = store.reorganize(4, 2, &[]).unwrap();
    assert_eq!(disconnected.len(), 2);

    let rtx = store.read_txn().unwrap();
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 3);

    let tip = HeaderStore::tip(&rtx).unwrap().unwrap();
    assert_eq!(tip.height, 2);

    // Sync progress should point to fork_height (2) since no new headers
    // were connected -- set_sync_progress was called with fork_height and
    // a zero hash.
    let sync_h = MetaStore::sync_height(&rtx).unwrap().unwrap();
    assert_eq!(sync_h, 2);
}

#[test]
fn reorganize_single_block_reorg() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 3, zero_hash());
    store.insert_headers_batch(&chain).unwrap();

    // Replace only the tip (height 2)
    let fork_prev = chain[1].0.block_hash();
    let new_header = Header {
        nonce: 5_000_000,
        ..make_header(2, fork_prev)
    };

    let disconnected = store.reorganize(2, 1, &[(new_header, 2)]).unwrap();
    assert_eq!(disconnected.len(), 1);
    assert_eq!(disconnected[0], chain[2].0.block_hash());

    let rtx = store.read_txn().unwrap();
    let stored = HeaderStore::get_by_height(&rtx, 2).unwrap().unwrap();
    assert_eq!(stored.header, new_header);
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 3);
}

// ---------------------------------------------------------------------------
// Sub-store accessors
// ---------------------------------------------------------------------------

#[test]
fn accessor_methods_return_functional_substores() {
    let (store, _dir) = temp_store();

    // The sub-store accessors should return usable zero-sized structs
    let _headers = store.headers();
    let _meta = store.meta();
    let _peers = store.peers();

    // Verify we can use them (they are just unit structs, but the test
    // confirms the API compiles and the types are public)
    let header = make_header(0, zero_hash());
    let wtx = store.write_txn().unwrap();
    HeaderStore::insert(&wtx, &header, 0).unwrap();
    wtx.commit().unwrap();

    let rtx = store.read_txn().unwrap();
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 1);
}

// ---------------------------------------------------------------------------
// raw_db access
// ---------------------------------------------------------------------------

#[test]
fn raw_db_returns_underlying_database() {
    let (store, _dir) = temp_store();

    // raw_db should allow us to create transactions directly
    let db = store.raw_db();
    let wtx = db.begin_write().unwrap();
    MetaStore::set(&wtx, "via_raw_db", b"works").unwrap();
    wtx.commit().unwrap();

    let rtx = db.begin_read().unwrap();
    let val = MetaStore::get(&rtx, "via_raw_db").unwrap().unwrap();
    assert_eq!(val, b"works");
}

// ---------------------------------------------------------------------------
// Persistence across close and reopen
// ---------------------------------------------------------------------------

#[test]
fn data_persists_across_close_and_reopen() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("persist.redb");
    let chain = make_chain(0, 5, zero_hash());

    // First session: insert data
    {
        let store = NodeStore::open(&db_path).unwrap();
        store.insert_headers_batch(&chain).unwrap();
    }

    // Second session: verify data
    {
        let store = NodeStore::open(&db_path).unwrap();
        let rtx = store.read_txn().unwrap();
        assert_eq!(HeaderStore::count(&rtx).unwrap(), 5);

        for (header, height) in &chain {
            let stored = HeaderStore::get_by_height(&rtx, *height).unwrap().unwrap();
            assert_eq!(stored.header, *header);
        }

        let tip = HeaderStore::tip(&rtx).unwrap().unwrap();
        assert_eq!(tip.height, 4);
    }
}
