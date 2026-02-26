//! Comprehensive tests for the `PeerStore` sub-store.
//!
//! Tests cover upsert, get, remove, listing, banning, unbanning, ban expiry
//! purging, ban checking, random peer selection, and count operations.

use tempfile::TempDir;
use wolfe_store::{NodeStore, PeerRecord, PeerStore};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn temp_store() -> (NodeStore, TempDir) {
    let dir = TempDir::new().expect("create temp dir");
    let db_path = dir.path().join("test.redb");
    let store = NodeStore::open(&db_path).expect("open store");
    (store, dir)
}

fn make_peer(addr: &str) -> PeerRecord {
    PeerRecord {
        addr: addr.to_string(),
        services: 1,
        last_seen: 1_700_000_000,
        first_seen: 1_699_000_000,
        connection_count: 1,
        fail_count: 0,
        user_agent: "/TestNode:0.1/".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Upsert and get
// ---------------------------------------------------------------------------

#[test]
fn upsert_and_get_roundtrip() {
    let (store, _dir) = temp_store();
    let peer = make_peer("127.0.0.1:8333");

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::upsert(&wtx, &peer).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let stored = PeerStore::get(&rtx, "127.0.0.1:8333")
        .unwrap()
        .expect("peer should exist");
    assert_eq!(stored.addr, "127.0.0.1:8333");
    assert_eq!(stored.services, 1);
    assert_eq!(stored.last_seen, 1_700_000_000);
    assert_eq!(stored.user_agent, "/TestNode:0.1/");
}

#[test]
fn get_nonexistent_peer_returns_none() {
    let (store, _dir) = temp_store();

    // Need the table to exist
    {
        let wtx = store.write_txn().unwrap();
        let p = make_peer("1.1.1.1:8333");
        PeerStore::upsert(&wtx, &p).unwrap();
        PeerStore::remove(&wtx, "1.1.1.1:8333").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let result = PeerStore::get(&rtx, "10.0.0.1:8333").unwrap();
    assert!(result.is_none());
}

#[test]
fn upsert_overwrites_existing_peer() {
    let (store, _dir) = temp_store();
    let mut peer = make_peer("127.0.0.1:8333");

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::upsert(&wtx, &peer).unwrap();
        wtx.commit().unwrap();
    }

    // Update the peer
    peer.connection_count = 10;
    peer.last_seen = 1_800_000_000;
    peer.user_agent = "/Updated:0.2/".to_string();

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::upsert(&wtx, &peer).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let stored = PeerStore::get(&rtx, "127.0.0.1:8333").unwrap().unwrap();
    assert_eq!(stored.connection_count, 10);
    assert_eq!(stored.last_seen, 1_800_000_000);
    assert_eq!(stored.user_agent, "/Updated:0.2/");
}

// ---------------------------------------------------------------------------
// Remove
// ---------------------------------------------------------------------------

#[test]
fn remove_deletes_peer() {
    let (store, _dir) = temp_store();
    let peer = make_peer("127.0.0.1:8333");

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::upsert(&wtx, &peer).unwrap();
        wtx.commit().unwrap();
    }

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::remove(&wtx, "127.0.0.1:8333").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert!(PeerStore::get(&rtx, "127.0.0.1:8333").unwrap().is_none());
    assert_eq!(PeerStore::count(&rtx).unwrap(), 0);
}

#[test]
fn remove_nonexistent_does_not_error() {
    let (store, _dir) = temp_store();

    // Create table by upserting then removing
    {
        let wtx = store.write_txn().unwrap();
        let p = make_peer("1.1.1.1:8333");
        PeerStore::upsert(&wtx, &p).unwrap();
        wtx.commit().unwrap();
    }

    let wtx = store.write_txn().unwrap();
    let result = PeerStore::remove(&wtx, "10.20.30.40:8333");
    assert!(result.is_ok());
}

#[test]
fn remove_does_not_affect_other_peers() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::upsert(&wtx, &make_peer("10.0.0.1:8333")).unwrap();
        PeerStore::upsert(&wtx, &make_peer("10.0.0.2:8333")).unwrap();
        PeerStore::upsert(&wtx, &make_peer("10.0.0.3:8333")).unwrap();
        wtx.commit().unwrap();
    }

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::remove(&wtx, "10.0.0.2:8333").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert!(PeerStore::get(&rtx, "10.0.0.1:8333").unwrap().is_some());
    assert!(PeerStore::get(&rtx, "10.0.0.2:8333").unwrap().is_none());
    assert!(PeerStore::get(&rtx, "10.0.0.3:8333").unwrap().is_some());
    assert_eq!(PeerStore::count(&rtx).unwrap(), 2);
}

// ---------------------------------------------------------------------------
// Count
// ---------------------------------------------------------------------------

