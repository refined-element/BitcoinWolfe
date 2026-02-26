use bitcoin::Transaction;
use wolfe_types::config::MempoolConfig;

use crate::error::MempoolError;

/// Policy engine that decides whether a transaction meets our relay/acceptance criteria.
/// This is where BitcoinWolfe's opinionated defaults live — and where operators can tune.
pub struct PolicyEngine {
    config: MempoolConfig,
}

impl PolicyEngine {
    pub fn new(config: MempoolConfig) -> Self {
        Self { config }
    }

    /// Check if a transaction passes our policy rules.
    /// This does NOT check consensus validity — that's the kernel's job.
    pub fn check(&self, tx: &Transaction) -> Result<(), MempoolError> {
        self.check_datacarrier(tx)?;
        self.check_size(tx)?;
        Ok(())
    }

    /// Check OP_RETURN outputs against our policy.
    fn check_datacarrier(&self, tx: &Transaction) -> Result<(), MempoolError> {
        for output in &tx.output {
            if output.script_pubkey.is_op_return() {
                if !self.config.datacarrier {
                    return Err(MempoolError::DatacarrierDisabled);
                }

                let data_len = output.script_pubkey.len();
                if data_len > self.config.max_datacarrier_bytes {
                    return Err(MempoolError::DatacarrierTooLarge {
                        size: data_len,
                        max: self.config.max_datacarrier_bytes,
                    });
                }
            }
        }
        Ok(())
    }

    /// Check transaction weight/size limits.
    fn check_size(&self, tx: &Transaction) -> Result<(), MempoolError> {
        let weight = tx.weight().to_wu() as usize;
        // Standard transaction weight limit: 400,000 WU (100,000 vbytes)
        let max_weight = 400_000;
        if weight > max_weight {
            return Err(MempoolError::TooLarge { size: weight });
        }
        Ok(())
    }

    /// Estimate the fee rate of a transaction given its total fee.
    pub fn fee_rate_sat_per_vb(&self, tx: &Transaction, fee_sats: u64) -> f64 {
        let vsize = tx.weight().to_vbytes_ceil() as f64;
        if vsize == 0.0 {
            return 0.0;
        }
        fee_sats as f64 / vsize
    }

    /// Check if a fee rate meets our minimum threshold.
    pub fn check_fee_rate(&self, fee_rate: f64) -> Result<(), MempoolError> {
        if fee_rate < self.config.min_fee_rate {
            return Err(MempoolError::FeeTooLow {
                fee_rate,
                min_fee_rate: self.config.min_fee_rate,
            });
        }
        Ok(())
    }

    pub fn config(&self) -> &MempoolConfig {
        &self.config
    }
}
