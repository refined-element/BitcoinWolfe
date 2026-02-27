use bitcoin::{Transaction, Txid};
use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tracing::{debug, info};
use wolfe_types::config::MempoolConfig;

use crate::error::MempoolError;
use crate::policy::PolicyEngine;

/// A transaction stored in the mempool along with metadata.
#[derive(Debug, Clone)]
pub struct MempoolEntry {
    pub tx: Transaction,
    pub txid: Txid,
    pub fee: u64,
    pub fee_rate: f64,
    pub size_vbytes: u64,
    pub added_at: Instant,
    pub ancestor_count: usize,
    pub descendant_count: usize,
}

/// The mempool: holds unconfirmed transactions and enforces policy.
pub struct Mempool {
    entries: DashMap<Txid, MempoolEntry>,
    total_bytes: AtomicUsize,
    policy: PolicyEngine,
    max_bytes: usize,
}

impl Mempool {
    pub fn new(config: MempoolConfig) -> Self {
        let max_bytes = config.max_size_mb * 1024 * 1024;
        Self {
            entries: DashMap::new(),
            total_bytes: AtomicUsize::new(0),
            policy: PolicyEngine::new(config),
            max_bytes,
        }
    }

    /// Try to add a transaction to the mempool.
    /// The caller must provide the fee (computed from inputs - outputs).
    pub fn add(&self, tx: Transaction, fee: u64) -> Result<Txid, MempoolError> {
        let txid = tx.compute_txid();

        // Check for duplicates
        if self.entries.contains_key(&txid) {
            return Err(MempoolError::Duplicate(txid));
        }

        // Run policy checks
        self.policy.check(&tx)?;

        // If fee is 0 (caller couldn't calculate), try to estimate from mempool parents.
        // For transactions spending mempool parents, we can sum parent output values.
        let effective_fee = if fee == 0 {
            let mut input_sum = 0u64;
            let mut all_inputs_resolved = true;
            for input in &tx.input {
                if let Some(parent) = self.entries.get(&input.previous_output.txid) {
                    if let Some(output) = parent.tx.output.get(input.previous_output.vout as usize)
                    {
                        input_sum += output.value.to_sat();
                    } else {
                        all_inputs_resolved = false;
                    }
                } else {
                    // Input references a confirmed UTXO we don't have access to
                    all_inputs_resolved = false;
                }
            }

            if all_inputs_resolved && input_sum > 0 {
                let output_sum: u64 = tx.output.iter().map(|o| o.value.to_sat()).sum();
                input_sum.saturating_sub(output_sum)
            } else {
                // Can't determine fee — skip fee rate check for relay txs
                // (consensus engine will validate when the block is mined)
                fee
            }
        } else {
            fee
        };

        let fee_rate = self.policy.fee_rate_sat_per_vb(&tx, effective_fee);
        // Only enforce fee rate if we have a real fee (either provided or estimated)
        if fee > 0 || effective_fee > 0 {
            self.policy.check_fee_rate(fee_rate)?;
        }

        let size_vbytes = tx.weight().to_vbytes_ceil();

        // Check mempool capacity
        let current_bytes = self.total_bytes.load(Ordering::Relaxed);
        if current_bytes + size_vbytes as usize > self.max_bytes {
            return Err(MempoolError::Full {
                size_mb: current_bytes as f64 / (1024.0 * 1024.0),
                max_mb: self.max_bytes / (1024 * 1024),
            });
        }

        // Count in-mempool ancestors (transactions that this tx spends from)
        let mut ancestor_count = 0usize;
        for input in &tx.input {
            if self.entries.contains_key(&input.previous_output.txid) {
                ancestor_count += 1;
                // Also count ancestors of ancestors (transitive)
                if let Some(parent) = self.entries.get(&input.previous_output.txid) {
                    ancestor_count += parent.ancestor_count;
                }
            }
        }
        let max_ancestors = self.policy.config().max_ancestors;
        if ancestor_count > max_ancestors {
            return Err(MempoolError::TooManyAncestors {
                count: ancestor_count,
                max: max_ancestors,
            });
        }

        // Check descendant limits on parents we'd be spending from
        let max_descendants = self.policy.config().max_descendants;
        for input in &tx.input {
            if let Some(parent) = self.entries.get(&input.previous_output.txid) {
                if parent.descendant_count >= max_descendants {
                    return Err(MempoolError::TooManyDescendants {
                        count: parent.descendant_count + 1,
                        max: max_descendants,
                    });
                }
            }
        }

        let entry = MempoolEntry {
            tx: tx.clone(),
            txid,
            fee: effective_fee,
            fee_rate,
            size_vbytes,
            added_at: Instant::now(),
            ancestor_count,
            descendant_count: 0,
        };

        self.entries.insert(txid, entry);
        self.total_bytes
            .fetch_add(size_vbytes as usize, Ordering::Relaxed);

        // Increment descendant count on all parent entries
        for input in &tx.input {
            if let Some(mut parent) = self.entries.get_mut(&input.previous_output.txid) {
                parent.descendant_count += 1;
            }
        }

        debug!(%txid, fee_rate = format!("{:.1}", fee_rate), "tx accepted into mempool");
        Ok(txid)
    }

