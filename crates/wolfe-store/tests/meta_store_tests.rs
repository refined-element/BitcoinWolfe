//! Comprehensive tests for the `MetaStore` sub-store.
//!
//! Tests cover generic get/set, typed helpers (sync_height, sync_hash,
//! user_agent, network, db_version), set_sync_progress, init_if_needed
//! idempotency, remove, and get_required error paths.

use tempfile::TempDir;
use wolfe_store::meta::{MetaStore, CURRENT_DB_VERSION};
use wolfe_store::NodeStore;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_store() -> (NodeStore, TempDir) {
    let dir = TempDir::new().expect("create temp dir");
    let db_path = dir.path().join("test.redb");
    let store = NodeStore::open(&db_path).expect("open store");
    (store, dir)
}

// ---------------------------------------------------------------------------
// Generic get / set
// ---------------------------------------------------------------------------

#[test]
fn set_and_get_roundtrip() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set(&wtx, "test_key", b"hello world").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let val = MetaStore::get(&rtx, "test_key")
        .unwrap()
        .expect("key should exist");
    assert_eq!(val, b"hello world");
}

#[test]
fn get_nonexistent_key_returns_none() {
    let (store, _dir) = temp_store();

    let rtx = store.read_txn().unwrap();
    let val = MetaStore::get(&rtx, "no_such_key").unwrap();
    assert!(val.is_none());
}

#[test]
fn set_overwrites_existing_value() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set(&wtx, "key", b"first").unwrap();
        wtx.commit().unwrap();
    }
    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set(&wtx, "key", b"second").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let val = MetaStore::get(&rtx, "key").unwrap().unwrap();
    assert_eq!(val, b"second");
}

#[test]
fn set_empty_value() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set(&wtx, "empty", b"").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let val = MetaStore::get(&rtx, "empty").unwrap().unwrap();
    assert!(val.is_empty());
}

// ---------------------------------------------------------------------------
// get_required
// ---------------------------------------------------------------------------

#[test]
fn get_required_returns_value_when_present() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set(&wtx, "req_key", b"payload").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let val = MetaStore::get_required(&rtx, "req_key").unwrap();
    assert_eq!(val, b"payload");
}

#[test]
fn get_required_errors_when_absent() {
    let (store, _dir) = temp_store();

    let rtx = store.read_txn().unwrap();
    let result = MetaStore::get_required(&rtx, "missing");
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("missing"),
        "error message should mention the key name: {}",
        err_msg
    );
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

#[test]
fn remove_deletes_key() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set(&wtx, "ephemeral", b"data").unwrap();
        wtx.commit().unwrap();
    }

    // Verify it exists
    {
        let rtx = store.read_txn().unwrap();
        assert!(MetaStore::get(&rtx, "ephemeral").unwrap().is_some());
    }

    // Remove it
    {
        let wtx = store.write_txn().unwrap();
        MetaStore::remove(&wtx, "ephemeral").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert!(MetaStore::get(&rtx, "ephemeral").unwrap().is_none());
}

#[test]
fn remove_nonexistent_key_does_not_error() {
    let (store, _dir) = temp_store();

    let wtx = store.write_txn().unwrap();
    let result = MetaStore::remove(&wtx, "never_existed");
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// Sync height / sync hash
// ---------------------------------------------------------------------------

#[test]
fn sync_height_roundtrip() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_sync_progress(&wtx, 840_000, &[0xaa; 32]).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let height = MetaStore::sync_height(&rtx).unwrap().expect("should exist");
    assert_eq!(height, 840_000);
}

#[test]
fn sync_hash_roundtrip() {
    let (store, _dir) = temp_store();
    let expected_hash = [0xbb; 32];

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_sync_progress(&wtx, 100, &expected_hash).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let hash = MetaStore::sync_hash(&rtx).unwrap().expect("should exist");
    assert_eq!(hash, expected_hash);
}

#[test]
fn sync_height_returns_none_when_never_set() {
    let (store, _dir) = temp_store();

    let rtx = store.read_txn().unwrap();
    let height = MetaStore::sync_height(&rtx).unwrap();
    assert!(height.is_none());
}

#[test]
fn sync_hash_returns_none_when_never_set() {
    let (store, _dir) = temp_store();

    let rtx = store.read_txn().unwrap();
    let hash = MetaStore::sync_hash(&rtx).unwrap();
    assert!(hash.is_none());
}

