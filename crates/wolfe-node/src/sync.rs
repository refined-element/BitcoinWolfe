//! Header-first blockchain synchronization.
//!
//! Implements the standard Bitcoin sync strategy:
//! 1. Connect to peers
//! 2. Download and validate all block headers
//! 3. Download and validate full blocks
//! 4. Feed validated blocks to the wallet

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use bitcoin::block::Header;
use bitcoin::hashes::Hash;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::{GetHeadersMessage, Inventory};
use bitcoin::BlockHash;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use wolfe_p2p::peer::PeerId;
use wolfe_p2p::PeerManager;
use wolfe_store::NodeStore;

/// Tracks the current sync state of the node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// Waiting for peer connections.
    WaitingForPeers,
    /// Downloading and validating block headers.
    SyncingHeaders,
    /// Downloading and validating full blocks.
    SyncingBlocks,
    /// Fully synchronized with the network.
    Synced,
}

impl std::fmt::Display for SyncState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncState::WaitingForPeers => write!(f, "waiting_for_peers"),
            SyncState::SyncingHeaders => write!(f, "syncing_headers"),
            SyncState::SyncingBlocks => write!(f, "syncing_blocks"),
            SyncState::Synced => write!(f, "synced"),
        }
    }
}

/// Progress information for the sync process.
pub struct SyncProgress {
    pub state: SyncState,
    pub headers_height: Arc<AtomicU64>,
    pub blocks_height: Arc<AtomicU64>,
    pub peer_count: Arc<AtomicU64>,
    pub headers_per_second: Arc<AtomicU64>,
}

