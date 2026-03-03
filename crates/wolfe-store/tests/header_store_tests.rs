//! Comprehensive tests for the `HeaderStore` sub-store.
//!
//! Tests cover insert, retrieve (by hash and height), tip, count, remove,
//! disconnect_range, and cross-index consistency guarantees.

use bitcoin::block::Header;
use bitcoin::hashes::Hash;
use bitcoin::BlockHash;
use tempfile::TempDir;
use wolfe_store::{HeaderStore, NodeStore};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Create a fresh `NodeStore` backed by a temp directory.
/// Returns both the store and the `TempDir` guard (dropping the guard deletes
/// the directory, so keep it alive for the duration of the test).
fn temp_store() -> (NodeStore, TempDir) {
    let dir = TempDir::new().expect("create temp dir");
    let db_path = dir.path().join("test.redb");
    let store = NodeStore::open(&db_path).expect("open store");
    (store, dir)
}

/// Build a synthetic but valid `bitcoin::block::Header`.
///
/// We vary `nonce` and `prev_blockhash` so that each header has a unique
/// block hash. The height parameter is used to derive the nonce so we can
/// create deterministic chains.
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

/// Build a chain of headers starting from a given previous hash.
///
/// Returns a vec of (Header, height) pairs suitable for batch insertion.
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

/// The genesis-like "zero" hash used as prev_blockhash for the first header.
fn zero_hash() -> BlockHash {
    BlockHash::from_raw_hash(bitcoin::hashes::sha256d::Hash::from_byte_array([0u8; 32]))
}

// ---------------------------------------------------------------------------
// Insert and retrieve by hash
// ---------------------------------------------------------------------------

#[test]
fn insert_and_get_by_hash() {
    let (store, _dir) = temp_store();
    let header = make_header(0, zero_hash());
    let hash = header.block_hash();

    // Insert
    {
        let wtx = store.write_txn().unwrap();
        HeaderStore::insert(&wtx, &header, 0).unwrap();
        wtx.commit().unwrap();
    }

    // Retrieve
    let rtx = store.read_txn().unwrap();
    let stored = HeaderStore::get_by_hash(&rtx, &hash)
        .unwrap()
        .expect("header should exist");
    assert_eq!(stored.hash, hash);
    assert_eq!(stored.height, 0);
    assert_eq!(stored.header, header);
}