#[test]
fn set_sync_progress_updates_both_height_and_hash_atomically() {
    let (store, _dir) = temp_store();
    let hash_1 = [0x11; 32];
    let hash_2 = [0x22; 32];

    // First progress
    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_sync_progress(&wtx, 100, &hash_1).unwrap();
        wtx.commit().unwrap();
    }

    // Second progress overwrites
    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_sync_progress(&wtx, 200, &hash_2).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert_eq!(MetaStore::sync_height(&rtx).unwrap().unwrap(), 200);
    assert_eq!(MetaStore::sync_hash(&rtx).unwrap().unwrap(), hash_2);
}

#[test]
fn sync_height_boundary_values() {
    let (store, _dir) = temp_store();

    // Test with 0
    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_sync_progress(&wtx, 0, &[0x00; 32]).unwrap();
        wtx.commit().unwrap();
    }
    {
        let rtx = store.read_txn().unwrap();
        assert_eq!(MetaStore::sync_height(&rtx).unwrap().unwrap(), 0);
    }

    // Test with u32::MAX
    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_sync_progress(&wtx, u32::MAX, &[0xff; 32]).unwrap();
        wtx.commit().unwrap();
    }
    {
        let rtx = store.read_txn().unwrap();
        assert_eq!(MetaStore::sync_height(&rtx).unwrap().unwrap(), u32::MAX);
    }
}

// ---------------------------------------------------------------------------
// User agent
// ---------------------------------------------------------------------------

#[test]
fn user_agent_roundtrip() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_user_agent(&wtx, "/BitcoinWolfe:0.1.0/").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let ua = MetaStore::user_agent(&rtx).unwrap().expect("should exist");
    assert_eq!(ua, "/BitcoinWolfe:0.1.0/");
}

#[test]
fn user_agent_returns_none_when_not_set() {
    let (store, _dir) = temp_store();

    let rtx = store.read_txn().unwrap();
    let ua = MetaStore::user_agent(&rtx).unwrap();
    assert!(ua.is_none());
}

#[test]
fn user_agent_empty_string() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_user_agent(&wtx, "").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let ua = MetaStore::user_agent(&rtx).unwrap().unwrap();
    assert_eq!(ua, "");
}

#[test]
fn user_agent_unicode() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_user_agent(&wtx, "Wolfe Node v0.1 / test").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let ua = MetaStore::user_agent(&rtx).unwrap().unwrap();
    assert_eq!(ua, "Wolfe Node v0.1 / test");
}

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

#[test]
fn network_roundtrip() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_network(&wtx, "mainnet").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let net = MetaStore::network(&rtx).unwrap().expect("should exist");
    assert_eq!(net, "mainnet");
}

#[test]
fn network_returns_none_when_not_set() {
    let (store, _dir) = temp_store();

    let rtx = store.read_txn().unwrap();
    let net = MetaStore::network(&rtx).unwrap();
    assert!(net.is_none());
}

#[test]
fn network_various_values() {
    let (store, _dir) = temp_store();
    let networks = ["mainnet", "testnet", "regtest", "signet"];

    for net_name in &networks {
        {
            let wtx = store.write_txn().unwrap();
            MetaStore::set_network(&wtx, net_name).unwrap();
            wtx.commit().unwrap();
        }

        let rtx = store.read_txn().unwrap();
        let stored = MetaStore::network(&rtx).unwrap().unwrap();
        assert_eq!(&stored, net_name);
    }
}

// ---------------------------------------------------------------------------
// Node ID
// ---------------------------------------------------------------------------

#[test]
fn node_id_roundtrip() {
    let (store, _dir) = temp_store();
    let nonce: [u8; 8] = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_node_id(&wtx, &nonce).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let stored = MetaStore::node_id(&rtx).unwrap().expect("should exist");
    assert_eq!(stored, nonce);
}

#[test]
fn node_id_returns_none_when_not_set() {
    let (store, _dir) = temp_store();

    let rtx = store.read_txn().unwrap();
    assert!(MetaStore::node_id(&rtx).unwrap().is_none());
}

// ---------------------------------------------------------------------------
// DB version
// ---------------------------------------------------------------------------