#[test]
fn count_reflects_number_of_peers() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        for i in 0..5 {
            PeerStore::upsert(&wtx, &make_peer(&format!("10.0.0.{}:8333", i))).unwrap();
        }
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert_eq!(PeerStore::count(&rtx).unwrap(), 5);
}

// ---------------------------------------------------------------------------
// list_all
// ---------------------------------------------------------------------------

#[test]
fn list_all_returns_all_peers_sorted_by_last_seen_desc() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        let mut p1 = make_peer("10.0.0.1:8333");
        p1.last_seen = 100;
        let mut p2 = make_peer("10.0.0.2:8333");
        p2.last_seen = 300;
        let mut p3 = make_peer("10.0.0.3:8333");
        p3.last_seen = 200;

        PeerStore::upsert(&wtx, &p1).unwrap();
        PeerStore::upsert(&wtx, &p2).unwrap();
        PeerStore::upsert(&wtx, &p3).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let all = PeerStore::list_all(&rtx).unwrap();
    assert_eq!(all.len(), 3);
    // Should be sorted by last_seen descending
    assert_eq!(all[0].addr, "10.0.0.2:8333"); // last_seen=300
    assert_eq!(all[1].addr, "10.0.0.3:8333"); // last_seen=200
    assert_eq!(all[2].addr, "10.0.0.1:8333"); // last_seen=100
}

#[test]
fn list_all_empty_store() {
    let (store, _dir) = temp_store();

    // Create table
    {
        let wtx = store.write_txn().unwrap();
        let p = make_peer("1.1.1.1:8333");
        PeerStore::upsert(&wtx, &p).unwrap();
        PeerStore::remove(&wtx, "1.1.1.1:8333").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let all = PeerStore::list_all(&rtx).unwrap();
    assert!(all.is_empty());
}

// ---------------------------------------------------------------------------
// Banning
// ---------------------------------------------------------------------------

#[test]
fn ban_and_is_banned() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::ban(&wtx, "10.0.0.1:8333", 2_000_000_000).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();

    // Should be banned if now is before the expiry
    let result = PeerStore::is_banned(&rtx, "10.0.0.1:8333", 1_999_999_999).unwrap();
    assert_eq!(result, Some(2_000_000_000));

    // Should NOT be banned if now is at or after the expiry
    let result = PeerStore::is_banned(&rtx, "10.0.0.1:8333", 2_000_000_000).unwrap();
    assert!(result.is_none());

    let result = PeerStore::is_banned(&rtx, "10.0.0.1:8333", 2_000_000_001).unwrap();
    assert!(result.is_none());
}

#[test]
fn is_banned_returns_none_for_unbanned_peer() {
    let (store, _dir) = temp_store();

    // Create table
    {
        let wtx = store.write_txn().unwrap();
        PeerStore::ban(&wtx, "x:1", 100).unwrap();
        PeerStore::unban(&wtx, "x:1").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let result = PeerStore::is_banned(&rtx, "10.0.0.1:8333", 0).unwrap();
    assert!(result.is_none());
}

#[test]
fn unban_removes_ban() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::ban(&wtx, "10.0.0.1:8333", 2_000_000_000).unwrap();
        wtx.commit().unwrap();
    }

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::unban(&wtx, "10.0.0.1:8333").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let result = PeerStore::is_banned(&rtx, "10.0.0.1:8333", 0).unwrap();
    assert!(result.is_none());
}

#[test]
fn banned_count_reflects_ban_records() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::ban(&wtx, "10.0.0.1:8333", 100).unwrap();
        PeerStore::ban(&wtx, "10.0.0.2:8333", 200).unwrap();
        PeerStore::ban(&wtx, "10.0.0.3:8333", 300).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert_eq!(PeerStore::banned_count(&rtx).unwrap(), 3);
}

// ---------------------------------------------------------------------------
// purge_expired_bans
// ---------------------------------------------------------------------------

#[test]
fn purge_expired_bans_removes_only_expired() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::ban(&wtx, "expired1:8333", 100).unwrap(); // expires at 100
        PeerStore::ban(&wtx, "expired2:8333", 200).unwrap(); // expires at 200
        PeerStore::ban(&wtx, "active:8333", 500).unwrap(); // expires at 500
        wtx.commit().unwrap();
    }

    // Purge with now_unix=250: should remove expired1 and expired2
    {
        let wtx = store.write_txn().unwrap();
        let purged = PeerStore::purge_expired_bans(&wtx, 250).unwrap();
        wtx.commit().unwrap();
        assert_eq!(purged, 2);
    }

    let rtx = store.read_txn().unwrap();
    assert_eq!(PeerStore::banned_count(&rtx).unwrap(), 1);
    // The remaining ban should be "active:8333"
    let result = PeerStore::is_banned(&rtx, "active:8333", 249).unwrap();
    assert!(result.is_some());
}

