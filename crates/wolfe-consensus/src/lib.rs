//! Consensus validation layer for BitcoinWolfe.
//!
//! This crate provides [`ConsensusEngine`], a safe and ergonomic Rust wrapper around
//! the [`bitcoinkernel`] crate (v0.2.0), which itself provides Rust bindings to
//! Bitcoin Core's `libbitcoinkernel` consensus library.
//!
//! # Architecture
//!
//! The `ConsensusEngine` encapsulates:
//! - A [`bitcoinkernel::Context`] configured with chain parameters and notification callbacks
//! - A [`bitcoinkernel::ChainstateManager`] for block validation and chain state queries
//! - An optional [`bitcoinkernel::Logger`] that bridges kernel log messages into the
//!   `tracing` ecosystem
//!
//! All kernel notification callbacks (block tip, header tip, progress, warnings, errors)
//! are routed through `tracing` spans and events, providing unified observability with
//! the rest of the BitcoinWolfe node.
//!
//! # Usage
//!
//! ```no_run
//! use wolfe_consensus::{ConsensusEngine, ChainType};
//!
//! // Initialize for regtest with a temporary directory
//! let engine = ConsensusEngine::new("/tmp/wolfe-data", ChainType::Regtest)?;
//!
//! // Validate a block from raw serialized bytes
//! let block_bytes: Vec<u8> = vec![]; // serialized block data
//! let result = engine.validate_block(&block_bytes)?;
//! println!("Block result: {:?}", result);
//!
//! // Query the chain tip
//! if let Some(tip) = engine.get_chain_tip() {
//!     println!("Chain tip: height={}, hash={}", tip.height, tip.hash_hex);
//! }
//!
//! // Query a block at a specific height
//! if let Some(info) = engine.get_block_at_height(0) {
//!     println!("Genesis block: {}", info.hash_hex);
//! }
//! # Ok::<(), wolfe_consensus::ConsensusError>(())
//! ```
//!
//! # Error Handling
//!
//! The crate defines [`ConsensusError`] for all failure modes, which can be
//! converted into [`wolfe_types::WolfeError::Consensus`] for propagation through
//! the broader node error hierarchy.
//!
//! In addition to standard operation errors, the kernel may report fatal errors or
//! flush errors asynchronously through its callback interface. These are logged at
//! `error` level via `tracing` and indicate that the engine should be torn down.

pub mod error;

pub use error::ConsensusError;

// Re-export key bitcoinkernel types that callers need to interact with.
pub use bitcoinkernel::ChainType;
pub use bitcoinkernel::ProcessBlockResult;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use bitcoinkernel::prelude::*;
use bitcoinkernel::{
    Block, BlockHash, BlockValidationStateRef, ChainstateManager, Context, ContextBuilder, Logger,
    SynchronizationState, ValidationMode, Warning,
};

use tracing::{debug, error, info, trace, warn};

// ---------------------------------------------------------------------------
// Bridge: kernel logging -> tracing
// ---------------------------------------------------------------------------

/// Bridges `bitcoinkernel` log messages into the `tracing` ecosystem.
///
/// The kernel library has its own internal logging system with a 1 MB buffer.
/// This struct implements `bitcoinkernel::Log` so that all kernel messages
/// are emitted as `tracing` events at the `debug` level under the
/// `wolfe_consensus::kernel` target.
struct TracingKernelLog;

impl bitcoinkernel::Log for TracingKernelLog {
    fn log(&self, message: &str) {
        // Kernel messages arrive with a trailing newline; strip it for cleaner output.
        let msg = message.trim_end();
        if !msg.is_empty() {
            debug!(target: "wolfe_consensus::kernel", "{}", msg);
        }
    }
}

// ---------------------------------------------------------------------------
// Block information returned from chain queries
// ---------------------------------------------------------------------------

/// Summary information about a block in the chain.
///
/// This is a lightweight, owned representation of block metadata suitable for
/// returning across API boundaries without lifetime ties to the
/// [`ChainstateManager`] internals.
#[derive(Debug, Clone)]
pub struct BlockInfo {
    /// Block height (0 for genesis).
    pub height: i32,
    /// Block hash as a hex-encoded string (human-readable byte order).
    pub hash_hex: String,
    /// Block hash as raw 32 bytes (internal byte order).
    pub hash_bytes: [u8; 32],
}

