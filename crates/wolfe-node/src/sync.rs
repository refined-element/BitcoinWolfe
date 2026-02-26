//! Header-first blockchain synchronization.
//!
//! Implements the standard Bitcoin sync strategy:
//! 1. Connect to peers
//! 2. Download and validate all block headers
//! 3. Download and validate full blocks
//! 4. Feed validated blocks to the wallet

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use bitcoin::block::Header;
use bitcoin::consensus::Encodable;
use bitcoin::hashes::Hash;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_blockdata::{GetHeadersMessage, Inventory};
use bitcoin::BlockHash;
use tracing::{debug, error, info, warn};

use wolfe_consensus::ConsensusEngine;
use wolfe_p2p::peer::PeerId;
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

/// Maximum number of blocks to request in a single getdata batch.
const BLOCK_DOWNLOAD_BATCH: usize = 16;

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
    /// Hash of our current tip (for chain continuity validation).
    tip_hash: BlockHash,
    /// Known genesis hash for the configured network.
    genesis_hash: BlockHash,
    /// Consensus engine for full block validation (optional — not available in header-only mode).
    consensus: Option<Arc<ConsensusEngine>>,
    /// Height of the last fully validated block.
    validated_height: u64,
    /// Queue of block hashes we've requested but not yet received.
    pending_blocks: VecDeque<BlockHash>,
    /// Txids from the most recently validated block (for mempool cleanup).
    last_confirmed_txids: Vec<bitcoin::Txid>,
    /// The most recently validated block and its height (for wallet feeding).
    last_validated_block: Option<(bitcoin::Block, u32)>,
}

impl SyncEngine {
    pub fn new(
        store: Arc<NodeStore>,
        network: bitcoin::Network,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        let genesis_hash = genesis_hash_for_network(network);

        // Check stored sync progress
        let (tip_height, tip_hash) = store
            .read_txn()
            .ok()
            .map(|txn| {
                let height = wolfe_store::MetaStore::sync_height(&txn)
                    .ok()
                    .flatten()
                    .unwrap_or(0);
                let hash = if height > 0 {
                    wolfe_store::HeaderStore::get_by_height(&txn, height)
                        .ok()
                        .flatten()
                        .map(|h| h.hash)
                        .unwrap_or(genesis_hash)
                } else {
                    genesis_hash
                };
                (height as u64, hash)
            })
            .unwrap_or((0, genesis_hash));

        info!(tip_height, %tip_hash, %genesis_hash, "sync engine initialized");

        Self {
            store,
            network,
            progress: SyncProgress::new(),
            shutdown,
            sync_peer: None,
            header_batch: Vec::new(),
            tip_height,
            tip_hash,
            genesis_hash,
            consensus: None,
            validated_height: 0,
            pending_blocks: VecDeque::new(),
            last_confirmed_txids: Vec::new(),
            last_validated_block: None,
        }
    }

    /// Attach a consensus engine for full block validation.
    pub fn set_consensus(&mut self, engine: Arc<ConsensusEngine>) {
        // Check if the consensus engine already has blocks validated
        let kernel_height = engine.chain_height();
        if kernel_height > 0 {
            self.validated_height = kernel_height as u64;
            info!(kernel_height, "consensus engine has existing chain state");
        }
        self.consensus = Some(engine);
    }

    pub fn progress(&self) -> &SyncProgress {
        &self.progress
    }

    pub fn tip_height(&self) -> u64 {
        self.tip_height
    }

    pub fn tip_hash(&self) -> BlockHash {
        self.tip_hash
    }

    pub fn validated_height(&self) -> u64 {
        self.validated_height
    }

    /// Returns txids from the last validated block (for mempool cleanup).
    pub fn take_confirmed_txids(&mut self) -> Vec<bitcoin::Txid> {
        std::mem::take(&mut self.last_confirmed_txids)
    }

    /// Returns the last validated block and its height (for wallet feeding).
    pub fn take_validated_block(&mut self) -> Option<(bitcoin::Block, u32)> {
        self.last_validated_block.take()
    }