impl SyncProgress {
    pub fn new() -> Self {
        Self {
            state: SyncState::WaitingForPeers,
            headers_height: Arc::new(AtomicU64::new(0)),
            blocks_height: Arc::new(AtomicU64::new(0)),
            peer_count: Arc::new(AtomicU64::new(0)),
            headers_per_second: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// The sync engine orchestrates header and block download from peers.
pub struct SyncEngine {
    store: Arc<NodeStore>,
    network: bitcoin::Network,
    progress: SyncProgress,
    shutdown: Arc<AtomicBool>,
    /// The peer we're currently syncing headers from.
    sync_peer: Option<PeerId>,
    /// Headers we've received but haven't stored yet (batch buffer).
    header_batch: Vec<(Header, u32)>,
    /// Our current best header height.
    tip_height: u64,
    /// Known genesis hash for the configured network.
    genesis_hash: BlockHash,
}

impl SyncEngine {
    pub fn new(
        store: Arc<NodeStore>,
        network: bitcoin::Network,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        let genesis_hash = genesis_hash_for_network(network);

        // Check stored sync progress
        let tip_height = store
            .read_txn()
            .ok()
            .and_then(|txn| wolfe_store::MetaStore::sync_height(&txn).ok().flatten())
            .map(|h| h as u64)
            .unwrap_or(0);

        info!(tip_height, %genesis_hash, "sync engine initialized");

        Self {
            store,
            network,
            progress: SyncProgress::new(),
            shutdown,
            sync_peer: None,
            header_batch: Vec::new(),
            tip_height,
            genesis_hash,
        }
    }

    pub fn progress(&self) -> &SyncProgress {
        &self.progress
    }

    pub fn tip_height(&self) -> u64 {
        self.tip_height
    }

    /// Handle a P2P message from a peer. Returns an optional response message.
    pub fn handle_message(
        &mut self,
        peer_id: PeerId,
        msg: NetworkMessage,
    ) -> Option<(PeerId, NetworkMessage)> {
        match msg {
            NetworkMessage::Headers(headers) => {
                self.handle_headers(peer_id, headers)
            }
            NetworkMessage::Inv(inv) => {
                self.handle_inv(peer_id, inv)
            }
            NetworkMessage::Block(block) => {
                self.handle_block(peer_id, block);
                None
            }
            NetworkMessage::SendHeaders => {
                // Peer prefers headers announcements — we always do.
                debug!(peer = ?peer_id, "peer prefers headers announcements");
                None
            }
            _ => None,
        }
    }

    /// Called when a new peer connects. If we need a sync peer, claim this one.
    pub fn on_peer_connected(&mut self, peer_id: PeerId, start_height: i32) -> Option<NetworkMessage> {
        self.progress.peer_count.fetch_add(1, Ordering::Relaxed);

        // If we don't have a sync peer and this peer has more headers than us, use them.
        if self.sync_peer.is_none() && (start_height as u64) > self.tip_height {
            info!(
                peer = ?peer_id,
                their_height = start_height,
                our_height = self.tip_height,
                "selected sync peer"
            );
            self.sync_peer = Some(peer_id);
            self.progress.state = SyncState::SyncingHeaders;

            // Build getheaders request starting from our tip
            return Some(self.build_getheaders());
        }

        None
    }

    /// Called when a peer disconnects.
    pub fn on_peer_disconnected(&mut self, peer_id: PeerId) {
        self.progress.peer_count.fetch_sub(1, Ordering::Relaxed);

        if self.sync_peer == Some(peer_id) {
            warn!(peer = ?peer_id, "sync peer disconnected");
            self.sync_peer = None;
            // Will pick a new sync peer on next connection
        }
    }

    /// Handle received block headers.
    fn handle_headers(
        &mut self,
        peer_id: PeerId,
        headers: Vec<Header>,
    ) -> Option<(PeerId, NetworkMessage)> {
        if headers.is_empty() {
            // Empty headers response means we're caught up with this peer.
            if self.sync_peer == Some(peer_id) {
                info!(
                    height = self.tip_height,
                    "header sync complete"
                );
                self.progress.state = SyncState::Synced;
                self.progress
                    .headers_height
                    .store(self.tip_height, Ordering::Relaxed);
            }
            return None;
        }

        let count = headers.len();
        let mut stored = 0u64;

        for header in &headers {
            let height = self.tip_height + 1;

            // Store the header
            match self.store.insert_header_and_update_sync(header, height as u32) {
                Ok(()) => {
                    self.tip_height = height;
                    stored += 1;
                }
                Err(e) => {
                    warn!(
                        height,
                        hash = %header.block_hash(),
                        ?e,
                        "failed to store header"
                    );
                    break;
                }
            }
        }

        self.progress
            .headers_height
            .store(self.tip_height, Ordering::Relaxed);

        if stored > 0 {
            // Log progress periodically
            if self.tip_height % 10_000 == 0 || count < 2000 {
                info!(
                    height = self.tip_height,
                    batch = stored,
                    "syncing headers"
                );
            } else if self.tip_height % 1_000 == 0 {
                debug!(
                    height = self.tip_height,
                    batch = stored,
                    "syncing headers"
                );
            }
        }

        // If we got a full batch (2000 headers), request more.
        if count >= 2000 && self.sync_peer == Some(peer_id) {
            let next_request = self.build_getheaders();
            return Some((peer_id, next_request));
        }

        // Less than 2000 means we've caught up.
        if self.sync_peer == Some(peer_id) {
            info!(height = self.tip_height, "header sync complete");
            self.progress.state = SyncState::Synced;
        }

        None
    }

    /// Handle inventory announcements.
    fn handle_inv(
        &mut self,
        _peer_id: PeerId,
        inventory: Vec<Inventory>,
    ) -> Option<(PeerId, NetworkMessage)> {
        for inv in &inventory {
            match inv {
                Inventory::Block(hash) => {
                    debug!(%hash, "new block announced");
                }
                Inventory::CompactBlock(hash) => {
                    debug!(%hash, "compact block announced");
                }
                _ => {}
            }
        }
        None
    }

    /// Handle a received full block.
    fn handle_block(&mut self, _peer_id: PeerId, block: bitcoin::Block) {
        let hash = block.block_hash();
        let txcount = block.txdata.len();
        debug!(%hash, txcount, "received block");

        // TODO: Validate block with consensus engine
        // TODO: Feed to wallet via apply_block
        // TODO: Remove confirmed txs from mempool
    }

    /// Build a getheaders message starting from our current tip.
    fn build_getheaders(&self) -> NetworkMessage {
        // Build locator: our tip hash, then genesis as stop hash (means "send everything")
        let locator = self.build_locator();

        NetworkMessage::GetHeaders(GetHeadersMessage {
            version: 70016,
            locator_hashes: locator,
            stop_hash: BlockHash::all_zeros(),
        })
    }

    /// Build a block locator (list of known block hashes) for getheaders.
    /// Uses exponential backoff: tip, tip-1, tip-2, tip-4, tip-8, ..., genesis.
    fn build_locator(&self) -> Vec<BlockHash> {
        let mut locator = Vec::new();
        let mut height = self.tip_height;
        let mut step = 1u64;

        let read_txn = match self.store.read_txn() {
            Ok(txn) => txn,
            Err(_) => return vec![self.genesis_hash],
        };

        // Walk back through our chain with exponential steps
        loop {
            match wolfe_store::HeaderStore::get_by_height(&read_txn, height as u32) {
                Ok(Some(stored)) => {
                    locator.push(stored.hash);
                }
                _ => {}
            }

            if height == 0 {
                break;
            }

            // Exponential backoff after first 10 entries
            if locator.len() >= 10 {
                step *= 2;
            }

            if height < step {
                height = 0;
            } else {
                height -= step;
            }
        }

        // Always include genesis
        if locator.last() != Some(&self.genesis_hash) {
            locator.push(self.genesis_hash);
        }

        locator
    }
}

/// Get the genesis block hash for a network.
fn genesis_hash_for_network(network: bitcoin::Network) -> BlockHash {
    use bitcoin::constants::genesis_block;
    genesis_block(network.params()).block_hash()
}
