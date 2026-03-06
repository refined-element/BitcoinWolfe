//! Integration tests for the new LightningManager RPC methods:
//! connect_peer, open_channel, create_invoice, pay_invoice.

use std::sync::Arc;

use bitcoin::hashes::Hash as _;
use bitcoin::BlockHash;
use tempfile::TempDir;

use wolfe_lightning::error::LightningError;
use wolfe_lightning::LightningManager;
use wolfe_mempool::Mempool;
use wolfe_store::NodeStore;
use wolfe_types::config::{LightningConfig, MempoolConfig};

/// Build a real LightningManager backed by temp dirs.
fn setup() -> (LightningManager, TempDir, TempDir) {
    let store_dir = TempDir::new().unwrap();
    let ln_dir = TempDir::new().unwrap();

    let store = Arc::new(NodeStore::open(store_dir.path().join("test.redb")).unwrap());
    let mempool = Arc::new(Mempool::new(MempoolConfig::default()));

    let config = LightningConfig {
        enabled: true,
        listen_port: 0, // not starting listener in tests
        ..LightningConfig::default()
    };

    let genesis_hash = BlockHash::all_zeros();

    let (manager, _sender, _broadcast_rx) = LightningManager::new(
        config,
        bitcoin::Network::Regtest,
        ln_dir.path(),
        store,
        mempool,
        genesis_hash,
        0,
    )
    .unwrap();

    (manager, store_dir, ln_dir)
}

// ── create_invoice tests ────────────────────────────────────────────────

#[test]
fn create_invoice_with_amount_and_description() {
    let (mgr, _sd, _ld) = setup();

    let invoice_str = mgr
        .create_invoice(Some(100_000), "test payment", Some(3600))
        .unwrap();

    // Must be a valid BOLT11 string starting with "lnbcrt" (regtest)
    assert!(
        invoice_str.starts_with("lnbcrt"),
        "expected regtest invoice prefix, got: {}",
        &invoice_str[..20]
    );

    // Parse it back to verify it's valid
    let invoice: lightning_invoice::Bolt11Invoice = invoice_str.parse().unwrap();
    assert_eq!(invoice.amount_milli_satoshis(), Some(100_000));
}

#[test]
fn create_invoice_zero_amount() {
    let (mgr, _sd, _ld) = setup();

    let invoice_str = mgr
        .create_invoice(None, "donation", None)
        .unwrap();

    let invoice: lightning_invoice::Bolt11Invoice = invoice_str.parse().unwrap();
    assert_eq!(invoice.amount_milli_satoshis(), None);
}

#[test]
fn create_invoice_with_custom_expiry() {
    let (mgr, _sd, _ld) = setup();

    let invoice_str = mgr
        .create_invoice(Some(50_000), "expiry test", Some(60))
        .unwrap();

    let invoice: lightning_invoice::Bolt11Invoice = invoice_str.parse().unwrap();

    // Verify the expiry was set (LDK uses Duration)
    let expiry = invoice.expiry_time();
    assert_eq!(expiry.as_secs(), 60);
}

#[test]
fn create_invoice_uses_our_node_id() {
    let (mgr, _sd, _ld) = setup();

    let invoice_str = mgr
        .create_invoice(Some(1000), "node id check", None)
        .unwrap();

    let invoice: lightning_invoice::Bolt11Invoice = invoice_str.parse().unwrap();
    let payee = invoice
        .payee_pub_key()
        .copied()
        .unwrap_or_else(|| invoice.recover_payee_pub_key());

    assert_eq!(
        payee,
        mgr.node_id(),
        "invoice payee should be our node"
    );
}

#[test]
fn create_multiple_invoices_unique_hashes() {
    let (mgr, _sd, _ld) = setup();

    let inv1 = mgr.create_invoice(Some(1000), "first", None).unwrap();
    let inv2 = mgr.create_invoice(Some(1000), "second", None).unwrap();

    let parsed1: lightning_invoice::Bolt11Invoice = inv1.parse().unwrap();
    let parsed2: lightning_invoice::Bolt11Invoice = inv2.parse().unwrap();

    assert_ne!(
        parsed1.payment_hash(),
        parsed2.payment_hash(),
        "each invoice must have a unique payment hash"
    );
}

// ── pay_invoice error tests ─────────────────────────────────────────────