    /// Remove a transaction from the mempool (e.g., when mined).
    pub fn remove(&self, txid: &Txid) -> Option<MempoolEntry> {
        if let Some((_, entry)) = self.entries.remove(txid) {
            self.total_bytes
                .fetch_sub(entry.size_vbytes as usize, Ordering::Relaxed);
            Some(entry)
        } else {
            None
        }
    }

    /// Remove all transactions confirmed in a block.
    pub fn remove_for_block(&self, txids: &[Txid]) {
        let mut removed = 0;
        for txid in txids {
            if self.remove(txid).is_some() {
                removed += 1;
            }
        }
        if removed > 0 {
            info!(
                removed,
                remaining = self.entries.len(),
                "pruned mempool for new block"
            );
        }
    }

    /// Get a transaction from the mempool.
    pub fn get(&self, txid: &Txid) -> Option<MempoolEntry> {
        self.entries.get(txid).map(|e| e.clone())
    }

    /// Check if a transaction is in the mempool.
    pub fn contains(&self, txid: &Txid) -> bool {
        self.entries.contains_key(txid)
    }

    /// Number of transactions in the mempool.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total size of the mempool in bytes.
    pub fn size_bytes(&self) -> usize {
        self.total_bytes.load(Ordering::Relaxed)
    }