// ---------------------------------------------------------------------------
// ConsensusEngine
// ---------------------------------------------------------------------------

/// The main consensus validation engine for BitcoinWolfe.
///
/// `ConsensusEngine` manages the full lifecycle of the `libbitcoinkernel`
/// context and chainstate manager. It provides safe, high-level methods for
/// block validation and chain queries.
///
/// # Thread Safety
///
/// `ConsensusEngine` is `Send` and `Sync`. The underlying kernel types
/// (`Context`, `ChainstateManager`) are themselves `Send + Sync`, and all
/// shared state uses atomic operations.
///
/// # Lifecycle
///
/// On [`Drop`], the engine interrupts any in-progress kernel operations and
/// then drops the chainstate manager before the context, ensuring correct
/// teardown order as required by `libbitcoinkernel`.
pub struct ConsensusEngine {
    /// The kernel context. Must outlive `chainstate_manager`.
    /// Stored as an `Option` so we can control drop order in `Drop`.
    context: Option<Context>,

    /// The chainstate manager for block validation and chain queries.
    /// Stored as an `Option` so we can drop it before the context.
    chainstate_manager: Option<ChainstateManager>,

    /// The kernel logger connection. Kept alive so log messages continue
    /// to flow through tracing for the engine's lifetime.
    _logger: Option<Logger>,

    /// Flag set to `true` when a fatal error callback fires.
    /// Callers can check this to decide whether the engine is still usable.
    fatal_error_occurred: Arc<AtomicBool>,

    /// The chain type this engine was configured for.
    chain_type: ChainType,
}

// SAFETY: Context and ChainstateManager are thread-safe per the bitcoinkernel v0.2 crate
// documentation. The kernel library uses internal mutexes for all shared state.
// Context, ChainstateManager, and Logger are all documented as thread-safe.
// The Arc<AtomicBool> (fatal_error_occurred) is trivially Send + Sync.
// The ChainType is Copy and has no interior mutability.
// The Option wrappers are only mutated in Drop (which has exclusive access).
// Validated by: manual review of bitcoinkernel 0.2.0 source, all FFI calls go through
// cs_main or similar locks in libbitcoinkernel.
unsafe impl Send for ConsensusEngine {}
unsafe impl Sync for ConsensusEngine {}