#[test]
fn get_by_hash_nonexistent_returns_none() {
    let (store, _dir) = temp_store();
    let missing_hash =
        BlockHash::from_raw_hash(bitcoin::hashes::sha256d::Hash::from_byte_array([0xff; 32]));

    // Ensure tables exist by inserting then removing a dummy header.
    {
        let wtx = store.write_txn().unwrap();
        let dummy = make_header(0, zero_hash());
        HeaderStore::insert(&wtx, &dummy, 0).unwrap();
        HeaderStore::remove(&wtx, &dummy.block_hash()).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let result = HeaderStore::get_by_hash(&rtx, &missing_hash).unwrap();
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// Insert and retrieve by height
// ---------------------------------------------------------------------------

#[test]
fn insert_and_get_by_height() {
    let (store, _dir) = temp_store();
    let header = make_header(42, zero_hash());
    let hash = header.block_hash();

    {
        let wtx = store.write_txn().unwrap();
        HeaderStore::insert(&wtx, &header, 42).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let stored = HeaderStore::get_by_height(&rtx, 42)
        .unwrap()
        .expect("header should exist at height 42");
    assert_eq!(stored.hash, hash);
    assert_eq!(stored.height, 42);
    assert_eq!(stored.header, header);
}

#[test]
fn get_by_height_nonexistent_returns_none() {
    let (store, _dir) = temp_store();

    // Ensure tables exist by inserting then removing a dummy header.
    {
        let wtx = store.write_txn().unwrap();
        let dummy = make_header(0, zero_hash());
        HeaderStore::insert(&wtx, &dummy, 0).unwrap();
        HeaderStore::remove(&wtx, &dummy.block_hash()).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let result = HeaderStore::get_by_height(&rtx, 999).unwrap();
    assert!(result.is_none());
}

// ---------------------------------------------------------------------------
// Cross-index consistency: hash and height lookups agree
// ---------------------------------------------------------------------------

#[test]
fn hash_and_height_lookups_return_same_header() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 5, zero_hash());

    {
        let wtx = store.write_txn().unwrap();
        for (header, height) in &chain {
            HeaderStore::insert(&wtx, header, *height).unwrap();
        }
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    for (header, height) in &chain {
        let by_hash = HeaderStore::get_by_hash(&rtx, &header.block_hash())
            .unwrap()
            .expect("get_by_hash should find header");
        let by_height = HeaderStore::get_by_height(&rtx, *height)
            .unwrap()
            .expect("get_by_height should find header");

        assert_eq!(by_hash.hash, by_height.hash);
        assert_eq!(by_hash.height, by_height.height);
        assert_eq!(by_hash.header, by_height.header);
        assert_eq!(by_hash.height, *height);
        assert_eq!(by_hash.header, *header);
    }
}

// ---------------------------------------------------------------------------
// tip()
// ---------------------------------------------------------------------------

#[test]
fn tip_on_empty_store_returns_none() {
    let (store, _dir) = temp_store();

    // Need to open the tables first so they exist for the read
    {
        let wtx = store.write_txn().unwrap();
        // Insert then remove to force table creation
        let header = make_header(0, zero_hash());
        HeaderStore::insert(&wtx, &header, 0).unwrap();
        HeaderStore::remove(&wtx, &header.block_hash()).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let tip = HeaderStore::tip(&rtx).unwrap();
    assert!(tip.is_none());
}

#[test]
fn tip_returns_highest_height() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 10, zero_hash());

    {
        let wtx = store.write_txn().unwrap();
        for (header, height) in &chain {
            HeaderStore::insert(&wtx, header, *height).unwrap();
        }
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let tip = HeaderStore::tip(&rtx).unwrap().expect("tip should exist");
    let (last_header, last_height) = chain.last().unwrap();
    assert_eq!(tip.height, *last_height);
    assert_eq!(tip.hash, last_header.block_hash());
}

#[test]
fn tip_updates_after_inserting_higher_header() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 3, zero_hash());

    // Insert first two
    {
        let wtx = store.write_txn().unwrap();
        for (header, height) in &chain[..2] {
            HeaderStore::insert(&wtx, header, *height).unwrap();
        }
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let tip = HeaderStore::tip(&rtx).unwrap().unwrap();
    assert_eq!(tip.height, 1);
    drop(rtx);

    // Insert the third
    {
        let wtx = store.write_txn().unwrap();
        let (header, height) = &chain[2];
        HeaderStore::insert(&wtx, header, *height).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let tip = HeaderStore::tip(&rtx).unwrap().unwrap();
    assert_eq!(tip.height, 2);
}

// ---------------------------------------------------------------------------
// count()
// ---------------------------------------------------------------------------

#[test]
fn count_reflects_number_of_stored_headers() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 7, zero_hash());

    {
        let wtx = store.write_txn().unwrap();
        for (header, height) in &chain {
            HeaderStore::insert(&wtx, header, *height).unwrap();
        }
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let count = HeaderStore::count(&rtx).unwrap();
    assert_eq!(count, 7);
}

#[test]
fn count_decreases_after_remove() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 5, zero_hash());

    {
        let wtx = store.write_txn().unwrap();
        for (header, height) in &chain {
            HeaderStore::insert(&wtx, header, *height).unwrap();
        }
        wtx.commit().unwrap();
    }

    // Remove the last header
    {
        let wtx = store.write_txn().unwrap();
        HeaderStore::remove(&wtx, &chain[4].0.block_hash()).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 4);
}

// ---------------------------------------------------------------------------
// remove()
// ---------------------------------------------------------------------------

#[test]
fn remove_cleans_up_all_three_tables() {
    let (store, _dir) = temp_store();
    let header = make_header(10, zero_hash());
    let hash = header.block_hash();

    {
        let wtx = store.write_txn().unwrap();
        HeaderStore::insert(&wtx, &header, 10).unwrap();
        wtx.commit().unwrap();
    }

    // Verify it exists
    {
        let rtx = store.read_txn().unwrap();
        assert!(HeaderStore::get_by_hash(&rtx, &hash).unwrap().is_some());
        assert!(HeaderStore::get_by_height(&rtx, 10).unwrap().is_some());
    }

    // Remove it
    {
        let wtx = store.write_txn().unwrap();
        HeaderStore::remove(&wtx, &hash).unwrap();
        wtx.commit().unwrap();
    }

    // Verify all three indices are clean
    let rtx = store.read_txn().unwrap();
    assert!(HeaderStore::get_by_hash(&rtx, &hash).unwrap().is_none());
    assert!(HeaderStore::get_by_height(&rtx, 10).unwrap().is_none());
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 0);
}