#[test]
fn purge_expired_bans_none_expired() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::ban(&wtx, "10.0.0.1:8333", 1_000).unwrap();
        PeerStore::ban(&wtx, "10.0.0.2:8333", 2_000).unwrap();
        wtx.commit().unwrap();
    }

    // now=0, nothing should be expired
    {
        let wtx = store.write_txn().unwrap();
        let purged = PeerStore::purge_expired_bans(&wtx, 0).unwrap();
        wtx.commit().unwrap();
        assert_eq!(purged, 0);
    }

    let rtx = store.read_txn().unwrap();
    assert_eq!(PeerStore::banned_count(&rtx).unwrap(), 2);
}

#[test]
fn purge_expired_bans_all_expired() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::ban(&wtx, "10.0.0.1:8333", 100).unwrap();
        PeerStore::ban(&wtx, "10.0.0.2:8333", 200).unwrap();
        wtx.commit().unwrap();
    }

    {
        let wtx = store.write_txn().unwrap();
        let purged = PeerStore::purge_expired_bans(&wtx, 999).unwrap();
        wtx.commit().unwrap();
        assert_eq!(purged, 2);
    }

    let rtx = store.read_txn().unwrap();
    assert_eq!(PeerStore::banned_count(&rtx).unwrap(), 0);
}

// ---------------------------------------------------------------------------
// get_random
// ---------------------------------------------------------------------------

#[test]
fn get_random_excludes_banned_peers() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::upsert(&wtx, &make_peer("10.0.0.1:8333")).unwrap();
        PeerStore::upsert(&wtx, &make_peer("10.0.0.2:8333")).unwrap();
        PeerStore::upsert(&wtx, &make_peer("10.0.0.3:8333")).unwrap();

        // Ban one peer with future expiry
        PeerStore::ban(&wtx, "10.0.0.2:8333", 9_999_999_999).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let random = PeerStore::get_random(&rtx, 10, 1_000).unwrap();

    // Should have 2 peers (10.0.0.2 is banned)
    assert_eq!(random.len(), 2);
    let addrs: Vec<&str> = random.iter().map(|p| p.addr.as_str()).collect();
    assert!(!addrs.contains(&"10.0.0.2:8333"));
}

#[test]
fn get_random_includes_expired_bans() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::upsert(&wtx, &make_peer("10.0.0.1:8333")).unwrap();
        PeerStore::upsert(&wtx, &make_peer("10.0.0.2:8333")).unwrap();

        // Ban 10.0.0.2 but with an expiry in the past relative to now_unix
        PeerStore::ban(&wtx, "10.0.0.2:8333", 500).unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    // now_unix=1000, ban expired at 500
    let random = PeerStore::get_random(&rtx, 10, 1_000).unwrap();
    assert_eq!(random.len(), 2, "expired ban should not exclude the peer");
}

#[test]
fn get_random_respects_count_limit() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        for i in 0..20 {
            PeerStore::upsert(&wtx, &make_peer(&format!("10.0.0.{}:8333", i))).unwrap();
        }
        // Create the banned_peers table so get_random can open it
        PeerStore::ban(&wtx, "dummy:1", 1).unwrap();
        PeerStore::unban(&wtx, "dummy:1").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let random = PeerStore::get_random(&rtx, 5, 0).unwrap();
    assert_eq!(random.len(), 5);
}

#[test]
fn get_random_returns_all_if_count_exceeds_available() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        PeerStore::upsert(&wtx, &make_peer("10.0.0.1:8333")).unwrap();
        PeerStore::upsert(&wtx, &make_peer("10.0.0.2:8333")).unwrap();
        // Create the banned_peers table so get_random can open it
        PeerStore::ban(&wtx, "dummy:1", 1).unwrap();
        PeerStore::unban(&wtx, "dummy:1").unwrap();
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    let random = PeerStore::get_random(&rtx, 100, 0).unwrap();
    assert_eq!(random.len(), 2);
}

// ---------------------------------------------------------------------------
// Multiple peers in one transaction
// ---------------------------------------------------------------------------

#[test]
fn batch_upsert_in_single_transaction() {
    let (store, _dir) = temp_store();

    {
        let wtx = store.write_txn().unwrap();
        for i in 0..50 {
            let mut peer = make_peer(&format!("192.168.1.{}:8333", i));
            peer.services = i as u64;
            PeerStore::upsert(&wtx, &peer).unwrap();
        }
        wtx.commit().unwrap();
    }

    let rtx = store.read_txn().unwrap();
    assert_eq!(PeerStore::count(&rtx).unwrap(), 50);

    // Spot-check a few
    let p = PeerStore::get(&rtx, "192.168.1.25:8333").unwrap().unwrap();
    assert_eq!(p.services, 25);
}