impl ConsensusEngine {
    /// Creates and initializes a new consensus engine.
    ///
    /// This performs the following steps:
    /// 1. Creates the data and blocks directories if they do not exist.
    /// 2. Connects a [`Logger`] that routes kernel messages through `tracing`.
    /// 3. Builds a [`Context`] with the specified chain type and full notification
    ///    and validation callback wiring.
    /// 4. Creates a [`ChainstateManager`] pointing at the given directories.
    /// 5. Calls `import_blocks()` to complete initialization (loads block index,
    ///    replays if needed).
    ///
    /// # Arguments
    ///
    /// * `data_dir` - Root directory for chainstate data. A `blocks/` subdirectory
    ///   will be created inside it for raw block storage.
    /// * `chain_type` - The Bitcoin network to validate against.
    ///
    /// # Errors
    ///
    /// Returns [`ConsensusError`] if directory creation, context construction,
    /// chainstate manager creation, or block import fails.
    pub fn new(data_dir: impl AsRef<Path>, chain_type: ChainType) -> Result<Self, ConsensusError> {
        let data_dir = data_dir.as_ref();
        let blocks_dir = data_dir.join("blocks");

        // Ensure directories exist.
        std::fs::create_dir_all(data_dir).map_err(|e| {
            ConsensusError::InvalidDataDir(format!(
                "failed to create data directory '{}': {}",
                data_dir.display(),
                e
            ))
        })?;
        std::fs::create_dir_all(&blocks_dir).map_err(|e| {
            ConsensusError::InvalidDataDir(format!(
                "failed to create blocks directory '{}': {}",
                blocks_dir.display(),
                e
            ))
        })?;

        let data_dir_str = data_dir.to_str().ok_or_else(|| {
            ConsensusError::InvalidDataDir("data directory path is not valid UTF-8".to_string())
        })?;
        let blocks_dir_str = blocks_dir.to_str().ok_or_else(|| {
            ConsensusError::InvalidDataDir("blocks directory path is not valid UTF-8".to_string())
        })?;

        // Set up kernel logging -> tracing bridge.
        let logger = match Logger::new(TracingKernelLog) {
            Ok(l) => {
                l.enable_category(bitcoinkernel::LogCategory::All);
                l.set_level_category(
                    bitcoinkernel::LogCategory::All,
                    bitcoinkernel::LogLevel::Info,
                );
                debug!("kernel logger connected to tracing");
                Some(l)
            }
            Err(e) => {
                warn!("failed to create kernel logger, continuing without: {}", e);
                None
            }
        };

        // Shared flag for fatal error detection.
        let fatal_flag = Arc::new(AtomicBool::new(false));
        let fatal_flag_cb = Arc::clone(&fatal_flag);

        // Build the kernel context with full callback wiring.
        let context = ContextBuilder::new()
            .chain_type(chain_type)
            // -- Notification callbacks --
            .with_block_tip_notification(
                move |state: SynchronizationState, hash: BlockHash, progress: f64| {
                    info!(
                        target: "wolfe_consensus::tip",
                        sync_state = ?state,
                        block_hash = %hash,
                        progress = format!("{:.4}", progress),
                        "block tip updated"
                    );
                },
            )
            .with_header_tip_notification(
                |state: SynchronizationState, height: i64, timestamp: i64, presync: bool| {
                    debug!(
                        target: "wolfe_consensus::header_tip",
                        sync_state = ?state,
                        height = height,
                        timestamp = timestamp,
                        presync = presync,
                        "header tip updated"
                    );
                },
            )
            .with_progress_notification(|title: String, percent: i32, resume: bool| {
                info!(
                    target: "wolfe_consensus::progress",
                    title = %title,
                    percent = percent,
                    resumable = resume,
                    "sync progress"
                );
            })
            .with_warning_set_notification(move |warning: Warning, message: String| {
                warn!(
                    target: "wolfe_consensus::warning",
                    warning = %warning,
                    message = %message,
                    "kernel warning set"
                );
            })
            .with_warning_unset_notification(move |warning: Warning| {
                info!(
                    target: "wolfe_consensus::warning",
                    warning = %warning,
                    "kernel warning cleared"
                );
            })
            .with_flush_error_notification(|message: String| {
                error!(
                    target: "wolfe_consensus::error",
                    message = %message,
                    "kernel flush error - disk I/O failure"
                );
            })
            .with_fatal_error_notification(move |message: String| {
                fatal_flag_cb.store(true, Ordering::SeqCst);
                error!(
                    target: "wolfe_consensus::error",
                    message = %message,
                    "FATAL kernel error - engine should be torn down"
                );
            })
            // -- Validation callbacks --
            .with_block_checked_validation(|block: Block, state: BlockValidationStateRef<'_>| {
                let mode = state.mode();
                let result = state.result();
                match mode {
                    ValidationMode::Valid => {
                        trace!(
                            target: "wolfe_consensus::validation",
                            block_hash = %block.hash(),
                            validation_mode = ?mode,
                            validation_result = ?result,
                            "block checked"
                        );
                    }
                    _ => {
                        warn!(
                            target: "wolfe_consensus::validation",
                            block_hash = %block.hash(),
                            validation_mode = ?mode,
                            validation_result = ?result,
                            "block validation failed"
                        );
                    }
                }
            })
            .build()
            .map_err(|e| {
                ConsensusError::InitializationFailed(format!("context creation failed: {}", e))
            })?;

        info!(
            chain_type = ?chain_type,
            data_dir = %data_dir.display(),
            "kernel context created"
        );

        // Create the chainstate manager.
        let chainman =
            ChainstateManager::new(&context, data_dir_str, blocks_dir_str).map_err(|e| {
                ConsensusError::InitializationFailed(format!(
                    "chainstate manager creation failed: {}",
                    e
                ))
            })?;

