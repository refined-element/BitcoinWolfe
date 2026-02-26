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

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::blockdata::locktime::absolute::LockTime;
    use bitcoin::blockdata::opcodes::all::OP_RETURN;
    use bitcoin::blockdata::script::{Builder, ScriptBuf};
    use bitcoin::blockdata::transaction::{OutPoint, Sequence, TxIn, TxOut, Version};
    use bitcoin::blockdata::witness::Witness;
    use bitcoin::hashes::Hash;
    use bitcoin::{Amount, Transaction, Txid};

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    /// Build a default MempoolConfig with datacarrier enabled, 80-byte limit,
    /// and 1.0 sat/vB minimum fee rate.
    fn default_config() -> MempoolConfig {
        MempoolConfig::default()
    }

    /// Build a config with datacarrier disabled.
    fn no_datacarrier_config() -> MempoolConfig {
        MempoolConfig {
            datacarrier: false,
            ..MempoolConfig::default()
        }
    }

    /// Build a config with a custom max datacarrier byte limit.
    fn datacarrier_config(max_bytes: usize) -> MempoolConfig {
        MempoolConfig {
            datacarrier: true,
            max_datacarrier_bytes: max_bytes,
            ..MempoolConfig::default()
        }
    }

    /// Build a config with a custom minimum fee rate.
    fn fee_rate_config(min_fee_rate: f64) -> MempoolConfig {
        MempoolConfig {
            min_fee_rate,
            ..MempoolConfig::default()
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

    /// Create a minimal valid transaction with one input and one P2PKH-like output.
    fn simple_tx() -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: dummy_outpoint(0),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::default(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(50_000),
                script_pubkey: ScriptBuf::from_bytes(vec![
                    0x76, 0xa9, 0x14, // OP_DUP OP_HASH160 PUSH20
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x88,
                    0xac, // OP_EQUALVERIFY OP_CHECKSIG
                ]),
            }],
        }
    }

    /// Create a transaction with an OP_RETURN output carrying `data_len` bytes
    /// of payload. The total script length includes the OP_RETURN opcode byte
    /// and the push-data prefix byte(s).
    fn tx_with_op_return(data_len: usize) -> Transaction {
        let data = vec![0xab; data_len];
        let op_return_script = if data_len == 0 {
            Builder::new().push_opcode(OP_RETURN).into_script()
        } else {
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

    /// Create a transaction with many outputs to inflate weight beyond the
    /// 400,000 WU standard limit.
    fn oversized_tx() -> Transaction {
        // Each P2PKH-style output is about 34 bytes serialized (8 value + 1 len + 25 script).
        // In non-witness tx, every byte counts as 4 WU.
        // We need > 400,000 WU => > 100,000 non-witness bytes.
        // ~3000 outputs * 34 bytes = ~102,000 bytes => ~408,000 WU.
        let outputs: Vec<TxOut> = (0..3000)
            .map(|_| TxOut {
                value: Amount::from_sat(1000),
                script_pubkey: ScriptBuf::from_bytes(vec![
                    0x76, 0xa9, 0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0x88, 0xac,
                ]),
            })
            .collect();

        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: dummy_outpoint(0),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::default(),
            }],
            output: outputs,
        }
    }

    /// Create a transaction with distinct inputs so each call produces a
    /// unique txid. `index` varies the prevout vout.
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

    // -----------------------------------------------------------------------
    // PolicyEngine::check -- normal transactions
    // -----------------------------------------------------------------------

    #[test]
    fn check_accepts_simple_transaction() {
        let engine = PolicyEngine::new(default_config());
        let tx = simple_tx();
        assert!(engine.check(&tx).is_ok());
    }

    #[test]
    fn check_accepts_transaction_without_outputs() {
        // A transaction with no outputs has no OP_RETURN, so policy passes
        // (consensus validity is not the policy engine's concern).
        let engine = PolicyEngine::new(default_config());
        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: dummy_outpoint(0),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::default(),
            }],
            output: vec![],
        };
        assert!(engine.check(&tx).is_ok());
    }

    // -----------------------------------------------------------------------
    // PolicyEngine::check_datacarrier -- OP_RETURN policy
    // -----------------------------------------------------------------------

    #[test]
    fn check_rejects_op_return_when_datacarrier_disabled() {
        let engine = PolicyEngine::new(no_datacarrier_config());
        let tx = tx_with_op_return(10);
        let err = engine.check(&tx).unwrap_err();
        assert!(
            matches!(err, MempoolError::DatacarrierDisabled),
            "expected DatacarrierDisabled, got: {err:?}"
        );
    }

    #[test]
    fn check_accepts_op_return_when_datacarrier_enabled_within_limit() {
        // Default limit is 80 bytes. A 10-byte payload produces a script of
        // 1 (OP_RETURN) + 1 (push len) + 10 (data) = 12 bytes, well under 80.
        let engine = PolicyEngine::new(default_config());
        let tx = tx_with_op_return(10);
        assert!(engine.check(&tx).is_ok());
    }

    #[test]
    fn check_rejects_op_return_exceeding_datacarrier_limit() {
        // With a max of 20 bytes, a 30-byte payload script will exceed it.
        let config = datacarrier_config(20);
        let engine = PolicyEngine::new(config);
        let tx = tx_with_op_return(30);
        let err = engine.check(&tx).unwrap_err();
        match err {
            MempoolError::DatacarrierTooLarge { size, max } => {
                assert_eq!(max, 20);
                assert!(
                    size > 20,
                    "size should exceed max, got size={size}, max={max}"
                );
            }
            other => panic!("expected DatacarrierTooLarge, got: {other:?}"),
        }
    }

    #[test]
    fn check_accepts_op_return_exactly_at_limit() {
        // Build the tx first to measure the actual script length,
        // then set the limit to match exactly.
        let tx = tx_with_op_return(40);
        let actual_script_len = tx.output[1].script_pubkey.len();
        let config = datacarrier_config(actual_script_len);
        let engine = PolicyEngine::new(config);
        assert!(
            engine.check(&tx).is_ok(),
            "script at exactly the limit should be accepted"
        );
    }

    #[test]
    fn check_rejects_op_return_one_byte_over_limit() {
        // Set limit so that 76 bytes data is exactly at the limit,
        // then test with 77 bytes data which will be 1 byte over.
        let script_len_at_76 = 1 + 1 + 76; // 78
        let config = datacarrier_config(script_len_at_76);
        let engine = PolicyEngine::new(config);
        // 77 bytes of data: script_len = 1 + 2 + 77 = 80
        // (push prefix changes to OP_PUSHDATA1 + 1 byte len for data >= 76)
        let tx = tx_with_op_return(77);
        let script_len = tx.output[1].script_pubkey.len();
        if script_len > script_len_at_76 {
            assert!(engine.check(&tx).is_err());
        }
    }

    #[test]
    fn check_accepts_bare_op_return_no_data() {
        // A bare OP_RETURN with no push data -- script is just 1 byte.
        let engine = PolicyEngine::new(default_config());
        let tx = tx_with_op_return(0);
        assert!(engine.check(&tx).is_ok());
    }

    #[test]
    fn check_non_op_return_outputs_ignored_when_datacarrier_disabled() {
        // A transaction with only normal outputs should pass even when
        // datacarrier is disabled.
        let engine = PolicyEngine::new(no_datacarrier_config());
        let tx = simple_tx();
        assert!(engine.check(&tx).is_ok());
    }

    #[test]
    fn check_multiple_outputs_first_op_return_fails() {
        // If datacarrier is disabled, the first OP_RETURN output should
        // cause rejection even if other outputs are normal.
        let engine = PolicyEngine::new(no_datacarrier_config());
        let op_return_script = Builder::new().push_opcode(OP_RETURN).into_script();
        let tx = Transaction {
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
        };
        assert!(matches!(
            engine.check(&tx).unwrap_err(),
            MempoolError::DatacarrierDisabled
        ));
    }

    // -----------------------------------------------------------------------
    // PolicyEngine::check_size -- weight limit
    // -----------------------------------------------------------------------

    #[test]
    fn check_rejects_oversized_transaction() {
        let engine = PolicyEngine::new(default_config());
        let tx = oversized_tx();
        let weight = tx.weight().to_wu() as usize;
        assert!(
            weight > 400_000,
            "test setup: tx weight ({weight}) should exceed 400,000 WU"
        );
        let err = engine.check(&tx).unwrap_err();
        match err {
            MempoolError::TooLarge { size } => {
                assert_eq!(size, weight);
            }
            other => panic!("expected TooLarge, got: {other:?}"),
        }
    }

    #[test]
    fn check_accepts_transaction_under_weight_limit() {
        let engine = PolicyEngine::new(default_config());
        let tx = simple_tx();
        let weight = tx.weight().to_wu() as usize;
        assert!(
            weight < 400_000,
            "test setup: simple tx weight ({weight}) should be under 400,000 WU"
        );
        assert!(engine.check(&tx).is_ok());
    }

    // -----------------------------------------------------------------------
    // PolicyEngine::fee_rate_sat_per_vb
    // -----------------------------------------------------------------------

    #[test]
    fn fee_rate_calculation_basic() {
        let engine = PolicyEngine::new(default_config());
        let tx = simple_tx();
        let vsize = tx.weight().to_vbytes_ceil() as f64;

        let fee = 1000u64;
        let rate = engine.fee_rate_sat_per_vb(&tx, fee);
        let expected = fee as f64 / vsize;
        assert!(
            (rate - expected).abs() < 0.001,
            "fee rate should be {expected}, got {rate}"
        );
    }

    #[test]
    fn fee_rate_zero_fee_returns_zero() {
        let engine = PolicyEngine::new(default_config());
        let tx = simple_tx();
        let rate = engine.fee_rate_sat_per_vb(&tx, 0);
        assert!(
            rate.abs() < f64::EPSILON,
            "zero fee should yield zero rate, got {rate}"
        );
    }

    #[test]
    fn fee_rate_high_fee() {
        let engine = PolicyEngine::new(default_config());
        let tx = simple_tx();
        let fee = 1_000_000u64;
        let rate = engine.fee_rate_sat_per_vb(&tx, fee);
        assert!(rate > 1000.0, "high fee should yield high rate, got {rate}");
    }

    // -----------------------------------------------------------------------
    // PolicyEngine::check_fee_rate
    // -----------------------------------------------------------------------

    #[test]
    fn check_fee_rate_accepts_at_minimum() {
        let engine = PolicyEngine::new(fee_rate_config(1.0));
        assert!(engine.check_fee_rate(1.0).is_ok());
    }

    #[test]
    fn check_fee_rate_accepts_above_minimum() {
        let engine = PolicyEngine::new(fee_rate_config(1.0));
        assert!(engine.check_fee_rate(5.0).is_ok());
    }

    #[test]
    fn check_fee_rate_rejects_below_minimum() {
        let engine = PolicyEngine::new(fee_rate_config(1.0));
        let err = engine.check_fee_rate(0.5).unwrap_err();
        match err {
            MempoolError::FeeTooLow {
                fee_rate,
                min_fee_rate,
            } => {
                assert!((fee_rate - 0.5).abs() < f64::EPSILON);
                assert!((min_fee_rate - 1.0).abs() < f64::EPSILON);
            }
            other => panic!("expected FeeTooLow, got: {other:?}"),
        }
    }

    #[test]
    fn check_fee_rate_rejects_zero() {
        let engine = PolicyEngine::new(fee_rate_config(1.0));
        assert!(engine.check_fee_rate(0.0).is_err());
    }

    #[test]
    fn check_fee_rate_with_custom_minimum() {
        let engine = PolicyEngine::new(fee_rate_config(5.0));
        assert!(engine.check_fee_rate(4.9).is_err());
        assert!(engine.check_fee_rate(5.0).is_ok());
        assert!(engine.check_fee_rate(5.1).is_ok());
    }

    // -----------------------------------------------------------------------
    // PolicyEngine::config accessor
    // -----------------------------------------------------------------------

    #[test]
    fn config_accessor_returns_correct_config() {
        let config = fee_rate_config(3.5);
        let engine = PolicyEngine::new(config.clone());
        assert!((engine.config().min_fee_rate - 3.5).abs() < f64::EPSILON);
        assert_eq!(engine.config().max_datacarrier_bytes, 80);
    }

    // -----------------------------------------------------------------------
    // Integration: check() runs both datacarrier and size checks
    // -----------------------------------------------------------------------

    #[test]
    fn check_runs_datacarrier_before_size() {
        // An oversized transaction with an OP_RETURN and datacarrier disabled:
        // the datacarrier check runs first, so we should get DatacarrierDisabled.
        let config = MempoolConfig {
            datacarrier: false,
            ..MempoolConfig::default()
        };
        let engine = PolicyEngine::new(config);

        let op_return_script = Builder::new().push_opcode(OP_RETURN).into_script();
        // Build a large tx with an OP_RETURN output
        let mut tx = oversized_tx();
        tx.output.push(TxOut {
            value: Amount::from_sat(0),
            script_pubkey: op_return_script,
        });

        let err = engine.check(&tx).unwrap_err();
        assert!(
            matches!(err, MempoolError::DatacarrierDisabled),
            "datacarrier check should fire before size check, got: {err:?}"
        );
    }
}