    /// Get all transactions sorted by fee rate (highest first).
    pub fn get_sorted_by_fee_rate(&self) -> Vec<MempoolEntry> {
        let mut entries: Vec<MempoolEntry> = self.entries.iter().map(|e| e.clone()).collect();
        entries.sort_by(|a, b| {
            b.fee_rate
                .partial_cmp(&a.fee_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        entries
    }

    /// Evict the lowest fee-rate transactions to bring the mempool under its size limit.
    pub fn trim(&self) {
        let current = self.total_bytes.load(Ordering::Relaxed);
        if current <= self.max_bytes {
            return;
        }

        let mut entries = self.get_sorted_by_fee_rate();
        // Remove from the tail (lowest fee rate) until we're under the limit
        while self.total_bytes.load(Ordering::Relaxed) > self.max_bytes {
            if let Some(entry) = entries.pop() {
                self.remove(&entry.txid);
            } else {
                break;
            }
        }
    }

    /// Expire old transactions.
    pub fn expire(&self, max_age: std::time::Duration) {
        let expired: Vec<Txid> = self
            .entries
            .iter()
            .filter(|e| e.added_at.elapsed() > max_age)
            .map(|e| e.txid)
            .collect();

        for txid in &expired {
            self.remove(txid);
        }

        if !expired.is_empty() {
            info!(count = expired.len(), "expired old mempool transactions");
        }
    }

    /// Total size of the mempool in bytes (alias for size_bytes).
    pub fn total_bytes(&self) -> usize {
        self.total_bytes.load(Ordering::Relaxed)
    }

    /// Minimum fee rate configured for this mempool.
    pub fn min_fee_rate(&self) -> f64 {
        self.policy.config().min_fee_rate
    }

    /// Build a fee rate histogram: buckets of (fee_rate_sat_per_vb, tx_count).
    /// Returns buckets in descending order of fee rate.
    pub fn fee_histogram(&self) -> Vec<(f64, usize)> {
        let boundaries = [500.0, 200.0, 100.0, 50.0, 20.0, 10.0, 5.0, 2.0, 1.0];
        let mut buckets: Vec<(f64, usize)> = boundaries.iter().map(|&b| (b, 0)).collect();

        for entry in self.entries.iter() {
            for bucket in buckets.iter_mut() {
                if entry.fee_rate >= bucket.0 {
                    bucket.1 += 1;
                    break;
                }
            }
        }

        // Only return non-empty buckets
        buckets.retain(|b| b.1 > 0);
        buckets
    }

    pub fn policy(&self) -> &PolicyEngine {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::blockdata::locktime::absolute::LockTime;
    use bitcoin::blockdata::opcodes::all::OP_RETURN;
    use bitcoin::blockdata::script::{Builder, ScriptBuf};
    use bitcoin::blockdata::transaction::{OutPoint, Sequence, TxIn, TxOut, Version};
    use bitcoin::blockdata::witness::Witness;
    use bitcoin::hashes::Hash;
    use bitcoin::Amount;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    /// Default config with generous limits for most tests.
    fn default_config() -> MempoolConfig {
        MempoolConfig {
            max_size_mb: 300,
            min_fee_rate: 1.0,
            max_datacarrier_bytes: 80,
            datacarrier: true,
            full_rbf: true,
            max_ancestors: 25,
            max_descendants: 25,
            expiry_hours: 336,
        }
    }

    /// Config with a tiny mempool to test capacity limits.
    /// Size is set so that only a few small transactions fit.
    fn tiny_mempool_config() -> MempoolConfig {
        MempoolConfig {
            max_size_mb: 0, // 0 MB -- effectively 0 bytes
            min_fee_rate: 1.0,
            ..default_config()
        }
    }

    fn make_txid(bytes: [u8; 32]) -> Txid {
        Txid::from_raw_hash(bitcoin::hashes::sha256d::Hash::from_byte_array(bytes))
    }

    /// Create a dummy OutPoint with a zeroed txid and given vout index.
    fn dummy_outpoint(vout: u32) -> OutPoint {
        OutPoint {
            txid: make_txid([0u8; 32]),
            vout,
        }
    }

    /// Create a unique transaction by varying the input outpoint vout.
    /// Each distinct `index` produces a transaction with a unique txid.
    fn unique_tx(index: u32) -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: dummy_outpoint(index),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::default(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(50_000),
                script_pubkey: ScriptBuf::new(),
            }],
        }
    }

    /// Compute a fee that will pass the minimum fee rate for a given transaction.
    /// Returns a fee in satoshis such that fee / vbytes >= min_fee_rate.
    fn fee_for_rate(tx: &Transaction, rate: f64) -> u64 {
        let vbytes = tx.weight().to_vbytes_ceil();
        (rate * vbytes as f64).ceil() as u64
    }

    /// Create a transaction with an OP_RETURN output.
    fn tx_with_op_return(data_len: usize) -> Transaction {
        let op_return_script = if data_len == 0 {
            Builder::new().push_opcode(OP_RETURN).into_script()
        } else {
            let data = vec![0xab; data_len];
            ScriptBuf::new_op_return(
                bitcoin::script::PushBytesBuf::try_from(data)
                    .expect("data should fit in PushBytesBuf"),
            )
        };

        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: dummy_outpoint(0),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::default(),
            }],
            output: vec![
                TxOut {
                    value: Amount::from_sat(50_000),
                    script_pubkey: ScriptBuf::new(),
                },
                TxOut {
                    value: Amount::from_sat(0),
                    script_pubkey: op_return_script,
                },
            ],
        }
    }

    /// Helper: create a mempool and add a valid transaction, returning the pool
    /// and the txid.
    fn mempool_with_one_tx() -> (Mempool, Txid) {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 2.0);
        let txid = pool.add(tx, fee).expect("add should succeed");
        (pool, txid)
    }

    // -----------------------------------------------------------------------
    // Mempool::new
    // -----------------------------------------------------------------------

    #[test]
    fn new_mempool_is_empty() {
        let pool = Mempool::new(default_config());
        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.size_bytes(), 0);
    }

    // -----------------------------------------------------------------------
    // Mempool::add -- happy path
    // -----------------------------------------------------------------------

    #[test]
    fn add_valid_transaction_succeeds() {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(0);
        let expected_txid = tx.compute_txid();
        let fee = fee_for_rate(&tx, 2.0);
        let txid = pool.add(tx, fee).expect("add should succeed");
        assert_eq!(txid, expected_txid);
        assert_eq!(pool.len(), 1);
        assert!(!pool.is_empty());
    }

    #[test]
    fn add_increments_size_bytes() {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(0);
        let expected_vbytes = tx.weight().to_vbytes_ceil() as usize;
        let fee = fee_for_rate(&tx, 2.0);
        pool.add(tx, fee).unwrap();
        assert_eq!(pool.size_bytes(), expected_vbytes);
    }

    #[test]
    fn add_multiple_transactions() {
        let pool = Mempool::new(default_config());
        let mut total_vbytes = 0usize;
        for i in 0..5 {
            let tx = unique_tx(i);
            total_vbytes += tx.weight().to_vbytes_ceil() as usize;
            let fee = fee_for_rate(&tx, 2.0);
            pool.add(tx, fee).expect("add should succeed");
        }
        assert_eq!(pool.len(), 5);
        assert_eq!(pool.size_bytes(), total_vbytes);
    }

    #[test]
    fn add_stores_correct_metadata() {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(42);
        let expected_txid = tx.compute_txid();
        let expected_vbytes = tx.weight().to_vbytes_ceil();
        let fee = 5000u64;
        let fee_rate = fee as f64 / expected_vbytes as f64;
        pool.add(tx.clone(), fee).unwrap();

        let entry = pool.get(&expected_txid).expect("should exist");
        assert_eq!(entry.txid, expected_txid);
        assert_eq!(entry.fee, fee);
        assert_eq!(entry.size_vbytes, expected_vbytes);
        assert!(
            (entry.fee_rate - fee_rate).abs() < 0.01,
            "fee_rate mismatch: expected {fee_rate}, got {}",
            entry.fee_rate
        );
        assert_eq!(entry.ancestor_count, 0);
        assert_eq!(entry.descendant_count, 0);
    }

    // -----------------------------------------------------------------------
    // Mempool::add -- duplicate rejection
    // -----------------------------------------------------------------------

    #[test]
    fn add_duplicate_transaction_rejected() {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 2.0);
        let txid = pool.add(tx.clone(), fee).unwrap();
        let err = pool.add(tx, fee).unwrap_err();
        match err {
            MempoolError::Duplicate(dup_txid) => assert_eq!(dup_txid, txid),
            other => panic!("expected Duplicate, got: {other:?}"),
        }
        // Pool should still have exactly 1 entry
        assert_eq!(pool.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Mempool::add -- policy enforcement (datacarrier)
    // -----------------------------------------------------------------------

    #[test]
    fn add_rejects_op_return_when_datacarrier_disabled() {
        let config = MempoolConfig {
            datacarrier: false,
            ..default_config()
        };
        let pool = Mempool::new(config);
        let tx = tx_with_op_return(10);
        let fee = fee_for_rate(&tx, 2.0);
        let err = pool.add(tx, fee).unwrap_err();
        assert!(matches!(err, MempoolError::DatacarrierDisabled));
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn add_rejects_op_return_too_large() {
        let config = MempoolConfig {
            datacarrier: true,
            max_datacarrier_bytes: 10,
            ..default_config()
        };
        let pool = Mempool::new(config);
        let tx = tx_with_op_return(20);
        let fee = fee_for_rate(&tx, 2.0);
        let err = pool.add(tx, fee).unwrap_err();
        assert!(
            matches!(err, MempoolError::DatacarrierTooLarge { .. }),
            "expected DatacarrierTooLarge, got: {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Mempool::add -- fee rate enforcement
    // -----------------------------------------------------------------------

    #[test]
    fn add_rejects_too_low_fee_rate() {
        let config = MempoolConfig {
            min_fee_rate: 5.0,
            ..default_config()
        };
        let pool = Mempool::new(config);
        let tx = unique_tx(0);
        // Provide a fee that gives rate < 5.0 sat/vB
        let fee = 1; // 1 sat total for a ~85 vbyte tx = ~0.012 sat/vB
        let err = pool.add(tx, fee).unwrap_err();
        assert!(
            matches!(err, MempoolError::FeeTooLow { .. }),
            "expected FeeTooLow, got: {err:?}"
        );
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn add_accepts_fee_rate_at_minimum() {
        let config = MempoolConfig {
            min_fee_rate: 1.0,
            ..default_config()
        };
        let pool = Mempool::new(config);
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 1.0);
        assert!(pool.add(tx, fee).is_ok());
    }

    #[test]
    fn add_accepts_fee_rate_above_minimum() {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 100.0);
        assert!(pool.add(tx, fee).is_ok());
    }

    // -----------------------------------------------------------------------
    // Mempool::add -- capacity enforcement
    // -----------------------------------------------------------------------

    #[test]
    fn add_rejects_when_mempool_full() {
        // Create a mempool with max_size_mb = 0, which means max_bytes = 0.
        let pool = Mempool::new(tiny_mempool_config());
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 2.0);
        let err = pool.add(tx, fee).unwrap_err();
        assert!(
            matches!(err, MempoolError::Full { .. }),
            "expected Full, got: {err:?}"
        );
    }

    #[test]
    fn add_does_not_increment_counters_on_rejection() {
        let pool = Mempool::new(tiny_mempool_config());
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 2.0);
        let _ = pool.add(tx, fee);
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.size_bytes(), 0);
    }

    // -----------------------------------------------------------------------
    // Mempool::remove
    // -----------------------------------------------------------------------

    #[test]
    fn remove_existing_transaction() {
        let (pool, txid) = mempool_with_one_tx();
        let entry = pool.remove(&txid);
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert_eq!(entry.txid, txid);
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.size_bytes(), 0);
    }

    #[test]
    fn remove_nonexistent_transaction_returns_none() {
        let pool = Mempool::new(default_config());
        let fake_txid = make_txid([0xff; 32]);
        assert!(pool.remove(&fake_txid).is_none());
    }

    #[test]
    fn remove_decrements_size_bytes() {
        let pool = Mempool::new(default_config());

        let tx1 = unique_tx(0);
        let tx2 = unique_tx(1);
        let vb1 = tx1.weight().to_vbytes_ceil() as usize;
        let vb2 = tx2.weight().to_vbytes_ceil() as usize;
        let fee1 = fee_for_rate(&tx1, 2.0);
        let fee2 = fee_for_rate(&tx2, 2.0);
        let txid1 = pool.add(tx1, fee1).unwrap();
        pool.add(tx2, fee2).unwrap();

        assert_eq!(pool.size_bytes(), vb1 + vb2);

        pool.remove(&txid1);
        assert_eq!(pool.size_bytes(), vb2);
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn remove_idempotent() {
        let (pool, txid) = mempool_with_one_tx();
        assert!(pool.remove(&txid).is_some());
        assert!(pool.remove(&txid).is_none());
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.size_bytes(), 0);
    }

    // -----------------------------------------------------------------------
    // Mempool::remove_for_block
    // -----------------------------------------------------------------------

    #[test]
    fn remove_for_block_removes_matching_txids() {
        let pool = Mempool::new(default_config());
        let mut txids = Vec::new();
        for i in 0..5 {
            let tx = unique_tx(i);
            let fee = fee_for_rate(&tx, 2.0);
            txids.push(pool.add(tx, fee).unwrap());
        }

        // Remove first 3
        pool.remove_for_block(&txids[..3]);
        assert_eq!(pool.len(), 2);
        assert!(!pool.contains(&txids[0]));
        assert!(!pool.contains(&txids[1]));
        assert!(!pool.contains(&txids[2]));
        assert!(pool.contains(&txids[3]));
        assert!(pool.contains(&txids[4]));
    }

    #[test]
    fn remove_for_block_ignores_unknown_txids() {
        let (pool, txid) = mempool_with_one_tx();
        let unknown = make_txid([0xff; 32]);
        pool.remove_for_block(&[unknown]);
        assert_eq!(pool.len(), 1);
        assert!(pool.contains(&txid));
    }

    #[test]
    fn remove_for_block_with_empty_slice() {
        let (pool, txid) = mempool_with_one_tx();
        pool.remove_for_block(&[]);
        assert_eq!(pool.len(), 1);
        assert!(pool.contains(&txid));
    }

    #[test]
    fn remove_for_block_mixed_known_and_unknown() {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 2.0);
        let txid = pool.add(tx, fee).unwrap();
        let unknown = make_txid([0xff; 32]);

        pool.remove_for_block(&[unknown, txid]);
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn remove_for_block_decrements_size_bytes() {
        let pool = Mempool::new(default_config());

        let mut txids = Vec::new();
        let mut total_vbytes = 0usize;
        for i in 0..3 {
            let tx = unique_tx(i);
            total_vbytes += tx.weight().to_vbytes_ceil() as usize;
            let fee = fee_for_rate(&tx, 2.0);
            txids.push(pool.add(tx, fee).unwrap());
        }
        assert_eq!(pool.size_bytes(), total_vbytes);

        // Remove first tx -- get its vbytes for verification
        let first_vb = pool.get(&txids[0]).unwrap().size_vbytes as usize;
        pool.remove_for_block(&txids[..1]);
        assert_eq!(pool.size_bytes(), total_vbytes - first_vb);
    }

    // -----------------------------------------------------------------------
    // Mempool::get
    // -----------------------------------------------------------------------

    #[test]
    fn get_existing_transaction() {
        let (pool, txid) = mempool_with_one_tx();
        let entry = pool.get(&txid);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().txid, txid);
    }

    #[test]
    fn get_nonexistent_transaction_returns_none() {
        let pool = Mempool::new(default_config());
        let fake_txid = make_txid([0xaa; 32]);
        assert!(pool.get(&fake_txid).is_none());
    }

    // -----------------------------------------------------------------------
    // Mempool::contains
    // -----------------------------------------------------------------------

    #[test]
    fn contains_returns_true_for_present_tx() {
        let (pool, txid) = mempool_with_one_tx();
        assert!(pool.contains(&txid));
    }

    #[test]
    fn contains_returns_false_for_absent_tx() {
        let pool = Mempool::new(default_config());
        let fake_txid = make_txid([0xbb; 32]);
        assert!(!pool.contains(&fake_txid));
    }

    #[test]
    fn contains_false_after_removal() {
        let (pool, txid) = mempool_with_one_tx();
        pool.remove(&txid);
        assert!(!pool.contains(&txid));
    }

    // -----------------------------------------------------------------------
    // Mempool::len, is_empty, size_bytes
    // -----------------------------------------------------------------------

    #[test]
    fn len_and_is_empty_consistency() {
        let pool = Mempool::new(default_config());
        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);

        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 2.0);
        let txid = pool.add(tx, fee).unwrap();
        assert!(!pool.is_empty());
        assert_eq!(pool.len(), 1);

        pool.remove(&txid);
        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);
    }

    // -----------------------------------------------------------------------
    // Mempool::get_sorted_by_fee_rate
    // -----------------------------------------------------------------------

    #[test]
    fn get_sorted_by_fee_rate_returns_highest_first() {
        let pool = Mempool::new(default_config());

        // Add transactions with different fee rates
        let rates = [1.0, 10.0, 5.0, 20.0, 2.0];
        for (i, &rate) in rates.iter().enumerate() {
            let tx = unique_tx(i as u32);
            let fee = fee_for_rate(&tx, rate);
            pool.add(tx, fee).unwrap();
        }

        let sorted = pool.get_sorted_by_fee_rate();
        assert_eq!(sorted.len(), 5);

        // Verify descending order of fee rates
        for i in 1..sorted.len() {
            assert!(
                sorted[i - 1].fee_rate >= sorted[i].fee_rate,
                "entries should be in descending fee rate order: {} >= {} failed at index {}",
                sorted[i - 1].fee_rate,
                sorted[i].fee_rate,
                i
            );
        }

        // The highest rate should be first
        assert!(
            sorted[0].fee_rate >= 19.0,
            "first entry should have the highest fee rate (~20 sat/vB), got {}",
            sorted[0].fee_rate
        );
    }

    #[test]
    fn get_sorted_by_fee_rate_empty_pool() {
        let pool = Mempool::new(default_config());
        let sorted = pool.get_sorted_by_fee_rate();
        assert!(sorted.is_empty());
    }

    #[test]
    fn get_sorted_by_fee_rate_single_entry() {
        let (pool, txid) = mempool_with_one_tx();
        let sorted = pool.get_sorted_by_fee_rate();
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].txid, txid);
    }

    // -----------------------------------------------------------------------
    // Mempool::trim
    // -----------------------------------------------------------------------

    #[test]
    fn trim_evicts_lowest_fee_rate_first() {
        // Create a mempool with 1 MB capacity
        let config = MempoolConfig {
            max_size_mb: 1,
            min_fee_rate: 1.0,
            ..default_config()
        };
        let pool = Mempool::new(config);

        // Add several transactions with different fee rates
        let mut txids_and_rates: Vec<(Txid, f64)> = Vec::new();
        let rates = [2.0, 10.0, 5.0, 20.0, 1.0];
        for (i, &rate) in rates.iter().enumerate() {
            let tx = unique_tx(i as u32);
            let fee = fee_for_rate(&tx, rate);
            let txid = pool.add(tx, fee).unwrap();
            txids_and_rates.push((txid, rate));
        }

        // The pool is well under 1 MB, so trim should not evict anything
        pool.trim();
        assert_eq!(pool.len(), 5, "trim should be a no-op when under capacity");
    }

    #[test]
    fn trim_does_nothing_when_under_capacity() {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 2.0);
        pool.add(tx, fee).unwrap();

        let size_before = pool.size_bytes();
        let len_before = pool.len();
        pool.trim();
        assert_eq!(pool.size_bytes(), size_before);
        assert_eq!(pool.len(), len_before);
    }

    #[test]
    fn trim_on_empty_pool_is_noop() {
        let pool = Mempool::new(default_config());
        pool.trim();
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.size_bytes(), 0);
    }

    #[test]
    fn trim_evicts_lowest_fee_rate_when_over_capacity() {
        // Use a 1 MB mempool and force it over capacity by manipulating total_bytes
        let config = MempoolConfig {
            max_size_mb: 1,
            min_fee_rate: 1.0,
            ..default_config()
        };
        let pool = Mempool::new(config);

        // Add 3 transactions with known fee rates
        let low_tx = unique_tx(0);
        let mid_tx = unique_tx(1);
        let high_tx = unique_tx(2);

        let low_fee = fee_for_rate(&low_tx, 1.0);
        let mid_fee = fee_for_rate(&mid_tx, 5.0);
        let high_fee = fee_for_rate(&high_tx, 10.0);

        let low_txid = pool.add(low_tx, low_fee).unwrap();
        let _mid_txid = pool.add(mid_tx, mid_fee).unwrap();
        let high_txid = pool.add(high_tx, high_fee).unwrap();

        // Artificially inflate total_bytes to exceed max_bytes (1 MiB = 1048576).
        // We add enough to push us over the limit.
        let current = pool.size_bytes();
        let inflate = 1024 * 1024 + 1 - current;
        pool.total_bytes
            .fetch_add(inflate, std::sync::atomic::Ordering::Relaxed);

        assert!(pool.size_bytes() > pool.max_bytes);

        // Now trim -- it should evict the lowest fee-rate tx first
        pool.trim();

        // The low fee rate tx should have been evicted
        assert!(
            !pool.contains(&low_txid),
            "lowest fee-rate tx should be evicted"
        );
        // The high fee rate tx should remain
        assert!(
            pool.contains(&high_txid),
            "highest fee-rate tx should survive trim"
        );
    }

    // -----------------------------------------------------------------------
    // Mempool::expire
    // -----------------------------------------------------------------------

    #[test]
    fn expire_removes_old_transactions() {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 2.0);
        let txid = pool.add(tx, fee).unwrap();

        // Expire with zero duration -- everything is "old"
        pool.expire(std::time::Duration::from_secs(0));
        assert!(
            !pool.contains(&txid),
            "transaction should be expired with zero max_age"
        );
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn expire_keeps_recent_transactions() {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 2.0);
        let txid = pool.add(tx, fee).unwrap();

        // Expire with a large duration -- nothing should be removed
        pool.expire(std::time::Duration::from_secs(3600));
        assert!(pool.contains(&txid));
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn expire_decrements_size_bytes() {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 2.0);
        pool.add(tx, fee).unwrap();

        assert!(pool.size_bytes() > 0);
        pool.expire(std::time::Duration::from_secs(0));
        assert_eq!(pool.size_bytes(), 0);
    }

    #[test]
    fn expire_on_empty_pool_is_noop() {
        let pool = Mempool::new(default_config());
        pool.expire(std::time::Duration::from_secs(0));
        assert_eq!(pool.len(), 0);
    }

    // -----------------------------------------------------------------------
    // Mempool::policy accessor
    // -----------------------------------------------------------------------

    #[test]
    fn policy_accessor_returns_engine_with_correct_config() {
        let config = MempoolConfig {
            min_fee_rate: 7.5,
            ..default_config()
        };
        let pool = Mempool::new(config);
        assert!((pool.policy().config().min_fee_rate - 7.5).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // Edge case: weight/size limit enforced through add()
    // -----------------------------------------------------------------------

    #[test]
    fn add_rejects_oversized_transaction() {
        let pool = Mempool::new(default_config());

        // Build a transaction exceeding 400,000 WU (100,000 vbytes)
        let outputs: Vec<TxOut> = (0..3000)
            .map(|_| TxOut {
                value: Amount::from_sat(1000),
                script_pubkey: ScriptBuf::from_bytes(vec![
                    0x76, 0xa9, 0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0x88, 0xac,
                ]),
            })
            .collect();

        let big_tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: dummy_outpoint(0),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::default(),
            }],
            output: outputs,
        };

        let weight = big_tx.weight().to_wu() as usize;
        assert!(weight > 400_000, "setup: tx must exceed 400k WU");

        let fee = fee_for_rate(&big_tx, 10.0);
        let err = pool.add(big_tx, fee).unwrap_err();
        assert!(
            matches!(err, MempoolError::TooLarge { .. }),
            "expected TooLarge, got: {err:?}"
        );
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.size_bytes(), 0);
    }

    // -----------------------------------------------------------------------
    // Full lifecycle test
    // -----------------------------------------------------------------------

    #[test]
    fn lifecycle_add_get_remove_for_block() {
        let pool = Mempool::new(default_config());

        // Add 3 transactions
        let mut txids = Vec::new();
        for i in 0..3 {
            let tx = unique_tx(i);
            let fee = fee_for_rate(&tx, (i + 1) as f64 * 2.0);
            txids.push(pool.add(tx, fee).unwrap());
        }
        assert_eq!(pool.len(), 3);

        // Verify all present
        for txid in &txids {
            assert!(pool.contains(txid));
            assert!(pool.get(txid).is_some());
        }

        // Simulate a block confirming the first two
        pool.remove_for_block(&txids[..2]);
        assert_eq!(pool.len(), 1);
        assert!(!pool.contains(&txids[0]));
        assert!(!pool.contains(&txids[1]));
        assert!(pool.contains(&txids[2]));

        // Remove the last one
        let entry = pool.remove(&txids[2]).unwrap();
        assert_eq!(entry.txid, txids[2]);
        assert!(pool.is_empty());
        assert_eq!(pool.size_bytes(), 0);
    }

    // -----------------------------------------------------------------------
    // Add after remove: re-adding a previously removed tx
    // -----------------------------------------------------------------------

    #[test]
    fn can_readd_removed_transaction() {
        let pool = Mempool::new(default_config());
        let tx = unique_tx(0);
        let fee = fee_for_rate(&tx, 2.0);

        let txid = pool.add(tx.clone(), fee).unwrap();
        pool.remove(&txid);
        assert!(!pool.contains(&txid));

        // Re-add should succeed
        let txid2 = pool.add(tx, fee).unwrap();
        assert_eq!(txid, txid2);
        assert!(pool.contains(&txid));
        assert_eq!(pool.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Sorted order with equal fee rates
    // -----------------------------------------------------------------------

    #[test]
    fn get_sorted_by_fee_rate_handles_equal_rates() {
        let pool = Mempool::new(default_config());

        // Add 3 transactions all with the same fee rate
        for i in 0..3 {
            let tx = unique_tx(i);
            let fee = fee_for_rate(&tx, 5.0);
            pool.add(tx, fee).unwrap();
        }

        let sorted = pool.get_sorted_by_fee_rate();
        assert_eq!(sorted.len(), 3);

        // All should have approximately the same fee rate
        for entry in &sorted {
            assert!(
                (entry.fee_rate - sorted[0].fee_rate).abs() < 0.1,
                "all entries should have ~same fee rate"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Capacity boundary: exactly at limit
    // -----------------------------------------------------------------------

    #[test]
    fn add_rejects_when_exactly_at_capacity() {
        // max_size_mb = 1 => max_bytes = 1048576
        let config = MempoolConfig {
            max_size_mb: 1,
            min_fee_rate: 1.0,
            ..default_config()
        };
        let pool = Mempool::new(config);

        // Fill the pool to exactly the limit by inflating the counter
        let tx = unique_tx(0);
        let vbytes = tx.weight().to_vbytes_ceil() as usize;
        let inflate = 1024 * 1024 - vbytes; // Leave room for exactly 0 more bytes after adding
        pool.total_bytes
            .fetch_add(inflate, std::sync::atomic::Ordering::Relaxed);

        // Now adding the tx would make total = inflate + vbytes = 1048576, which is
        // exactly max_bytes, so current + size > max_bytes is false (equal, not greater).
        let fee = fee_for_rate(&tx, 2.0);
        // current_bytes = inflate, adding vbytes gives inflate + vbytes = 1048576 = max_bytes
        // The check is: current_bytes + size_vbytes > max_bytes
        // inflate + vbytes = 1048576, max_bytes = 1048576, so NOT greater => should succeed
        let result = pool.add(tx, fee);
        // At exactly the boundary (==), the check passes
        assert!(
            result.is_ok(),
            "adding tx that fills pool exactly to capacity should succeed"
        );
    }

    #[test]
    fn add_rejects_when_one_byte_over_capacity() {
        let config = MempoolConfig {
            max_size_mb: 1,
            min_fee_rate: 1.0,
            ..default_config()
        };
        let pool = Mempool::new(config);

        let tx = unique_tx(0);
        let vbytes = tx.weight().to_vbytes_ceil() as usize;
        // Inflate so adding the tx would be 1 byte over
        let inflate = 1024 * 1024 - vbytes + 1;
        pool.total_bytes
            .fetch_add(inflate, std::sync::atomic::Ordering::Relaxed);

        let fee = fee_for_rate(&tx, 2.0);
        let err = pool.add(tx, fee).unwrap_err();
        assert!(
            matches!(err, MempoolError::Full { .. }),
            "expected Full, got: {err:?}"
        );
    }
}