#[test]
fn remove_nonexistent_does_not_error() {
    let (store, _dir) = temp_store();
    let fake_hash =
        BlockHash::from_raw_hash(bitcoin::hashes::sha256d::Hash::from_byte_array([0xdd; 32]));

    // Need tables to exist
    {
        let wtx = store.write_txn().unwrap();
        let h = make_header(0, zero_hash());
        HeaderStore::insert(&wtx, &h, 0).unwrap();
        HeaderStore::remove(&wtx, &h.block_hash()).unwrap();
        wtx.commit().unwrap();
    }

    // Removing a hash that was never inserted should not panic or error
    let wtx = store.write_txn().unwrap();
    let result = HeaderStore::remove(&wtx, &fake_hash);
    assert!(result.is_ok());
}

#[test]
fn remove_does_not_affect_other_headers() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 3, zero_hash());

    {
        let wtx = store.write_txn().unwrap();
        for (header, height) in &chain {
            HeaderStore::insert(&wtx, header, *height).unwrap();
        }
        wtx.commit().unwrap();
    }

    // Remove the middle header
    {
        let wtx = store.write_txn().unwrap();
        HeaderStore::remove(&wtx, &chain[1].0.block_hash()).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert!(HeaderStore::get_by_hash(&rtx, &chain[0].0.block_hash())
        .unwrap()
        .is_some());
    assert!(HeaderStore::get_by_hash(&rtx, &chain[1].0.block_hash())
        .unwrap()
        .is_none());
    assert!(HeaderStore::get_by_hash(&rtx, &chain[2].0.block_hash())
        .unwrap()
        .is_some());
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 2);
}

// ---------------------------------------------------------------------------
// disconnect_range()
// ---------------------------------------------------------------------------

#[test]
fn disconnect_range_removes_correct_headers() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 10, zero_hash());

    {
        let wtx = store.write_txn().unwrap();
        for (header, height) in &chain {
            HeaderStore::insert(&wtx, header, *height).unwrap();
        }
        wtx.commit().unwrap();
    }

    // Disconnect heights 7, 8, 9 (from_height=9, to_height=7)
    let disconnected;
    {
        let wtx = store.write_txn().unwrap();
        disconnected = HeaderStore::disconnect_range(&wtx, 9, 7).unwrap();
        wtx.commit().unwrap();
    }

    // Should have removed 3 headers (heights 9, 8, 7)
    assert_eq!(disconnected.len(), 3);

    // Disconnected hashes should be in descending height order
    assert_eq!(disconnected[0], chain[9].0.block_hash());
    assert_eq!(disconnected[1], chain[8].0.block_hash());
    assert_eq!(disconnected[2], chain[7].0.block_hash());

    // Heights 0..6 should still exist
    let rtx = store.read_txn().unwrap();
    for h in 0..7 {
        assert!(
            HeaderStore::get_by_height(&rtx, h).unwrap().is_some(),
            "height {} should still exist",
            h
        );
    }

    // Heights 7..9 should be gone
    for h in 7..10 {
        assert!(
            HeaderStore::get_by_height(&rtx, h).unwrap().is_none(),
            "height {} should have been disconnected",
            h
        );
    }

    assert_eq!(HeaderStore::count(&rtx).unwrap(), 7);
}

#[test]
fn disconnect_range_single_height() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 5, zero_hash());

    {
        let wtx = store.write_txn().unwrap();
        for (header, height) in &chain {
            HeaderStore::insert(&wtx, header, *height).unwrap();
        }
        wtx.commit().unwrap();
    }

    // Disconnect only height 4 (from=4, to=4)
    let disconnected;
    {
        let wtx = store.write_txn().unwrap();
        disconnected = HeaderStore::disconnect_range(&wtx, 4, 4).unwrap();
        wtx.commit().unwrap();
    }

    assert_eq!(disconnected.len(), 1);
    assert_eq!(disconnected[0], chain[4].0.block_hash());

    let rtx = store.read_txn().unwrap();
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 4);
    assert!(HeaderStore::get_by_height(&rtx, 4).unwrap().is_none());
}

