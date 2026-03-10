//! Tests for wallet integration with Lightning:
//! - set_wallet() injection
//! - close_channel() cooperative and force-close
//! - FundingGenerationReady with wallet present (no real channel, but exercises codepath)

use std::sync::{Arc, Mutex};

use bitcoin::hashes::Hash as _;
use bitcoin::BlockHash;
use tempfile::TempDir;
use tokio::sync::mpsc;

use wolfe_lightning::LightningManager;
use wolfe_mempool::Mempool;
use wolfe_store::NodeStore;
use wolfe_types::config::{LightningConfig, MempoolConfig};
use wolfe_wallet::NodeWallet;

/// Build a real LightningManager, return it plus the broadcast_rx and temp dirs.
fn setup() -> (
    LightningManager,
    mpsc::UnboundedReceiver<bitcoin::Transaction>,
    TempDir,
    TempDir,
) {
    let store_dir = TempDir::new().unwrap();
    let ln_dir = TempDir::new().unwrap();
    let store = Arc::new(NodeStore::open(store_dir.path().join("test.redb")).unwrap());
    let mempool = Arc::new(Mempool::new(MempoolConfig::default()));
    let config = LightningConfig {
        enabled: true,
        listen_port: 0,
        accept_inbound_channels: true,
        ..LightningConfig::default()
    };
    let genesis_hash = BlockHash::all_zeros();
    let (manager, _sender, broadcast_rx) = LightningManager::new(
        config,
        bitcoin::Network::Regtest,
        ln_dir.path(),
        store,
        mempool,
        genesis_hash,
        0,
    )
    .unwrap();

    (manager, broadcast_rx, store_dir, ln_dir)
}

/// Create a test wallet and return it wrapped in Arc<Mutex<>>.
fn create_test_wallet() -> (Arc<Mutex<NodeWallet>>, TempDir) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("wallet.sqlite3");
    let (wallet, _mnemonic) =
        NodeWallet::create_new(&db_path, bdk_wallet::bitcoin::Network::Regtest).unwrap();
    (Arc::new(Mutex::new(wallet)), dir)
}

// ── set_wallet tests ────────────────────────────────────────────────────

#[test]
fn set_wallet_does_not_panic() {
    let (mgr, _rx, _sd, _ld) = setup();
    let (wallet, _wd) = create_test_wallet();

    mgr.set_wallet(wallet);
    // Manager should still be functional
    assert!(!mgr.node_id().serialize().is_empty());
}

#[test]
fn set_wallet_can_be_called_multiple_times() {
    let (mgr, _rx, _sd, _ld) = setup();
    let (wallet1, _wd1) = create_test_wallet();
    let (wallet2, _wd2) = create_test_wallet();

    mgr.set_wallet(wallet1);
    mgr.set_wallet(wallet2); // Should replace, not panic
}

#[tokio::test]
async fn tick_with_wallet_does_not_panic() {
    let (mgr, _rx, _sd, _ld) = setup();
    let (wallet, _wd) = create_test_wallet();

    mgr.set_wallet(wallet);

    // Multiple ticks with wallet injected should work fine
    for _ in 0..5 {
        mgr.tick().await;
    }

    // Manager still functional
    let channels = mgr.channel_manager().list_channels();
    assert!(channels.is_empty());
}

#[tokio::test]
async fn tick_without_wallet_still_works() {
    let (mgr, _rx, _sd, _ld) = setup();

    // No wallet set — tick should not panic (wallet is None)
    for _ in 0..3 {
        mgr.tick().await;
    }
}

// ── close_channel tests ─────────────────────────────────────────────────

#[test]
fn close_channel_fails_for_nonexistent_channel() {
    let (mgr, _rx, _sd, _ld) = setup();

    let fake_channel_id = lightning::ln::types::ChannelId([0u8; 32]);
    let fake_counterparty = bitcoin::secp256k1::PublicKey::from_slice(
        &hex::decode("02eec7245d6b7d2ccb30380bfbe2a3648cd7a942653f5aa340edcea1f283686619")
            .unwrap(),
    )
    .unwrap();

    // Cooperative close of non-existent channel should fail
    let result = mgr.close_channel(fake_channel_id, fake_counterparty, false);
    assert!(result.is_err(), "closing nonexistent channel should fail");

    match result.unwrap_err() {
        wolfe_lightning::error::LightningError::Channel(msg) => {
            assert!(!msg.is_empty(), "error message should not be empty");
        }
        other => panic!("expected Channel error, got: {:?}", other),
    }
}

#[test]
fn force_close_fails_for_nonexistent_channel() {
    let (mgr, _rx, _sd, _ld) = setup();

    let fake_channel_id = lightning::ln::types::ChannelId([1u8; 32]);
    let fake_counterparty = bitcoin::secp256k1::PublicKey::from_slice(
        &hex::decode("02eec7245d6b7d2ccb30380bfbe2a3648cd7a942653f5aa340edcea1f283686619")
            .unwrap(),
    )
    .unwrap();

    // Force close of non-existent channel should also fail
    let result = mgr.close_channel(fake_channel_id, fake_counterparty, true);
    assert!(result.is_err(), "force-closing nonexistent channel should fail");
}

// ── open_channel + wallet wiring ────────────────────────────────────────

#[test]
fn open_channel_with_wallet_set_still_fails_for_unknown_peer() {
    let (mgr, _rx, _sd, _ld) = setup();
    let (wallet, _wd) = create_test_wallet();
    mgr.set_wallet(wallet);

    let random_key = bitcoin::secp256k1::PublicKey::from_slice(
        &hex::decode("02eec7245d6b7d2ccb30380bfbe2a3648cd7a942653f5aa340edcea1f283686619")
            .unwrap(),
    )
    .unwrap();

    // Should fail because peer isn't connected, not because wallet is missing
    let result = mgr.open_channel(random_key, 100_000, 0);
    assert!(result.is_err());
}

// ── Persistence with wallet ─────────────────────────────────────────────

#[test]
fn persist_state_works_with_wallet_set() {
    let (mgr, _rx, _sd, _ld) = setup();
    let (wallet, _wd) = create_test_wallet();
    mgr.set_wallet(wallet);

    // Should not panic
    mgr.persist_state();
    mgr.shutdown();
}

#[test]
fn shutdown_with_wallet_does_not_panic() {
    let (mgr, _rx, _sd, _ld) = setup();
    let (wallet, _wd) = create_test_wallet();
    mgr.set_wallet(wallet);

    mgr.shutdown();
}

// ── Wallet remains accessible after set ─────────────────────────────────

#[test]
fn wallet_address_generation_after_set() {
    let (mgr, _rx, _sd, _ld) = setup();
    let (wallet, _wd) = create_test_wallet();
    let wallet_clone = wallet.clone();
    mgr.set_wallet(wallet);

    // Wallet should still be usable via the Arc
    let mut w = wallet_clone.lock().unwrap();
    let addr = w.new_address().unwrap();
    assert!(addr.starts_with("bcrt1"));
}