#[test]
fn db_version_roundtrip() {
    let (store, _dir) = temp_store();

    // NodeStore::open calls init_if_needed which sets the version.
    let rtx = store.read_txn().unwrap();
    let version = MetaStore::db_version(&rtx).unwrap().expect("should exist");
    assert_eq!(version, CURRENT_DB_VERSION);
}

#[test]
fn db_version_can_be_updated() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_db_version(&wtx, 42).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let version = MetaStore::db_version(&rtx).unwrap().unwrap();
    assert_eq!(version, 42);
}

// ---------------------------------------------------------------------------
// init_if_needed
// ---------------------------------------------------------------------------

#[test]
fn init_if_needed_returns_true_on_first_call() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.redb");
    let db = redb::Database::create(&db_path).unwrap();

    let wtx = db.begin_write().unwrap();
    let was_init = MetaStore::init_if_needed(&wtx).unwrap();
    wtx.commit().unwrap();

    assert!(was_init, "first call should initialise");
}

#[test]
fn init_if_needed_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.redb");
    let db = redb::Database::create(&db_path).unwrap();

    // First call
    {
        let wtx = db.begin_write().unwrap();
        let first = MetaStore::init_if_needed(&wtx).unwrap();
        wtx.commit().unwrap();
        assert!(first);
    }

    // Second call
    {
        let wtx = db.begin_write().unwrap();
        let second = MetaStore::init_if_needed(&wtx).unwrap();
        wtx.commit().unwrap();
        assert!(!second, "second call should be a no-op");
    }

    // Third call
    {
        let wtx = db.begin_write().unwrap();
        let third = MetaStore::init_if_needed(&wtx).unwrap();
        wtx.commit().unwrap();
        assert!(!third, "third call should also be a no-op");
    }

    // Version should still be CURRENT_DB_VERSION
    let rtx = db.begin_read().unwrap();
    let version = MetaStore::db_version(&rtx).unwrap().unwrap();
    assert_eq!(version, CURRENT_DB_VERSION);
}

#[test]
fn init_if_needed_does_not_overwrite_existing_version() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.redb");
    let db = redb::Database::create(&db_path).unwrap();

    // Manually set version to something else
    {
        let wtx = db.begin_write().unwrap();
        MetaStore::set_db_version(&wtx, 99).unwrap();
        wtx.commit().unwrap();
    }

    // init_if_needed should see the existing version and skip
    {
        let wtx = db.begin_write().unwrap();
        let result = MetaStore::init_if_needed(&wtx).unwrap();
        wtx.commit().unwrap();
        assert!(!result);
    }

    // Version should still be 99
    let rtx = db.begin_read().unwrap();
    let version = MetaStore::db_version(&rtx).unwrap().unwrap();
    assert_eq!(version, 99);
}

// ---------------------------------------------------------------------------
// Multiple keys in the same transaction
// ---------------------------------------------------------------------------

#[test]
fn multiple_keys_in_one_transaction() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set_user_agent(&wtx, "/TestAgent/").unwrap();
        MetaStore::set_network(&wtx, "regtest").unwrap();
        MetaStore::set_node_id(&wtx, &[0xff; 8]).unwrap();
        MetaStore::set_sync_progress(&wtx, 500, &[0xcc; 32]).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert_eq!(MetaStore::user_agent(&rtx).unwrap().unwrap(), "/TestAgent/");
    assert_eq!(MetaStore::network(&rtx).unwrap().unwrap(), "regtest");
    assert_eq!(MetaStore::node_id(&rtx).unwrap().unwrap(), vec![0xff; 8]);
    assert_eq!(MetaStore::sync_height(&rtx).unwrap().unwrap(), 500);
    assert_eq!(MetaStore::sync_hash(&rtx).unwrap().unwrap(), [0xcc; 32]);
}

// ---------------------------------------------------------------------------
// Transaction rollback: uncommitted writes should not persist
// ---------------------------------------------------------------------------

#[test]
fn uncommitted_transaction_does_not_persist() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        MetaStore::set(&wtx, "transient", b"should not persist").unwrap();
        // Intentionally do NOT commit -- drop the transaction
        drop(wtx);
    }

    let rtx = store.read_txn().unwrap();
    let val = MetaStore::get(&rtx, "transient").unwrap();
    assert!(val.is_none(), "uncommitted write should not be visible");
}