    /// Handle a P2P message from a peer. Returns an optional response message.
    pub fn handle_message(
        &mut self,
        peer_id: PeerId,
        msg: NetworkMessage,
    ) -> Option<(PeerId, NetworkMessage)> {
        match msg {
            NetworkMessage::Headers(headers) => self.handle_headers(peer_id, headers),
            NetworkMessage::Inv(inv) => self.handle_inv(peer_id, inv),
            NetworkMessage::Block(block) => self.handle_block(peer_id, block),
            NetworkMessage::SendHeaders => {
                // Peer prefers headers announcements — we always do.
                debug!(peer = ?peer_id, "peer prefers headers announcements");
                None
            }
            _ => None,
        }
    }

    /// Called when a new peer connects. If we need a sync peer, claim this one.
    pub fn on_peer_connected(
        &mut self,
        peer_id: PeerId,
        start_height: i32,
    ) -> Option<NetworkMessage> {
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
                info!(height = self.tip_height, "header sync complete");
                self.progress
                    .headers_height
                    .store(self.tip_height, Ordering::Relaxed);

                // Start block download if consensus engine is available
                if self.consensus.is_some() && self.validated_height < self.tip_height {
                    self.progress.state = SyncState::SyncingBlocks;
                    return self.request_next_blocks(peer_id);
                }

                self.progress.state = SyncState::Synced;
            }
            return None;
        }

        let count = headers.len();

        // Phase 1: Validate all headers (chain continuity + PoW)
        let mut validated: Vec<(Header, u32)> = Vec::with_capacity(count);
        let mut next_height = self.tip_height + 1;
        let mut prev_hash = self.tip_hash;

        for header in &headers {
            // Validate: prev_block_hash must chain from our current tip
            if header.prev_blockhash != prev_hash {
                warn!(
                    height = next_height,
                    expected = %prev_hash,
                    got = %header.prev_blockhash,
                    "header does not chain from our tip — disconnecting sync peer"
                );
                self.sync_peer = None;
                self.progress.state = SyncState::WaitingForPeers;
                return None;
            }

            // Validate: proof of work — the block hash must be <= target
            let target = header.target();
            let block_hash = header.block_hash();
            if header.validate_pow(target).is_err() {
                warn!(
                    height = next_height,
                    hash = %block_hash,
                    "header fails proof-of-work check — banning peer"
                );
                self.sync_peer = None;
                self.progress.state = SyncState::WaitingForPeers;
                return None;
            }

            validated.push((*header, next_height as u32));
            prev_hash = block_hash;
            next_height += 1;
        }

        // Phase 2: Batch-store all validated headers in a single transaction
        let stored = validated.len() as u64;
        if let Err(e) = self.store.insert_headers_batch(&validated) {
            warn!(?e, "failed to store header batch");
            return None;
        }

        // Update in-memory state
        self.tip_height = next_height - 1;
        self.tip_hash = prev_hash;

        self.progress
            .headers_height
            .store(self.tip_height, Ordering::Relaxed);

        if stored > 0 {
            // Log progress periodically
            if self.tip_height % 10_000 == 0 || count < 2000 {
                info!(height = self.tip_height, batch = stored, "syncing headers");
            } else if self.tip_height % 1_000 == 0 {
                debug!(height = self.tip_height, batch = stored, "syncing headers");
            }
        }

        // If we got a full batch (2000 headers), request more.
        if count >= 2000 && self.sync_peer == Some(peer_id) {
            let next_request = self.build_getheaders();
            return Some((peer_id, next_request));
        }

        // Less than 2000 means we've caught up with headers.
        if self.sync_peer == Some(peer_id) {
            info!(height = self.tip_height, "header sync complete");

            // If we have a consensus engine and blocks to download, start block sync
            if self.consensus.is_some() && self.validated_height < self.tip_height {
                self.progress.state = SyncState::SyncingBlocks;
                info!(
                    from = self.validated_height + 1,
                    to = self.tip_height,
                    "starting block download"
                );
                return self.request_next_blocks(peer_id);
            }

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

    /// Handle a received full block — validate via consensus engine.
    fn handle_block(
        &mut self,
        peer_id: PeerId,
        block: bitcoin::Block,
    ) -> Option<(PeerId, NetworkMessage)> {
        let hash = block.block_hash();
        let txcount = block.txdata.len();

        // Remove from pending queue
        self.pending_blocks.retain(|h| *h != hash);

        // Serialize the block for the consensus engine
        let consensus_engine = match &self.consensus {
            Some(engine) => engine.clone(),
            None => {
                debug!(%hash, txcount, "received block (no consensus engine)");
                return None;
            }
        };

        let mut block_bytes = Vec::new();
        if let Err(e) = block.consensus_encode(&mut block_bytes) {
            warn!(%hash, ?e, "failed to serialize block");
            return None;
        }

        // Validate through libbitcoinkernel
        match consensus_engine.validate_block(&block_bytes) {
            Ok(wolfe_consensus::ProcessBlockResult::NewBlock) => {
                self.validated_height += 1;
                self.progress
                    .blocks_height
                    .store(self.validated_height, Ordering::Relaxed);

                // Collect txids for mempool cleanup
                self.last_confirmed_txids =
                    block.txdata.iter().map(|tx| tx.compute_txid()).collect();

                // Store block for wallet consumption
                self.last_validated_block = Some((block, self.validated_height as u32));

                if self.validated_height % 1_000 == 0 {
                    info!(
                        height = self.validated_height,
                        headers = self.tip_height,
                        txcount,
                        "block validated"
                    );
                } else {
                    debug!(height = self.validated_height, %hash, txcount, "block validated");
                }
            }
            Ok(wolfe_consensus::ProcessBlockResult::Duplicate) => {
                debug!(%hash, "block already known to consensus engine");
                self.validated_height += 1;
            }
            Ok(wolfe_consensus::ProcessBlockResult::Rejected) => {
                error!(%hash, "block REJECTED by consensus engine");
                // Don't advance validated_height — this is a serious error
                return None;
            }
            Err(e) => {
                error!(%hash, ?e, "consensus engine error processing block");
                return None;
            }
        }

        // Request more blocks if we're still catching up
        if self.validated_height < self.tip_height && self.pending_blocks.is_empty() {
            return self.request_next_blocks(peer_id);
        }

        // Check if we're fully synced
        if self.validated_height >= self.tip_height {
            info!(
                height = self.validated_height,
                "block sync complete — fully validated"
            );
            self.progress.state = SyncState::Synced;
        }

        None
    }

    /// Build a getdata message for the next batch of blocks to download.
    fn request_next_blocks(&mut self, peer_id: PeerId) -> Option<(PeerId, NetworkMessage)> {
        let start = self.validated_height + 1;
        let end = std::cmp::min(start + BLOCK_DOWNLOAD_BATCH as u64, self.tip_height + 1);

        if start > self.tip_height {
            return None;
        }

        let read_txn = match self.store.read_txn() {
            Ok(txn) => txn,
            Err(e) => {
                warn!(?e, "failed to open read txn for block request");
                return None;
            }
        };

        let mut inventory = Vec::new();
        for height in start..end {
            match wolfe_store::HeaderStore::get_by_height(&read_txn, height as u32) {
                Ok(Some(stored)) => {
                    inventory.push(Inventory::Block(stored.hash));
                    self.pending_blocks.push_back(stored.hash);
                }
                _ => {
                    warn!(height, "missing header for block request");
                    break;
                }
            }
        }

        if inventory.is_empty() {
            return None;
        }

        debug!(
            from = start,
            to = end - 1,
            count = inventory.len(),
            "requesting blocks"
        );

        Some((peer_id, NetworkMessage::GetData(inventory)))
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
