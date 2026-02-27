//! Header-first blockchain synchronization.
//!
//! Implements the standard Bitcoin sync strategy:
//! 1. Connect to peers
//! 2. Download and validate all block headers
//! 3. Download and validate full blocks
//! 4. Feed validated blocks to the wallet

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

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
/// Bitcoin Core uses 1024 in-flight; we use 128 for a good balance of throughput.
const BLOCK_DOWNLOAD_BATCH: usize = 128;

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
    /// Set of txids we've already seen or requested (dedup filter).
    known_txids: HashSet<bitcoin::Txid>,
    /// Transactions received from peers, pending relay to mempool.
    pending_txs: Vec<bitcoin::Transaction>,
    /// Peer heights reported during handshake (for sync peer selection).
    peer_heights: HashMap<PeerId, u64>,
    /// Timestamp of the last block received from a peer. Used to detect
    /// stalls where the peer stops responding to getdata requests.
    last_block_time: Instant,
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
            known_txids: HashSet::new(),
            pending_txs: Vec::new(),
            peer_heights: HashMap::new(),
            last_block_time: Instant::now(),
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

    /// Take pending transactions received from peers (for mempool insertion).
    pub fn take_pending_txs(&mut self) -> Vec<bitcoin::Transaction> {
        std::mem::take(&mut self.pending_txs)
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
            NetworkMessage::Tx(tx) => {
                let txid = tx.compute_txid();
                debug!(%txid, "received tx from peer");
                self.known_txids.insert(txid);
                self.pending_txs.push(tx);
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
    pub fn on_peer_connected(
        &mut self,
        peer_id: PeerId,
        start_height: i32,
    ) -> Option<NetworkMessage> {
        self.progress.peer_count.fetch_add(1, Ordering::Relaxed);

        // Sanitize peer-reported height: clamp negatives to 0.
        let peer_height = if start_height < 0 {
            warn!(peer = ?peer_id, start_height, "peer reported negative start_height — treating as 0");
            0u64
        } else {
            start_height as u64
        };

        // Reject truly absurd heights (>2M blocks, ~38 years at 10min/block).
        // We don't restrict based on our own tip because during initial sync
        // we may be hundreds of thousands of blocks behind.
        if peer_height > 2_000_000 {
            warn!(
                peer = ?peer_id,
                peer_height,
                "peer reported impossibly high start_height — ignoring as sync candidate"
            );
            return None;
        }

        // Track peer as a candidate for syncing
        self.peer_heights.insert(peer_id, peer_height);

        // If we don't have a sync peer and this peer has more headers than us, use them.
        if self.sync_peer.is_none() && peer_height > self.tip_height {
            info!(
                peer = ?peer_id,
                their_height = peer_height,
                our_height = self.tip_height,
                "selected sync peer"
            );
            self.sync_peer = Some(peer_id);
            self.progress.state = SyncState::SyncingHeaders;

            // Build getheaders request starting from our tip
            return Some(self.build_getheaders());
        }

        // Headers already synced but blocks still needed — start block download.
        // This happens on restart when headers were fully synced in a previous session
        // but block validation hasn't caught up yet. We accept any peer ahead of our
        // validated height (not tip_height, which is the header frontier).
        if self.sync_peer.is_none()
            && peer_height > self.validated_height
            && self.consensus.is_some()
            && self.validated_height < self.tip_height
        {
            info!(
                peer = ?peer_id,
                validated = self.validated_height,
                tip = self.tip_height,
                "headers synced, resuming block download"
            );
            self.sync_peer = Some(peer_id);
            self.progress.state = SyncState::SyncingBlocks;
            return self
                .request_next_blocks(peer_id)
                .map(|(_pid, msg)| msg);
        }

        None
    }

    /// Called when a peer disconnects.
    pub fn on_peer_disconnected(&mut self, peer_id: PeerId) {
        self.progress.peer_count.fetch_sub(1, Ordering::Relaxed);
        self.peer_heights.remove(&peer_id);

        if self.sync_peer == Some(peer_id) {
            warn!(peer = ?peer_id, "sync peer disconnected — selecting replacement");
            self.sync_peer = None;
            // Will be re-selected by try_select_sync_peer() from the caller
        }
    }

    /// Try to select a new sync peer from the connected peer pool.
    /// Returns the peer_id and a getheaders message if a suitable peer was found.
    pub fn try_select_sync_peer(&mut self) -> Option<(PeerId, NetworkMessage)> {
        if self.sync_peer.is_some() {
            return None;
        }

        // Pick the peer with the highest reported height that's ahead of us (for headers)
        let best_candidate = self
            .peer_heights
            .iter()
            .filter(|(_, &height)| height > self.tip_height)
            .max_by_key(|(_, &height)| height)
            .map(|(&peer_id, &height)| (peer_id, height));

        if let Some((peer_id, height)) = best_candidate {
            info!(
                peer = ?peer_id,
                their_height = height,
                our_height = self.tip_height,
                "selected replacement sync peer"
            );
            self.sync_peer = Some(peer_id);
            self.progress.state = SyncState::SyncingHeaders;
            return Some((peer_id, self.build_getheaders()));
        }

        // Headers are synced but blocks still needed — pick any peer ahead of
        // our validated block height. The peer's reported height (from version
        // handshake) may be slightly behind our header tip due to new blocks
        // mined since they connected, so we compare against validated_height
        // which is where we actually need blocks from.
        if self.consensus.is_some() && self.validated_height < self.tip_height {
            let block_candidate = self
                .peer_heights
                .iter()
                .filter(|(_, &height)| height > self.validated_height)
                .max_by_key(|(_, &height)| height)
                .map(|(&peer_id, _)| peer_id);

            if let Some(peer_id) = block_candidate {
                info!(
                    peer = ?peer_id,
                    validated = self.validated_height,
                    tip = self.tip_height,
                    "selected replacement peer for block download"
                );
                self.sync_peer = Some(peer_id);
                self.progress.state = SyncState::SyncingBlocks;
                return self.request_next_blocks(peer_id);
            }
        }

        None
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
        self.last_block_time = Instant::now();

        // Phase 1: Validate all headers (chain continuity + PoW)
        let mut validated: Vec<(Header, u32)> = Vec::with_capacity(count);
        let mut next_height = self.tip_height + 1;
        let mut prev_hash = self.tip_hash;

        for header in &headers {
            // Validate: prev_block_hash must chain from our current tip
            if header.prev_blockhash != prev_hash {
                // Check if this is a reorg: try to find the fork point in our chain
                let fork_height = self.find_fork_point(header.prev_blockhash);
                if let Some(fork_h) = fork_height {
                    warn!(
                        fork_height = fork_h,
                        our_tip = self.tip_height,
                        "chain reorganization detected at height {}",
                        fork_h
                    );
                    // Rewind to fork point
                    if let Err(e) = self.store.reorganize(
                        self.tip_height as u32,
                        fork_h as u32,
                        &[],
                    ) {
                        error!(?e, "failed to execute reorg in store");
                        self.sync_peer = None;
                        self.progress.state = SyncState::WaitingForPeers;
                        return None;
                    }

                    // Update in-memory state to fork point
                    self.tip_height = fork_h as u64;
                    self.tip_hash = header.prev_blockhash;
                    if self.validated_height > self.tip_height {
                        self.validated_height = self.tip_height;
                    }
                    // Reset locals to match rewound state
                    prev_hash = header.prev_blockhash;
                    next_height = self.tip_height + 1;
                    validated.clear();
                    // Now re-validate this header (it chains from the fork point)
                } else {
                    warn!(
                        height = next_height,
                        expected = %prev_hash,
                        got = %header.prev_blockhash,
                        "header does not chain from our tip and no fork point found — disconnecting sync peer"
                    );
                    self.sync_peer = None;
                    self.progress.state = SyncState::WaitingForPeers;
                    return None;
                }
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
        peer_id: PeerId,
        inventory: Vec<Inventory>,
    ) -> Option<(PeerId, NetworkMessage)> {
        let mut tx_requests = Vec::new();
        for inv in &inventory {
            match inv {
                Inventory::Block(hash) => {
                    debug!(%hash, "new block announced");
                }
                Inventory::CompactBlock(hash) => {
                    debug!(%hash, "compact block announced");
                }
                Inventory::Transaction(txid) => {
                    if !self.known_txids.contains(txid) {
                        tx_requests.push(Inventory::WitnessTransaction(*txid));
                        self.known_txids.insert(*txid);
                    }
                }
                Inventory::WTx(wtxid) => {
                    // Convert wtxid to txid for dedup (best effort — they may differ for segwit)
                    let txid = bitcoin::Txid::from_raw_hash(
                        bitcoin::hashes::sha256d::Hash::from_byte_array(wtxid.to_byte_array()),
                    );
                    if !self.known_txids.contains(&txid) {
                        tx_requests.push(Inventory::WitnessTransaction(txid));
                        self.known_txids.insert(txid);
                    }
                }
                _ => {}
            }
        }

        if !tx_requests.is_empty() {
            debug!(count = tx_requests.len(), "requesting announced transactions");
            return Some((peer_id, NetworkMessage::GetData(tx_requests)));
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

        // Only process blocks we actually requested. After a rejection we clear
        // pending_blocks and re-request, but old in-flight blocks from the peer
        // keep arriving. Submitting those stale blocks creates a rejection cascade.
        let was_pending = self.pending_blocks.contains(&hash);
        self.pending_blocks.retain(|h| *h != hash);
        if !was_pending {
            debug!(%hash, "ignoring unrequested block");
            return None;
        }

        self.last_block_time = Instant::now();

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

        // Validate through libbitcoinkernel.
        //
        // We always advance validated_height (our "submitted" cursor) by 1
        // regardless of the result. This keeps the download pipeline moving
        // forward through our header chain. The kernel's active chain height
        // (chain_height) may lag behind — that's expected during IBD. We
        // report chain_height for progress and only use it to limit pipeline
        // depth so we don't race too far ahead.
        match consensus_engine.validate_block(&block_bytes) {
            Ok(wolfe_consensus::ProcessBlockResult::NewBlock) => {
                self.validated_height += 1;

                // Collect txids for mempool cleanup
                self.last_confirmed_txids =
                    block.txdata.iter().map(|tx| tx.compute_txid()).collect();

                // Store block for wallet consumption
                self.last_validated_block = Some((block, self.validated_height as u32));

                if self.validated_height % 1_000 == 0 {
                    let ch = consensus_engine.chain_height() as u64;
                    info!(
                        height = self.validated_height,
                        chain_height = ch,
                        headers = self.tip_height,
                        txcount,
                        "block validated"
                    );
                } else {
                    debug!(height = self.validated_height, %hash, txcount, "block validated");
                }
            }
            Ok(wolfe_consensus::ProcessBlockResult::Duplicate) => {
                self.validated_height += 1;
                debug!(%hash, height = self.validated_height, "block already known — skipping");
            }
            Ok(wolfe_consensus::ProcessBlockResult::Rejected) => {
                let ch = consensus_engine.chain_height() as u64;
                let prev = block.header.prev_blockhash;
                // Check if the kernel knows the parent block
                let parent_known = {
                    let prev_bytes: [u8; 32] = *prev.as_ref();
                    consensus_engine.get_block_by_hash(&prev_bytes).is_some()
                };
                // Compare kernel's block at chain_height with our header store
                let kernel_tip_hash = consensus_engine
                    .get_block_at_height(ch as u32)
                    .map(|b| b.hash_hex)
                    .unwrap_or_default();
                let store_hash = self.store.read_txn().ok().and_then(|txn| {
                    wolfe_store::HeaderStore::get_by_height(&txn, ch as u32)
                        .ok()
                        .flatten()
                        .map(|h| h.hash.to_string())
                }).unwrap_or_default();

                self.validated_height += 1;
                warn!(
                    %hash,
                    height = self.validated_height,
                    chain_height = ch,
                    %prev,
                    parent_known,
                    kernel_tip_hash,
                    store_hash,
                    "block rejected"
                );

                // On first rejection in a batch: find the divergence point
                // between kernel and header store, re-sync headers from
                // the common ancestor. We skip the expensive store.reorganize()
                // because insert_headers_batch() overwrites HEIGHT_TO_HASH
                // entries — the old wrong headers are simply replaced as new
                // correct headers arrive from the peer.
                if !self.pending_blocks.is_empty() {
                    self.pending_blocks.clear();

                    // Find the highest height where kernel and header store agree
                    let common = self.find_kernel_store_common_ancestor(&consensus_engine, ch);
                    warn!(
                        chain_height = ch,
                        common_ancestor = common,
                        "header store diverges from kernel — re-syncing headers"
                    );

                    // Reset state to the common ancestor (no store reorg needed —
                    // new headers from getheaders will overwrite the wrong ones).
                    self.validated_height = common;
                    if let Ok(txn) = self.store.read_txn() {
                        if let Ok(Some(stored)) = wolfe_store::HeaderStore::get_by_height(
                            &txn, common as u32,
                        ) {
                            self.tip_height = common;
                            self.tip_hash = stored.hash;
                        }
                    }

                    self.progress.state = SyncState::SyncingHeaders;
                    self.progress
                        .headers_height
                        .store(self.tip_height, Ordering::Relaxed);
                    self.last_block_time = Instant::now();

                    // Send getheaders from the common ancestor
                    if let Some(peer) = self.sync_peer {
                        let msg = self.build_getheaders();
                        return Some((peer, msg));
                    }
                }
            }
            Err(e) => {
                error!(%hash, ?e, "consensus engine error processing block");
                return None;
            }
        }

        // Update progress with the kernel's actual chain height
        let ch = consensus_engine.chain_height() as u64;
        self.progress.blocks_height.store(ch, Ordering::Relaxed);

        // Request more blocks if we're still catching up
        if self.validated_height < self.tip_height && self.pending_blocks.is_empty() {
            return self.request_next_blocks(peer_id);
        }

        // Check if we're fully synced
        if self.validated_height >= self.tip_height && ch >= self.tip_height {
            info!(
                height = ch,
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

    /// Check for stalled downloads. If pending blocks or header requests
    /// haven't been received within 60 seconds, retry them.
    /// Returns a message for the sync peer if re-requesting.
    pub fn check_stall(&mut self) -> Option<(PeerId, NetworkMessage)> {
        let elapsed = self.last_block_time.elapsed();
        if elapsed < std::time::Duration::from_secs(60) {
            return None;
        }

        let peer_id = self.sync_peer?;

        match self.progress.state {
            SyncState::SyncingBlocks if !self.pending_blocks.is_empty() => {
                let stuck_count = self.pending_blocks.len();
                let ch = self.consensus.as_ref().map(|e| e.chain_height() as u64).unwrap_or(0);

                warn!(
                    stuck_count,
                    elapsed_secs = elapsed.as_secs(),
                    validated_height = self.validated_height,
                    chain_height = ch,
                    "block download stalled — re-requesting"
                );

                // Clear and re-request the same blocks (don't advance validated_height).
                self.pending_blocks.clear();
                self.last_block_time = Instant::now();
                self.request_next_blocks(peer_id)
            }
            SyncState::SyncingHeaders => {
                warn!(
                    elapsed_secs = elapsed.as_secs(),
                    tip_height = self.tip_height,
                    "header sync stalled — re-sending getheaders"
                );
                self.last_block_time = Instant::now();
                let msg = self.build_getheaders();
                Some((peer_id, msg))
            }
            _ => None,
        }
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

    /// Find the highest height where the kernel's active chain and our header
    /// store agree on the block hash. Searches backwards from `start_height`.
    fn find_kernel_store_common_ancestor(
        &self,
        engine: &ConsensusEngine,
        start_height: u64,
    ) -> u64 {
        let read_txn = match self.store.read_txn() {
            Ok(txn) => txn,
            Err(_) => return 0,
        };

        for h in (0..=start_height).rev() {
            let kernel_block = engine.get_block_at_height(h as u32);
            let store_block =
                wolfe_store::HeaderStore::get_by_height(&read_txn, h as u32).ok().flatten();

            match (kernel_block, store_block) {
                (Some(kb), Some(sb)) => {
                    if kb.hash_hex == sb.hash.to_string() {
                        return h;
                    }
                }
                _ => {}
            }

            // Don't search more than 1000 blocks back
            if start_height - h > 1000 {
                break;
            }
        }

        0
    }

    /// Find the fork point by looking up the given hash in our header store.
    /// Returns the height of the block with the given hash, or None.
    fn find_fork_point(&self, prev_hash: BlockHash) -> Option<u64> {
        let read_txn = self.store.read_txn().ok()?;
        // Search backwards from our tip to find the hash
        for height in (0..=self.tip_height).rev() {
            if let Ok(Some(stored)) = wolfe_store::HeaderStore::get_by_height(&read_txn, height as u32) {
                if stored.hash == prev_hash {
                    return Some(height);
                }
            }
            // Only search back 2016 blocks (one difficulty period) max
            if self.tip_height - height > 2016 {
                break;
            }
        }
        None
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