        info!("chainstate manager created, importing blocks...");

        // Complete initialization: load block index, replay if needed.
        chainman
            .import_blocks()
            .map_err(|e| ConsensusError::ImportFailed(format!("import_blocks failed: {}", e)))?;

        let chain = chainman.active_chain();
        info!(
            chain_height = chain.height(),
            "block import complete, consensus engine ready"
        );

        Ok(Self {
            context: Some(context),
            chainstate_manager: Some(chainman),
            _logger: logger,
            fatal_error_occurred: fatal_flag,
            chain_type,
        })
    }

    /// Returns a reference to the underlying [`ChainstateManager`].
    ///
    /// # Panics
    ///
    /// Panics if the engine has already been dropped (should never happen
    /// in normal use since this is only callable while `self` is alive).
    fn chainman(&self) -> &ChainstateManager {
        self.chainstate_manager
            .as_ref()
            .expect("chainstate manager accessed after drop")
    }

    /// Returns `true` if a fatal error has been reported by the kernel.
    ///
    /// When this returns `true`, the engine is in an unrecoverable state.
    /// Callers should stop submitting blocks and drop the engine.
    pub fn has_fatal_error(&self) -> bool {
        self.fatal_error_occurred.load(Ordering::SeqCst)
    }

    /// Returns the chain type this engine was configured with.
    pub fn chain_type(&self) -> ChainType {
        self.chain_type
    }

    /// Validates a block from raw serialized bytes.
    ///
    /// The block goes through full consensus validation including proof-of-work,
    /// transaction validity, and script verification.
    ///
    /// # Arguments
    ///
    /// * `block_bytes` - The raw serialized block data (as received over P2P).
    ///
    /// # Returns
    ///
    /// A [`ProcessBlockResult`] indicating whether the block was:
    /// - `NewBlock` -- accepted and written to disk (not necessarily on the active chain)
    /// - `Duplicate` -- already known (valid)
    /// - `Rejected` -- failed consensus validation
    ///
    /// # Errors
    ///
    /// Returns [`ConsensusError::Kernel`] if the raw bytes cannot be deserialized
    /// into a valid block structure, or [`ConsensusError::FatalError`] if a fatal
    /// error has previously occurred.
    pub fn validate_block(&self, block_bytes: &[u8]) -> Result<ProcessBlockResult, ConsensusError> {
        if self.has_fatal_error() {
            return Err(ConsensusError::FatalError(
                "engine is in fatal error state, cannot validate blocks".to_string(),
            ));
        }

        let block = Block::new(block_bytes)?;

        debug!(
            block_hash = %block.hash(),
            block_size = block_bytes.len(),
            "processing block"
        );

        let result = self.chainman().process_block(&block);

        match result {
            ProcessBlockResult::NewBlock => {
                info!(block_hash = %block.hash(), "new block accepted");
            }
            ProcessBlockResult::Duplicate => {
                debug!(block_hash = %block.hash(), "duplicate block (already known)");
            }
            ProcessBlockResult::Rejected => {
                warn!(block_hash = %block.hash(), "block rejected by consensus");
            }
        }

        Ok(result)
    }

    /// Validates a block header from raw serialized bytes (80 bytes).
    ///
    /// This verifies proof of work and connection to an existing header or block.
    /// Useful for implementing "headers-first" synchronization where a complete
    /// header chain is synchronized before downloading full blocks.
    ///
    /// # Arguments
    ///
    /// * `header_bytes` - The raw serialized block header (80 bytes).
    ///
    /// # Returns
    ///
    /// `Ok(true)` if the header was successfully processed, `Ok(false)` if
    /// processing failed (the header violated consensus rules).
    ///
    /// # Errors
    ///
    /// Returns [`ConsensusError::Kernel`] if the raw bytes cannot be deserialized.
    ///
    /// # Important
    ///
    /// Callers must ensure that headers processed here build toward a most-work
    /// chain to avoid resource exhaustion from low-work header chains.
    pub fn validate_header(&self, header_bytes: &[u8]) -> Result<bool, ConsensusError> {
        if self.has_fatal_error() {
            return Err(ConsensusError::FatalError(
                "engine is in fatal error state, cannot validate headers".to_string(),
            ));
        }

        let header = bitcoinkernel::BlockHeader::new(header_bytes)?;

        let result = self.chainman().process_block_header(&header);

        match &result {
            bitcoinkernel::ProcessBlockHeaderResult::Success(_state) => {
                trace!("block header accepted");
                Ok(true)
            }
            bitcoinkernel::ProcessBlockHeaderResult::Failed(_state) => {
                debug!("block header rejected");
                Ok(false)
            }
        }
    }

    /// Returns information about the current chain tip (best block).
    ///
    /// The chain tip is the block with the most accumulated proof-of-work
    /// on the active chain.
    ///
    /// # Returns
    ///
    /// `Some(BlockInfo)` with the tip's height and hash, or `None` if the
    /// chain is uninitialized (should not happen after successful construction).
    pub fn get_chain_tip(&self) -> Option<BlockInfo> {
        let chain = self.chainman().active_chain();
        let tip = chain.tip();
        Some(block_tree_entry_to_info(&tip))
    }

    /// Returns the current chain height (tip height).
    ///
    /// This is equivalent to `get_chain_tip().map(|t| t.height)` but avoids
    /// constructing the full [`BlockInfo`].
    pub fn chain_height(&self) -> i32 {
        self.chainman().active_chain().height()
    }

    /// Returns information about the block at the given height on the active chain.
    ///
    /// # Arguments
    ///
    /// * `height` - The block height to query (0 = genesis).
    ///
    /// # Returns
    ///
    /// `Some(BlockInfo)` if a block exists at that height, `None` if the height
    /// exceeds the current chain tip.
    pub fn get_block_at_height(&self, height: u32) -> Option<BlockInfo> {
        let chain = self.chainman().active_chain();
        let entry = chain.at_height(height as usize)?;
        Some(block_tree_entry_to_info(&entry))
    }

    /// Returns information about a block identified by its hash.
    ///
    /// This searches the entire block tree, including blocks that are not on
    /// the active chain (e.g., stale/orphan blocks).
    ///
    /// # Arguments
    ///
    /// * `hash` - The 32-byte block hash (internal byte order).
    ///
    /// # Returns
    ///
    /// `Some(BlockInfo)` if a block with the given hash exists in the block tree,
    /// `None` otherwise.
    pub fn get_block_by_hash(&self, hash: &[u8; 32]) -> Option<BlockInfo> {
        let block_hash = BlockHash::from(*hash);
        let entry = self.chainman().get_block_tree_entry(&block_hash)?;
        Some(block_tree_entry_to_info(&entry))
    }

    /// Returns the block tree entry with the most known cumulative proof of work.
    ///
    /// This may differ from the active chain tip if a block with more work is
    /// known but has not yet been fully validated and connected.
    ///
    /// # Returns
    ///
    /// `Some(BlockInfo)` for the best-known entry, `None` if none exists (should
    /// not happen after initialization).
    pub fn get_best_known_block(&self) -> Option<BlockInfo> {
        let entry = self.chainman().best_entry()?;
        Some(block_tree_entry_to_info(&entry))
    }

    /// Checks whether a block (identified by hash) is on the active chain.
    ///
    /// # Arguments
    ///
    /// * `hash` - The 32-byte block hash (internal byte order).
    ///
    /// # Returns
    ///
    /// `true` if the block is part of the active chain, `false` if it is not
    /// found or is on a stale fork.
    pub fn is_block_in_active_chain(&self, hash: &[u8; 32]) -> bool {
        let block_hash = BlockHash::from(*hash);
        let entry = match self.chainman().get_block_tree_entry(&block_hash) {
            Some(e) => e,
            None => return false,
        };
        let chain = self.chainman().active_chain();
        chain.contains(&entry)
    }

    /// Reads the full block data from disk for the block at the given height.
    ///
    /// # Arguments
    ///
    /// * `height` - The block height on the active chain.
    ///
    /// # Returns
    ///
    /// The raw serialized block bytes, or an error if the block cannot be read
    /// (e.g., it has been pruned or the height is invalid).
    pub fn read_block_data_at_height(&self, height: u32) -> Result<Block, ConsensusError> {
        let chain = self.chainman().active_chain();
        let entry = chain.at_height(height as usize).ok_or_else(|| {
            ConsensusError::BlockNotFound(format!("no block at height {}", height))
        })?;
        self.chainman()
            .read_block_data(&entry)
            .map_err(|e| ConsensusError::BlockReadFailed(e.to_string()))
    }

    /// Interrupts any long-running kernel operations (e.g., block import, reindex).
    ///
    /// This is a cooperative signal -- the kernel will stop at the next safe point.
    /// Useful for graceful shutdown.
    ///
    /// # Errors
    ///
    /// Returns [`ConsensusError`] if the interrupt signal could not be delivered.
    pub fn interrupt(&self) -> Result<(), ConsensusError> {
        if let Some(ref ctx) = self.context {
            ctx.interrupt().map_err(|e| ConsensusError::Kernel(e))?;
            info!("kernel context interrupted");
        }
        Ok(())
    }
}

