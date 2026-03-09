use std::sync::Arc;

use bitcoin::Transaction;
use lightning::chain::chaininterface::{ConfirmationTarget, FeeEstimator};
use wolfe_mempool::Mempool;
use wolfe_types::config::MempoolConfig;

use wolfe_lightning::fee_estimator::WolfeFeeEstimator;

fn make_mempool() -> Arc<Mempool> {
    let config = MempoolConfig {
        min_fee_rate: 1.0,
        ..MempoolConfig::default()
    };
    Arc::new(Mempool::new(config))
}

/// Create a minimal valid transaction to add to the mempool.
fn dummy_tx(version: i32) -> Transaction {
    use bitcoin::transaction::{TxIn, TxOut};
    use bitcoin::Amount;
    Transaction {
        version: bitcoin::transaction::Version(version),
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![TxIn::default()],
        output: vec![TxOut {
            value: Amount::from_sat(50_000),
            script_pubkey: bitcoin::ScriptBuf::new(),
        }],
    }
}

#[test]
fn empty_mempool_returns_floor() {
    let mempool = make_mempool();
    let estimator = WolfeFeeEstimator::new(mempool);

    // With empty mempool, targets return their safety floor (not raw 253)
    // ChannelCloseMinimum floor is 1000 sat/kw (4 sat/vB)
    let fee = estimator.get_est_sat_per_1000_weight(ConfirmationTarget::ChannelCloseMinimum);
    assert_eq!(
        fee, 1000,
        "empty mempool should return ChannelCloseMinimum floor of 1000 sat/kw"
    );

    // MinAllowed targets have no elevated floor (just 253 sat/kw)
    let fee =
        estimator.get_est_sat_per_1000_weight(ConfirmationTarget::MinAllowedAnchorChannelRemoteFee);
    assert_eq!(fee, 253, "MinAllowed target should return 253 sat/kw floor");
}

#[test]
fn minimum_floor_enforced() {
    let mempool = make_mempool();
    let estimator = WolfeFeeEstimator::new(mempool);

    // Even with zero-fee mempool, floor should be 253
    for target in [
        ConfirmationTarget::MaximumFeeEstimate,
        ConfirmationTarget::UrgentOnChainSweep,
        ConfirmationTarget::AnchorChannelFee,
        ConfirmationTarget::NonAnchorChannelFee,
        ConfirmationTarget::ChannelCloseMinimum,
        ConfirmationTarget::MinAllowedAnchorChannelRemoteFee,
        ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee,
        ConfirmationTarget::OutputSpendingFee,
    ] {
        let fee = estimator.get_est_sat_per_1000_weight(target);
        assert!(
            fee >= 253,
            "target {:?} returned {} which is below LDK minimum 253",
            target,
            fee
        );
    }
}

#[test]
fn higher_priority_yields_higher_fee() {
    let mempool = make_mempool();

    // Add transactions with varying fees to populate the histogram
    // Fees: 50_000 sat for a ~100 vbyte tx = ~500 sat/vB
    for i in 0..20 {
        let tx = dummy_tx(i + 1);
        // Each tx has different fee to spread across histogram buckets
        let fee = ((i as u64) + 1) * 5000;
        let _ = mempool.add(tx, fee);
    }

    let estimator = WolfeFeeEstimator::new(mempool);

    let high = estimator.get_est_sat_per_1000_weight(ConfirmationTarget::UrgentOnChainSweep);
    let low = estimator.get_est_sat_per_1000_weight(ConfirmationTarget::ChannelCloseMinimum);

    // Higher priority (UrgentOnChainSweep) should yield >= lower priority (ChannelCloseMinimum)
    assert!(
        high >= low,
        "UrgentOnChainSweep ({}) should be >= ChannelCloseMinimum ({})",
        high,
        low
    );
}

#[test]
fn sat_per_vb_to_sat_per_kw_conversion() {
    // Conversion formula: sat/vB * 250 = sat/kw
    // If mempool returns 10 sat/vB → 2500 sat/kw
    // If mempool returns 1 sat/vB → 250 sat/kw → clamped to 253

    let mempool = make_mempool();
    let estimator = WolfeFeeEstimator::new(mempool);

    // With min_fee_rate = 1.0 sat/vB and empty mempool:
    // 1.0 * 250 = 250 → clamped to 253
    let fee =
        estimator.get_est_sat_per_1000_weight(ConfirmationTarget::MinAllowedAnchorChannelRemoteFee);
    assert_eq!(fee, 253);
}

#[test]
fn all_confirmation_targets_return_valid_fee() {
    let mempool = make_mempool();
    let estimator = WolfeFeeEstimator::new(mempool);

    let targets = [
        ConfirmationTarget::MaximumFeeEstimate,
        ConfirmationTarget::UrgentOnChainSweep,
        ConfirmationTarget::AnchorChannelFee,
        ConfirmationTarget::NonAnchorChannelFee,
        ConfirmationTarget::ChannelCloseMinimum,
        ConfirmationTarget::MinAllowedAnchorChannelRemoteFee,
        ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee,
        ConfirmationTarget::OutputSpendingFee,
    ];

    for target in targets {
        let fee = estimator.get_est_sat_per_1000_weight(target);
        assert!(fee > 0, "target {:?} returned zero fee", target);
        assert!(
            fee >= 253,
            "target {:?} returned {} below minimum",
            target,
            fee
        );
    }
}