#[test]
fn disconnect_range_updates_tip() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 10, zero_hash());

    {
        let wtx = store.write_txn().unwrap();
        for (header, height) in &chain {
            HeaderStore::insert(&wtx, header, *height).unwrap();
        }
        wtx.commit().unwrap();
    }

    // Disconnect the top 5
    {
        let wtx = store.write_txn().unwrap();
        HeaderStore::disconnect_range(&wtx, 9, 5).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let tip = HeaderStore::tip(&rtx).unwrap().unwrap();
    assert_eq!(tip.height, 4);
    assert_eq!(tip.hash, chain[4].0.block_hash());
}

#[test]
fn disconnect_range_missing_heights_are_skipped() {
    let (store, _dir) = temp_store();

    // Insert only heights 0, 1, 2 (not 3, 4)
    let chain = make_chain(0, 3, zero_hash());
    {
        let wtx = store.write_txn().unwrap();
        for (header, height) in &chain {
            HeaderStore::insert(&wtx, header, *height).unwrap();
        }
        wtx.commit().unwrap();
    }

    // Disconnect range 0..4 -- heights 3 and 4 don't exist, should be skipped
    let disconnected;
    {
        let wtx = store.write_txn().unwrap();
        disconnected = HeaderStore::disconnect_range(&wtx, 4, 0).unwrap();
        wtx.commit().unwrap();
    }

    // Only 3 actual headers were removed (heights 2, 1, 0)
    assert_eq!(disconnected.len(), 3);
}

// ---------------------------------------------------------------------------
// Insert overwrites at the same height (e.g. during reorg)
// ---------------------------------------------------------------------------

#[test]
fn insert_at_existing_height_overwrites() {
    let (store, _dir) = temp_store();
    let header_a = make_header(5, zero_hash());
    let header_b = Header {
        nonce: 999_999, // Different nonce produces a different hash
        ..make_header(5, zero_hash())
    };

    assert_ne!(
        header_a.block_hash(),
        header_b.block_hash(),
        "test setup: headers must have different hashes"
    );

    // Insert header_a at height 5
    {
        let wtx = store.write_txn().unwrap();
        HeaderStore::insert(&wtx, &header_a, 5).unwrap();
        wtx.commit().unwrap();
    }

    // Overwrite height 5 with header_b
    {
        let wtx = store.write_txn().unwrap();
        HeaderStore::insert(&wtx, &header_b, 5).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let stored = HeaderStore::get_by_height(&rtx, 5).unwrap().unwrap();
    assert_eq!(stored.hash, header_b.block_hash());
    assert_eq!(stored.header, header_b);
}

// ---------------------------------------------------------------------------
// Large batch insert
// ---------------------------------------------------------------------------

#[test]
fn large_batch_insert() {
    let (store, _dir) = temp_store();
    let chain = make_chain(0, 500, zero_hash());

    {
        let wtx = store.write_txn().unwrap();
        for (header, height) in &chain {
            HeaderStore::insert(&wtx, header, *height).unwrap();
        }
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert_eq!(HeaderStore::count(&rtx).unwrap(), 500);

    let tip = HeaderStore::tip(&rtx).unwrap().unwrap();
    assert_eq!(tip.height, 499);

    // Spot-check a few heights
    for h in [0u32, 1, 50, 100, 250, 499] {
        let stored = HeaderStore::get_by_height(&rtx, h)
            .unwrap()
            .unwrap_or_else(|| panic!("header at height {} should exist", h));
        assert_eq!(stored.height, h);
        assert_eq!(stored.header, chain[h as usize].0);
    }
}

// ---------------------------------------------------------------------------
// Serialisation roundtrip: the header bytes stored in redb should
// deserialise back to the exact same header.
// ---------------------------------------------------------------------------

#[test]
fn header_serialisation_roundtrip() {
    let (store, _dir) = temp_store();
    let header = make_header(0, zero_hash());
    let hash = header.block_hash();

    {
        let wtx = store.write_txn().unwrap();
        HeaderStore::insert(&wtx, &header, 0).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let stored = HeaderStore::get_by_hash(&rtx, &hash).unwrap().unwrap();

    // The deserialized header should produce the same hash when re-hashed.
    assert_eq!(stored.header.block_hash(), hash);
    assert_eq!(stored.header.version, header.version);
    assert_eq!(stored.header.prev_blockhash, header.prev_blockhash);
    assert_eq!(stored.header.merkle_root, header.merkle_root);
    assert_eq!(stored.header.time, header.time);
    assert_eq!(stored.header.bits, header.bits);
    assert_eq!(stored.header.nonce, header.nonce);
}