impl Drop for ConsensusEngine {
    fn drop(&mut self) {
        // Interrupt any in-progress operations.
        if let Some(ref ctx) = self.context {
            if let Err(e) = ctx.interrupt() {
                warn!("failed to interrupt kernel context during drop: {}", e);
            }
        }

        // Drop chainstate manager first (it depends on the context).
        // The ChainstateManager::drop calls btck_chainstate_manager_destroy.
        drop(self.chainstate_manager.take());
        debug!("chainstate manager dropped");

        // Then drop the context.
        // The Context::drop calls btck_context_destroy.
        drop(self.context.take());
        debug!("kernel context dropped");

        // Logger is dropped last (it has no dependency ordering requirement,
        // but keeping it alive until now ensures we capture all log output).
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Converts a `BlockTreeEntry` to an owned `BlockInfo`.
fn block_tree_entry_to_info(entry: &bitcoinkernel::BlockTreeEntry<'_>) -> BlockInfo {
    let hash_ref = entry.block_hash();
    let hash_bytes = hash_ref.to_bytes();
    let hash_hex = format!("{}", hash_ref);

    BlockInfo {
        height: entry.height(),
        hash_hex,
        hash_bytes,
    }
}

// ---------------------------------------------------------------------------
// Chain type conversion utilities
// ---------------------------------------------------------------------------

/// Converts a network name string (as used in [`wolfe_types::config::NetworkConfig`])
/// to a `bitcoinkernel::ChainType`.
///
/// Recognized values: "mainnet", "main", "testnet", "testnet3", "testnet4",
/// "signet", "regtest".
///
/// # Errors
///
/// Returns [`ConsensusError::UnknownChainType`] for unrecognized strings.
pub fn chain_type_from_str(s: &str) -> Result<ChainType, ConsensusError> {
    match s {
        "mainnet" | "main" => Ok(ChainType::Mainnet),
        "testnet" | "testnet3" => Ok(ChainType::Testnet),
        "testnet4" => Ok(ChainType::Testnet4),
        "signet" => Ok(ChainType::Signet),
        "regtest" => Ok(ChainType::Regtest),
        other => Err(ConsensusError::UnknownChainType(other.to_string())),
    }
}

/// Converts a `bitcoin::Network` (from the `bitcoin` crate) to the corresponding
/// `bitcoinkernel::ChainType`.
///
/// Note: `bitcoin::Network::Testnet` maps to `ChainType::Testnet` (testnet3).
/// There is no direct `bitcoin::Network` variant for testnet4.
pub fn chain_type_from_network(network: bitcoin::Network) -> ChainType {
    match network {
        bitcoin::Network::Bitcoin => ChainType::Mainnet,
        bitcoin::Network::Testnet => ChainType::Testnet,
        bitcoin::Network::Signet => ChainType::Signet,
        bitcoin::Network::Regtest => ChainType::Regtest,
        // Future bitcoin crate versions may add more variants.
        _ => ChainType::Mainnet,
    }
}

/// Converts a `bitcoinkernel::ChainType` to the corresponding `bitcoin::Network`.
pub fn network_from_chain_type(chain_type: ChainType) -> bitcoin::Network {
    match chain_type {
        ChainType::Mainnet => bitcoin::Network::Bitcoin,
        ChainType::Testnet => bitcoin::Network::Testnet,
        ChainType::Testnet4 => bitcoin::Network::Testnet,
        ChainType::Signet => bitcoin::Network::Signet,
        ChainType::Regtest => bitcoin::Network::Regtest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_type_from_str_mainnet() {
        assert_eq!(chain_type_from_str("mainnet").unwrap(), ChainType::Mainnet);
        assert_eq!(chain_type_from_str("main").unwrap(), ChainType::Mainnet);
    }

    #[test]
    fn test_chain_type_from_str_testnet() {
        assert_eq!(chain_type_from_str("testnet").unwrap(), ChainType::Testnet);
        assert_eq!(chain_type_from_str("testnet3").unwrap(), ChainType::Testnet);
    }

    #[test]
    fn test_chain_type_from_str_testnet4() {
        assert_eq!(
            chain_type_from_str("testnet4").unwrap(),
            ChainType::Testnet4
        );
    }

    #[test]
    fn test_chain_type_from_str_signet() {
        assert_eq!(chain_type_from_str("signet").unwrap(), ChainType::Signet);
    }

    #[test]
    fn test_chain_type_from_str_regtest() {
        assert_eq!(chain_type_from_str("regtest").unwrap(), ChainType::Regtest);
    }

    #[test]
    fn test_chain_type_from_str_unknown() {
        assert!(chain_type_from_str("fakenet").is_err());
        assert!(chain_type_from_str("").is_err());
    }

    #[test]
    fn test_chain_type_from_network() {
        assert_eq!(
            chain_type_from_network(bitcoin::Network::Bitcoin),
            ChainType::Mainnet
        );
        assert_eq!(
            chain_type_from_network(bitcoin::Network::Testnet),
            ChainType::Testnet
        );
        assert_eq!(
            chain_type_from_network(bitcoin::Network::Signet),
            ChainType::Signet
        );
        assert_eq!(
            chain_type_from_network(bitcoin::Network::Regtest),
            ChainType::Regtest
        );
    }

    #[test]
    fn test_network_from_chain_type() {
        assert_eq!(
            network_from_chain_type(ChainType::Mainnet),
            bitcoin::Network::Bitcoin
        );
        assert_eq!(
            network_from_chain_type(ChainType::Testnet),
            bitcoin::Network::Testnet
        );
        assert_eq!(
            network_from_chain_type(ChainType::Testnet4),
            bitcoin::Network::Testnet
        );
        assert_eq!(
            network_from_chain_type(ChainType::Signet),
            bitcoin::Network::Signet
        );
        assert_eq!(
            network_from_chain_type(ChainType::Regtest),
            bitcoin::Network::Regtest
        );
    }

    #[test]
    fn test_network_roundtrip() {
        // Verify that mainnet, testnet, signet, regtest survive a roundtrip.
        for network in [
            bitcoin::Network::Bitcoin,
            bitcoin::Network::Testnet,
            bitcoin::Network::Signet,
            bitcoin::Network::Regtest,
        ] {
            let chain_type = chain_type_from_network(network);
            let back = network_from_chain_type(chain_type);
            assert_eq!(network, back);
        }
    }

    #[test]
    fn test_block_info_debug() {
        let info = BlockInfo {
            height: 42,
            hash_hex: "00".repeat(32),
            hash_bytes: [0u8; 32],
        };
        let debug_str = format!("{:?}", info);
        assert!(debug_str.contains("42"));
    }

    #[test]
    fn test_block_info_clone() {
        let info = BlockInfo {
            height: 100,
            hash_hex: "ab".repeat(32),
            hash_bytes: [0xab; 32],
        };
        let cloned = info.clone();
        assert_eq!(cloned.height, 100);
        assert_eq!(cloned.hash_hex, info.hash_hex);
        assert_eq!(cloned.hash_bytes, info.hash_bytes);
    }
}
