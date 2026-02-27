use std::sync::Arc;

use lightning::util::persist::KVStoreSync;
use redb::Database;
use tempfile::TempDir;

use wolfe_lightning::persister::WolfeKVStore;

fn setup() -> (TempDir, WolfeKVStore) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test_lightning.redb");
    let db = Arc::new(Database::create(&db_path).unwrap());
    let store = WolfeKVStore::new(db);
    (tmp, store)
}

#[test]
fn write_and_read_roundtrip() {
    let (_tmp, store) = setup();
    let data = b"hello lightning".to_vec();

    store
        .write("channel_monitors", "", "outpoint_abc", data.clone())
        .unwrap();

    let result = store.read("channel_monitors", "", "outpoint_abc").unwrap();
    assert_eq!(result, data);
}

#[test]
fn read_missing_key_returns_not_found() {
    let (_tmp, store) = setup();

    let err = store.read("ns", "", "missing_key").unwrap_err();
    assert_eq!(err.kind(), lightning::io::ErrorKind::NotFound);
}

#[test]
fn write_overwrites_existing() {
    let (_tmp, store) = setup();

    store
        .write("ns", "", "key1", b"first".to_vec())
        .unwrap();
    store
        .write("ns", "", "key1", b"second".to_vec())
        .unwrap();

    let result = store.read("ns", "", "key1").unwrap();
    assert_eq!(result, b"second");
}

#[test]
fn remove_deletes_key() {
    let (_tmp, store) = setup();

    store
        .write("ns", "sub", "key1", b"data".to_vec())
        .unwrap();

    store.remove("ns", "sub", "key1", false).unwrap();

    let err = store.read("ns", "sub", "key1").unwrap_err();
    assert_eq!(err.kind(), lightning::io::ErrorKind::NotFound);
}

#[test]
fn remove_nonexistent_key_succeeds() {
    let (_tmp, store) = setup();
    // Removing a key that doesn't exist should not error
    store.remove("ns", "", "ghost", false).unwrap();
}

#[test]
fn list_returns_keys_in_namespace() {
    let (_tmp, store) = setup();

    store
        .write("monitors", "", "outpoint_a", b"mon_a".to_vec())
        .unwrap();
    store
        .write("monitors", "", "outpoint_b", b"mon_b".to_vec())
        .unwrap();
    store
        .write("manager", "", "state", b"mgr".to_vec())
        .unwrap();

    let mut keys = store.list("monitors", "").unwrap();
    keys.sort();
    assert_eq!(keys, vec!["outpoint_a", "outpoint_b"]);

    let mgr_keys = store.list("manager", "").unwrap();
    assert_eq!(mgr_keys, vec!["state"]);
}

#[test]
fn list_with_secondary_namespace() {
    let (_tmp, store) = setup();

    store
        .write("ns", "sub1", "key_a", b"a".to_vec())
        .unwrap();
    store
        .write("ns", "sub1", "key_b", b"b".to_vec())
        .unwrap();
    store
        .write("ns", "sub2", "key_c", b"c".to_vec())
        .unwrap();

    let mut keys = store.list("ns", "sub1").unwrap();
    keys.sort();
    assert_eq!(keys, vec!["key_a", "key_b"]);

    let keys2 = store.list("ns", "sub2").unwrap();
    assert_eq!(keys2, vec!["key_c"]);
}

#[test]
fn list_empty_namespace_returns_empty() {
    let (_tmp, store) = setup();

    let keys = store.list("empty_ns", "").unwrap();
    assert!(keys.is_empty());
}

#[test]
fn write_empty_data() {
    let (_tmp, store) = setup();

    store
        .write("ns", "", "empty", Vec::new())
        .unwrap();

    let result = store.read("ns", "", "empty").unwrap();
    assert!(result.is_empty());
}

#[test]
fn write_large_data() {
    let (_tmp, store) = setup();
    let data = vec![0xAB; 1_000_000]; // 1 MB

    store
        .write("ns", "", "big", data.clone())
        .unwrap();

    let result = store.read("ns", "", "big").unwrap();
    assert_eq!(result.len(), 1_000_000);
    assert_eq!(result, data);
}

#[test]
fn persistence_across_store_instances() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test_lightning.redb");

    // Write with first instance
    {
        let db = Arc::new(Database::create(&db_path).unwrap());
        let store = WolfeKVStore::new(db);
        store
            .write("ns", "", "persist_key", b"persisted".to_vec())
            .unwrap();
    }

    // Read with second instance (same file)
    {
        let db = Arc::new(Database::create(&db_path).unwrap());
        let store = WolfeKVStore::new(db);
        let result = store.read("ns", "", "persist_key").unwrap();
        assert_eq!(result, b"persisted");
    }
}

#[test]
fn list_after_remove() {
    let (_tmp, store) = setup();

    store
        .write("ns", "", "a", b"data_a".to_vec())
        .unwrap();
    store
        .write("ns", "", "b", b"data_b".to_vec())
        .unwrap();

    store.remove("ns", "", "a", false).unwrap();

    let keys = store.list("ns", "").unwrap();
    assert_eq!(keys, vec!["b"]);
}