#[test]
fn pay_invoice_rejects_garbage_string() {
    let (mgr, _sd, _ld) = setup();

    let result = mgr.pay_invoice("not-a-real-invoice");
    assert!(result.is_err());

    match result.unwrap_err() {
        LightningError::Invoice(msg) => {
            assert!(msg.contains("invalid invoice"), "got: {}", msg);
        }
        other => panic!("expected Invoice error, got: {:?}", other),
    }
}

#[test]
fn pay_invoice_rejects_empty_string() {
    let (mgr, _sd, _ld) = setup();

    let result = mgr.pay_invoice("");
    assert!(result.is_err());
}

// ── open_channel error tests ────────────────────────────────────────────

#[test]
fn open_channel_fails_for_unknown_peer() {
    let (mgr, _sd, _ld) = setup();

    // Use a random pubkey that we're not connected to
    let random_key = bitcoin::secp256k1::PublicKey::from_slice(
        &hex::decode("02eec7245d6b7d2ccb30380bfbe2a3648cd7a942653f5aa340edcea1f283686619")
            .unwrap(),
    )
    .unwrap();

    let result = mgr.open_channel(random_key, 100_000, 0);
    assert!(result.is_err(), "should fail when peer is not connected");

    match result.unwrap_err() {
        LightningError::Channel(msg) => {
            // LDK returns APE::APIMisuseError for unknown peer
            assert!(!msg.is_empty());
        }
        other => panic!("expected Channel error, got: {:?}", other),
    }
}

#[test]
fn open_channel_fails_for_zero_amount() {
    let (mgr, _sd, _ld) = setup();

    let random_key = bitcoin::secp256k1::PublicKey::from_slice(
        &hex::decode("02eec7245d6b7d2ccb30380bfbe2a3648cd7a942653f5aa340edcea1f283686619")
            .unwrap(),
    )
    .unwrap();

    let result = mgr.open_channel(random_key, 0, 0);
    assert!(result.is_err(), "zero amount should be rejected");
}

// ── connect_peer error tests ────────────────────────────────────────────

#[tokio::test]
async fn connect_peer_fails_for_unreachable_addr() {
    let (mgr, _sd, _ld) = setup();

    let random_key = bitcoin::secp256k1::PublicKey::from_slice(
        &hex::decode("02eec7245d6b7d2ccb30380bfbe2a3648cd7a942653f5aa340edcea1f283686619")
            .unwrap(),
    )
    .unwrap();

    // Use a local port that nothing is listening on
    let addr: std::net::SocketAddr = "127.0.0.1:19999".parse().unwrap();
    let result = mgr.connect_peer(random_key, addr).await;

    assert!(result.is_err(), "should fail connecting to unreachable addr");
    match result.unwrap_err() {
        LightningError::PeerConnection(msg) => {
            assert!(msg.contains("failed to connect"), "got: {}", msg);
        }
        other => panic!("expected PeerConnection error, got: {:?}", other),
    }
}

// ── node_id consistency ─────────────────────────────────────────────────

#[test]
fn node_id_is_valid_pubkey() {
    let (mgr, _sd, _ld) = setup();

    let node_id = mgr.node_id();
    // A valid secp256k1 public key is 33 bytes compressed
    assert_eq!(node_id.serialize().len(), 33);
}

#[test]
fn node_id_is_deterministic_for_same_seed() {
    // Two managers from the same store should have the same node_id
    // because the seed is persisted
    let store_dir = TempDir::new().unwrap();
    let store = Arc::new(NodeStore::open(store_dir.path().join("test.redb")).unwrap());
    let mempool = Arc::new(Mempool::new(MempoolConfig::default()));
    let genesis_hash = BlockHash::all_zeros();

    let config = LightningConfig {
        enabled: true,
        listen_port: 0,
        ..LightningConfig::default()
    };

    let ln_dir1 = TempDir::new().unwrap();
    let (mgr1, _, _) = LightningManager::new(
        config.clone(),
        bitcoin::Network::Regtest,
        ln_dir1.path(),
        store.clone(),
        mempool.clone(),
        genesis_hash,
        0,
    )
    .unwrap();
    let id1 = mgr1.node_id();
    drop(mgr1);

    let ln_dir2 = TempDir::new().unwrap();
    let (mgr2, _, _) = LightningManager::new(
        config,
        bitcoin::Network::Regtest,
        ln_dir2.path(),
        store,
        mempool,
        genesis_hash,
        0,
    )
    .unwrap();
    let id2 = mgr2.node_id();

    assert_eq!(id1, id2, "same seed should produce same node_id");
}
