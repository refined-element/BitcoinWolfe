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

        let fee_rate = self.policy.fee_rate_sat_per_vb(&tx, fee);
        self.policy.check_fee_rate(fee_rate)?;

        let size_vbytes = tx.weight().to_vbytes_ceil();

        // Check mempool capacity
        let current_bytes = self.total_bytes.load(Ordering::Relaxed);
        if current_bytes + size_vbytes as usize > self.max_bytes {
            return Err(MempoolError::Full {
                size_mb: current_bytes as f64 / (1024.0 * 1024.0),
                max_mb: self.max_bytes / (1024 * 1024),
            });
        }

        let entry = MempoolEntry {
            tx,
            txid,
            fee,
            fee_rate,
            size_vbytes,
            added_at: Instant::now(),
            ancestor_count: 0,
            descendant_count: 0,
        };

        self.entries.insert(txid, entry);
        self.total_bytes
            .fetch_add(size_vbytes as usize, Ordering::Relaxed);

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

    pub fn policy(&self) -> &PolicyEngine {
        &self.policy
    }
}
