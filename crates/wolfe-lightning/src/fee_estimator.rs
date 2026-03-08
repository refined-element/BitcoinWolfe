use std::sync::Arc;

use lightning::chain::chaininterface::{ConfirmationTarget, FeeEstimator};
use wolfe_mempool::Mempool;

/// Bridges LDK's FeeEstimator to our mempool's fee histogram.
///
/// Maps `ConfirmationTarget` variants to mempool percentiles, then converts
/// sat/vB to sat/kw (multiply by 250). Floors at 253 sat/kw (LDK minimum).
pub struct WolfeFeeEstimator {
    mempool: Arc<Mempool>,
}

impl WolfeFeeEstimator {
    pub fn new(mempool: Arc<Mempool>) -> Self {
        Self { mempool }
    }

    /// Get a fee rate suitable for sweeping spendable outputs (sat/kw).
    pub fn sweep_fee_rate(&self) -> u32 {
        self.get_est_sat_per_1000_weight(ConfirmationTarget::OutputSpendingFee)
    }

    /// Sample fee rate from mempool histogram at a given percentile (0.0 = lowest, 1.0 = highest).
    fn sample_fee_rate_sat_per_vb(&self, percentile: f64) -> f64 {
        let histogram = self.mempool.fee_histogram();
        if histogram.is_empty() {
            return self.mempool.min_fee_rate();
        }

        // histogram is [(fee_rate, count)] in descending order
        let total_txs: usize = histogram.iter().map(|(_, count)| count).sum();
        if total_txs == 0 {
            return self.mempool.min_fee_rate();
        }

        let target_index = ((total_txs as f64) * percentile).ceil() as usize;
        let mut cumulative = 0usize;

        for &(fee_rate, count) in &histogram {
            cumulative += count;
            if cumulative >= target_index {
                return fee_rate;
            }
        }

        // Fallback: return the lowest bucket
        histogram.last().map(|(rate, _)| *rate).unwrap_or(1.0)
    }
}

impl FeeEstimator for WolfeFeeEstimator {
    fn get_est_sat_per_1000_weight(&self, target: ConfirmationTarget) -> u32 {
        let sat_per_vb = match target {
            // Sanity-check ceiling for counterparty feerates
            ConfirmationTarget::MaximumFeeEstimate => self.sample_fee_rate_sat_per_vb(0.05),

            // High priority: HTLC resolution / on-chain sweep
            ConfirmationTarget::UrgentOnChainSweep => self.sample_fee_rate_sat_per_vb(0.1),

            // Medium priority: anchor channel fee bumping
            ConfirmationTarget::AnchorChannelFee => self.sample_fee_rate_sat_per_vb(0.5),

            // Normal priority: non-anchor channel close
            ConfirmationTarget::NonAnchorChannelFee => self.sample_fee_rate_sat_per_vb(0.3),

            // Low priority: channel close minimum
            ConfirmationTarget::ChannelCloseMinimum => self.sample_fee_rate_sat_per_vb(0.9),

            // Minimum mempool fee
            ConfirmationTarget::MinAllowedAnchorChannelRemoteFee => self.mempool.min_fee_rate(),

            // Non-exhaustive: default to minimum
            ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee => self.mempool.min_fee_rate(),

            // Output spending: medium-low priority
            ConfirmationTarget::OutputSpendingFee => self.sample_fee_rate_sat_per_vb(0.6),
        };

        // Convert sat/vB to sat/kw: multiply by 250 (1 vbyte = 4 weight units, 1000/4 = 250)
        let sat_per_kw = (sat_per_vb * 250.0) as u32;

        // Safety floors per target — protects against empty mempool during IBD
        // returning 0 sat/vB, which would cause LDK to create unbroadcastable txs.
        let floor = match target {
            ConfirmationTarget::MaximumFeeEstimate => 50_000,  // 200 sat/vB
            ConfirmationTarget::UrgentOnChainSweep => 5_000,   // 20 sat/vB
            ConfirmationTarget::NonAnchorChannelFee => 3_000,  // 12 sat/vB
            ConfirmationTarget::AnchorChannelFee => 1_000,     // 4 sat/vB
            ConfirmationTarget::ChannelCloseMinimum => 1_000,  // 4 sat/vB
            _ => 253,                                          // 1 sat/vB minimum
        };

        std::cmp::max(sat_per_kw, floor)
    }
}
