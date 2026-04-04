//! Tests for NodeWallet: auto-generation, fund_channel, address derivation,
//! mnemonic recovery, and persistence.

use bdk_wallet::bitcoin::Network;
use tempfile::TempDir;
use wolfe_wallet::NodeWallet;

/// Create a fresh auto-generated wallet in a temp directory.
fn create_test_wallet() -> (NodeWallet, wolfe_wallet::Mnemonic, TempDir) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("wallet.sqlite3");
    let (wallet, mnemonic) = NodeWallet::create_new(&db_path, Network::Regtest).unwrap();
    (wallet, mnemonic, dir)
}

// ── create_new tests ────────────────────────────────────────────────────

#[test]
fn create_new_returns_wallet_and_mnemonic() {
    let (wallet, mnemonic, _dir) = create_test_wallet();

    // Mnemonic should be 12 words
    let mnemonic_str = mnemonic.to_string();
    let words: Vec<&str> = mnemonic_str.split_whitespace().collect();
    assert_eq!(
        words.len(),
        12,
        "expected 12-word mnemonic, got {}",
        words.len()
    );

    // Wallet should have zero balance initially
    let balance = wallet.balance();
    assert_eq!(balance.confirmed, 0);
    assert_eq!(balance.trusted_pending, 0);
}

#[test]
fn create_new_generates_unique_mnemonics() {
    let dir1 = TempDir::new().unwrap();
    let dir2 = TempDir::new().unwrap();
    let (_, m1) = NodeWallet::create_new(&dir1.path().join("w1.db"), Network::Regtest).unwrap();
    let (_, m2) = NodeWallet::create_new(&dir2.path().join("w2.db"), Network::Regtest).unwrap();

    assert_ne!(
        m1.to_string(),
        m2.to_string(),
        "two wallets should have different mnemonics"
    );
}

#[test]
fn create_new_works_for_all_networks() {
    for network in [
        Network::Bitcoin,
        Network::Testnet,
        Network::Signet,
        Network::Regtest,
    ] {
        let dir = TempDir::new().unwrap();
        let result = NodeWallet::create_new(&dir.path().join("wallet.db"), network);
        assert!(
            result.is_ok(),
            "create_new failed for {:?}: {:?}",
            network,
            result.err()
        );
    }
}

// ── Address generation ──────────────────────────────────────────────────

#[test]
fn new_address_returns_valid_bech32() {
    let (mut wallet, _, _dir) = create_test_wallet();

    let addr = wallet.new_address().unwrap();
    // Regtest bech32 addresses start with "bcrt1"
    assert!(
        addr.starts_with("bcrt1"),
        "expected bcrt1 prefix, got: {}",
        addr
    );
}

#[test]
fn new_address_increments_index() {
    let (mut wallet, _, _dir) = create_test_wallet();

    let addr1 = wallet.new_address().unwrap();
    let addr2 = wallet.new_address().unwrap();

    assert_ne!(addr1, addr2, "consecutive addresses should differ");
}

#[test]
fn change_address_differs_from_receive() {
    let (mut wallet, _, _dir) = create_test_wallet();

    let receive = wallet.new_address().unwrap();
    let change = wallet.new_change_address().unwrap();

    assert_ne!(
        receive, change,
        "receive and change addresses should differ"
    );
}

// ── Mnemonic recovery ───────────────────────────────────────────────────

#[test]
fn mnemonic_recovery_produces_same_addresses() {
    use bdk_wallet::keys::{DerivableKey, ExtendedKey};

    let dir1 = TempDir::new().unwrap();
    let (mut wallet1, mnemonic, _) = create_test_wallet_in(&dir1.path().join("w1.db"));

    let addr1 = wallet1.new_address().unwrap();

    // Recover from the same mnemonic
    let xkey: ExtendedKey = mnemonic.clone().into_extended_key().unwrap();
    let xprv = xkey.into_xprv(Network::Regtest).unwrap();
    let ext_desc = format!("wpkh({}/84h/1h/0h/0/*)", xprv);
    let int_desc = format!("wpkh({}/84h/1h/0h/1/*)", xprv);

    let dir2 = TempDir::new().unwrap();
    let mut wallet2 = NodeWallet::open(
        &dir2.path().join("w2.db"),
        Network::Regtest,
        ext_desc,
        int_desc,
    )
    .unwrap();

    let addr2 = wallet2.new_address().unwrap();

    assert_eq!(
        addr1, addr2,
        "recovered wallet should produce same first address"
    );
}

fn create_test_wallet_in(db_path: &std::path::Path) -> (NodeWallet, wolfe_wallet::Mnemonic, ()) {
    let (wallet, mnemonic) = NodeWallet::create_new(db_path, Network::Regtest).unwrap();
    (wallet, mnemonic, ())
}

// ── Persistence ─────────────────────────────────────────────────────────

#[test]
fn wallet_persists_across_reopen() {
    use bdk_wallet::keys::{DerivableKey, ExtendedKey};

    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("wallet.db");

    // Create and generate addresses
    let (ext_desc, int_desc);
    {
        let (mut wallet, mnemonic, _) = {
            let (w, m) = NodeWallet::create_new(&db_path, Network::Regtest).unwrap();
            (w, m, ())
        };

        // Derive descriptors for reopening
        let xkey: ExtendedKey = mnemonic.into_extended_key().unwrap();
        let xprv = xkey.into_xprv(Network::Regtest).unwrap();
        ext_desc = format!("wpkh({}/84h/1h/0h/0/*)", xprv);
        int_desc = format!("wpkh({}/84h/1h/0h/1/*)", xprv);

        // Generate some addresses (advances the index)
        let _a1 = wallet.new_address().unwrap();
        let _a2 = wallet.new_address().unwrap();
        // wallet dropped here, state persisted
    }

    // Reopen with same descriptors
    let mut wallet2 = NodeWallet::open(&db_path, Network::Regtest, ext_desc, int_desc).unwrap();

    // Next address should be index 2 (0 and 1 already revealed)
    let addr3 = wallet2.new_address().unwrap();
    assert!(
        addr3.starts_with("bcrt1"),
        "reopened wallet should still work"
    );
}

