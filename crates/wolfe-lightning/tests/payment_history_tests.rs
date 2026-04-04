use std::sync::Arc;

use redb::Database;
use tempfile::TempDir;

use wolfe_lightning::persister::{PaymentRecord, WolfeKVStore};

fn setup() -> (TempDir, WolfeKVStore) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test_lightning.redb");
    let db = Arc::new(Database::create(&db_path).unwrap());
    let store = WolfeKVStore::new(db);
    (tmp, store)
}

fn make_payment(
    hash: &str,
    direction: &str,
    status: &str,
    amount_msat: Option<u64>,
    fee_msat: Option<u64>,
    timestamp: u64,
) -> PaymentRecord {
    PaymentRecord {
        payment_hash: hash.to_string(),
        direction: direction.to_string(),
        status: status.to_string(),
        amount_msat,
        fee_msat,
        timestamp,
    }
}

#[test]
fn record_and_list_roundtrip() {
    let (_tmp, store) = setup();

    let record = make_payment("abc123", "send", "completed", Some(50_000), Some(100), 1000);
    store.record_payment(&record);

    let payments = store.list_payments(10);
    assert_eq!(payments.len(), 1);
    assert_eq!(payments[0].payment_hash, "abc123");
    assert_eq!(payments[0].direction, "send");
    assert_eq!(payments[0].status, "completed");
    assert_eq!(payments[0].amount_msat, Some(50_000));
    assert_eq!(payments[0].fee_msat, Some(100));
    assert_eq!(payments[0].timestamp, 1000);
}

#[test]
fn list_payments_respects_limit() {
    let (_tmp, store) = setup();

    for i in 0..5 {
        let record = make_payment(
            &format!("hash_{}", i),
            "send",
            "completed",
            Some(1000 * (i + 1)),
            None,
            100 + i,
        );
        store.record_payment(&record);
    }

    let payments = store.list_payments(2);
    assert_eq!(payments.len(), 2);
}

#[test]
fn list_payments_returns_most_recent_first() {
    let (_tmp, store) = setup();

    let early = make_payment("early", "send", "completed", Some(1000), None, 100);
    let middle = make_payment("middle", "receive", "completed", Some(2000), None, 500);
    let late = make_payment("late", "send", "completed", Some(3000), None, 900);

    // Insert in non-chronological order to ensure sorting is by key, not insertion order
    store.record_payment(&middle);
    store.record_payment(&early);
    store.record_payment(&late);

    let payments = store.list_payments(10);
    assert_eq!(payments.len(), 3);
    assert_eq!(payments[0].payment_hash, "late");
    assert_eq!(payments[0].timestamp, 900);
    assert_eq!(payments[1].payment_hash, "middle");
    assert_eq!(payments[1].timestamp, 500);
    assert_eq!(payments[2].payment_hash, "early");
    assert_eq!(payments[2].timestamp, 100);
}

#[test]
fn record_send_payment() {
    let (_tmp, store) = setup();

    let record = make_payment(
        "send_hash_001",
        "send",
        "completed",
        Some(250_000),
        Some(1_500),
        1_700_000_000,
    );
    store.record_payment(&record);

    let payments = store.list_payments(10);
    assert_eq!(payments.len(), 1);

    let p = &payments[0];
    assert_eq!(p.payment_hash, "send_hash_001");
    assert_eq!(p.direction, "send");
    assert_eq!(p.status, "completed");
    assert_eq!(p.amount_msat, Some(250_000));
    assert_eq!(p.fee_msat, Some(1_500));
    assert_eq!(p.timestamp, 1_700_000_000);
}

#[test]
fn record_receive_payment() {
    let (_tmp, store) = setup();

    let record = make_payment(
        "recv_hash_002",
        "receive",
        "completed",
        Some(100_000),
        None,
        1_700_001_000,
    );
    store.record_payment(&record);

    let payments = store.list_payments(10);
    assert_eq!(payments.len(), 1);

    let p = &payments[0];
    assert_eq!(p.payment_hash, "recv_hash_002");
    assert_eq!(p.direction, "receive");
    assert_eq!(p.status, "completed");
    assert_eq!(p.amount_msat, Some(100_000));
    assert_eq!(p.fee_msat, None);
    assert_eq!(p.timestamp, 1_700_001_000);
}

#[test]
fn record_failed_payment() {
    let (_tmp, store) = setup();

    let record = make_payment(
        "fail_hash_003",
        "send",
        "failed",
        Some(500_000),
        None,
        1_700_002_000,
    );
    store.record_payment(&record);

    let payments = store.list_payments(10);
    assert_eq!(payments.len(), 1);

    let p = &payments[0];
    assert_eq!(p.payment_hash, "fail_hash_003");
    assert_eq!(p.direction, "send");
    assert_eq!(p.status, "failed");
    assert_eq!(p.amount_msat, Some(500_000));
    assert_eq!(p.fee_msat, None);
    assert_eq!(p.timestamp, 1_700_002_000);
}

#[test]
fn empty_payment_history() {
    let (_tmp, store) = setup();

    let payments = store.list_payments(10);
    assert!(payments.is_empty());
}

#[test]
fn payment_history_persists_across_instances() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("test_lightning.redb");

    // Write with first store instance
    {
        let db = Arc::new(Database::create(&db_path).unwrap());
        let store = WolfeKVStore::new(db);

        let record = make_payment(
            "persist_hash",
            "send",
            "completed",
            Some(75_000),
            Some(200),
            1_700_003_000,
        );
        store.record_payment(&record);
    }

    // Read with a second store instance backed by the same database file
    {
        let db = Arc::new(Database::create(&db_path).unwrap());
        let store = WolfeKVStore::new(db);

        let payments = store.list_payments(10);
        assert_eq!(payments.len(), 1);

        let p = &payments[0];
        assert_eq!(p.payment_hash, "persist_hash");
        assert_eq!(p.direction, "send");
        assert_eq!(p.status, "completed");
        assert_eq!(p.amount_msat, Some(75_000));
        assert_eq!(p.fee_msat, Some(200));
        assert_eq!(p.timestamp, 1_700_003_000);
    }
}
