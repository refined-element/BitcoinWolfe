use bitcoin::Transaction;
use lightning::chain::chaininterface::BroadcasterInterface;
use tokio::sync::mpsc;

/// Bridges LDK's BroadcasterInterface to an mpsc channel.
///
/// LDK calls `broadcast_transactions()` synchronously, but our P2P layer is async.
/// We decouple via an unbounded channel. The main event loop drains the receiver
/// and broadcasts via `PeerManager::broadcast(NetworkMessage::Tx(tx))`.
pub struct WolfeBroadcaster {
    tx_sender: mpsc::UnboundedSender<Transaction>,
}

impl WolfeBroadcaster {
    pub fn new(tx_sender: mpsc::UnboundedSender<Transaction>) -> Self {
        Self { tx_sender }
    }
}

impl BroadcasterInterface for WolfeBroadcaster {
    fn broadcast_transactions(&self, txs: &[&Transaction]) {
        for tx in txs {
            if let Err(e) = self.tx_sender.send((*tx).clone()) {
                tracing::warn!("failed to queue tx for broadcast: {}", e);
            }
        }
    }
}