// ── fund_channel tests ──────────────────────────────────────────────────

#[test]
fn fund_channel_fails_with_no_funds() {
    let (mut wallet, _, _dir) = create_test_wallet();

    let script =
        bdk_wallet::bitcoin::ScriptBuf::new_p2wsh(&bdk_wallet::bitcoin::hashes::Hash::all_zeros());
    let fee_rate = bdk_wallet::bitcoin::FeeRate::from_sat_per_vb(2).unwrap();

    let result = wallet.fund_channel(script, 100_000, fee_rate);
    assert!(
        result.is_err(),
        "fund_channel should fail with empty wallet"
    );

    // Should be a BDK error about insufficient funds
    let err = result.unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("fund")
            || msg.contains("insufficient")
            || msg.contains("Insufficient")
            || msg.contains("build"),
        "expected funding/insufficient error, got: {}",
        msg
    );
}

#[test]
fn fund_channel_rejects_zero_amount() {
    let (mut wallet, _, _dir) = create_test_wallet();

    let script =
        bdk_wallet::bitcoin::ScriptBuf::new_p2wsh(&bdk_wallet::bitcoin::hashes::Hash::all_zeros());
    let fee_rate = bdk_wallet::bitcoin::FeeRate::from_sat_per_vb(1).unwrap();

    // BDK should reject a zero-value output
    let result = wallet.fund_channel(script, 0, fee_rate);
    assert!(result.is_err(), "zero-amount fund_channel should fail");
}

// ── Balance tests ───────────────────────────────────────────────────────

#[test]
fn fresh_wallet_has_zero_balance() {
    let (wallet, _, _dir) = create_test_wallet();

    let balance = wallet.balance();
    assert_eq!(balance.confirmed, 0);
    assert_eq!(balance.trusted_pending, 0);
    assert_eq!(balance.untrusted_pending, 0);
    assert_eq!(balance.immature, 0);
    assert_eq!(balance.total(), 0);
}

// ── Transaction listing ─────────────────────────────────────────────────

#[test]
fn fresh_wallet_has_no_transactions() {
    let (wallet, _, _dir) = create_test_wallet();
    assert!(wallet.list_transactions().is_empty());
}

// ── reset_chain tests ───────────────────────────────────────────────────

#[test]
fn reset_chain_succeeds() {
    let (mut wallet, _, _dir) = create_test_wallet();

    let result = wallet.reset_chain();
    assert!(
        result.is_ok(),
        "reset_chain should succeed on a fresh wallet: {:?}",
        result.err()
    );
}

#[test]
fn reset_chain_preserves_address_generation() {
    let (mut wallet, _, _dir) = create_test_wallet();

    // Generate an address before reset
    let addr_before = wallet.new_address().unwrap();
    assert!(
        addr_before.starts_with("bcrt1"),
        "pre-reset address should be valid regtest bech32"
    );

    // Reset chain state
    wallet.reset_chain().unwrap();

    // Generate another address after reset — keys should still work
    let addr_after = wallet.new_address().unwrap();
    assert!(
        addr_after.starts_with("bcrt1"),
        "post-reset address should be valid regtest bech32"
    );

    // The two addresses should differ because the index keeps advancing
    assert_ne!(
        addr_before, addr_after,
        "address generated after reset should differ (index preserved)"
    );
}

#[test]
fn reset_chain_resets_balance_to_zero() {
    let (mut wallet, _, _dir) = create_test_wallet();

    // Reset chain state
    wallet.reset_chain().unwrap();

    // Balance should be zero after reset
    let balance = wallet.balance();
    assert_eq!(
        balance.confirmed, 0,
        "confirmed balance should be 0 after reset"
    );
    assert_eq!(
        balance.trusted_pending, 0,
        "trusted_pending should be 0 after reset"
    );
    assert_eq!(
        balance.untrusted_pending, 0,
        "untrusted_pending should be 0 after reset"
    );
    assert_eq!(balance.immature, 0, "immature should be 0 after reset");
    assert_eq!(balance.total(), 0, "total balance should be 0 after reset");
}

#[test]
fn reset_chain_wallet_still_functional() {
    let (mut wallet, _, _dir) = create_test_wallet();

    // Generate some addresses before reset to advance state
    let _addr1 = wallet.new_address().unwrap();
    let _addr2 = wallet.new_address().unwrap();

    // Reset chain state
    wallet.reset_chain().unwrap();

    // Verify new_address still works
    let addr = wallet.new_address().unwrap();
    assert!(
        addr.starts_with("bcrt1"),
        "new_address should work after reset, got: {}",
        addr
    );

    // Verify balance still works
    let balance = wallet.balance();
    assert_eq!(
        balance.total(),
        0,
        "balance should work and be 0 after reset"
    );

    // Verify list_transactions still works
    let txs = wallet.list_transactions();
    assert!(
        txs.is_empty(),
        "list_transactions should return empty after reset"
    );
}
