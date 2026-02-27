use bitcoin::transaction::{Transaction, TxIn, TxOut, Version};
use bitcoin::Amount;
use lightning::chain::chaininterface::BroadcasterInterface;
use tokio::sync::mpsc;

use wolfe_lightning::broadcaster::WolfeBroadcaster;

fn dummy_tx(version: i32) -> Transaction {
    Transaction {
        version: Version(version),
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn::default()],
        output: vec![TxOut {
            value: Amount::from_sat(50_000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        }],
    }
}

#[test]
fn broadcast_single_tx() {
    let (tx_sender, mut rx) = mpsc::unbounded_channel();
    let broadcaster = WolfeBroadcaster::new(tx_sender);

    let tx = dummy_tx(1);
    broadcaster.broadcast_transactions(&[&tx]);

    let received = rx.try_recv().unwrap();
    assert_eq!(received.compute_txid(), tx.compute_txid());
}

#[test]
fn broadcast_multiple_txs() {
    let (tx_sender, mut rx) = mpsc::unbounded_channel();
    let broadcaster = WolfeBroadcaster::new(tx_sender);

    let tx1 = dummy_tx(1);
    let tx2 = dummy_tx(2);
    broadcaster.broadcast_transactions(&[&tx1, &tx2]);

    let r1 = rx.try_recv().unwrap();
    let r2 = rx.try_recv().unwrap();
    assert_eq!(r1.compute_txid(), tx1.compute_txid());
    assert_eq!(r2.compute_txid(), tx2.compute_txid());
}

#[test]
fn broadcast_empty_slice_is_noop() {
    let (tx_sender, mut rx) = mpsc::unbounded_channel();
    let broadcaster = WolfeBroadcaster::new(tx_sender);

    broadcaster.broadcast_transactions(&[]);
    assert!(rx.try_recv().is_err());
}

#[test]
fn broadcast_after_receiver_dropped_does_not_panic() {
    let (tx_sender, rx) = mpsc::unbounded_channel();
    let broadcaster = WolfeBroadcaster::new(tx_sender);

    // Drop the receiver
    drop(rx);

    // This should not panic, just log a warning
    let tx = dummy_tx(1);
    broadcaster.broadcast_transactions(&[&tx]);
}
