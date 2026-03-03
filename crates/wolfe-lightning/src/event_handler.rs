use lightning::events::{Event, PaymentPurpose};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Events emitted by the Lightning subsystem to the main event loop.
#[derive(Debug)]
pub enum LightningEvent {
    /// A payment was successfully received.
    PaymentReceived {
        payment_hash: String,
        amount_msat: u64,
    },
    /// A payment we sent was successfully delivered.
    PaymentSent {
        payment_hash: String,
        fee_paid_msat: Option<u64>,
    },
    /// A payment we sent failed.
    PaymentFailed { payment_hash: String },
    /// A channel was opened.
    ChannelOpened {
        channel_id: String,
        counterparty_node_id: String,
        capacity_sat: u64,
    },
    /// A channel was closed.
    ChannelClosed { channel_id: String, reason: String },
}

/// Handle LDK events. Called by the background processor.
pub async fn handle_ldk_event(event: Event, event_tx: &mpsc::Sender<LightningEvent>) {
    match event {
        Event::FundingGenerationReady {
            temporary_channel_id,
            counterparty_node_id,
            channel_value_satoshis,
            output_script,
            ..
        } => {
            // TODO: Create funding transaction via wallet, then call
            // channel_manager.funding_transaction_generated()
            info!(
                channel_value = channel_value_satoshis,
                counterparty = %counterparty_node_id,
                "funding generation ready — wallet integration needed"
            );
            let _ = temporary_channel_id;
            let _ = output_script;
        }

        Event::PaymentClaimable {
            payment_hash,
            amount_msat,
            purpose,
            ..
        } => {
            info!(
                hash = %payment_hash,
                amount_msat,
                "payment claimable"
            );

            // Auto-claim for known invoices
            match purpose {
                PaymentPurpose::Bolt11InvoicePayment {
                    payment_preimage, ..
                } => {
                    if let Some(preimage) = payment_preimage {
                        debug!(hash = %payment_hash, "auto-claiming with known preimage");
                        // The actual claim happens via channel_manager.claim_funds()
                        // which must be called from the LightningManager
                        let _ = preimage;
                    }
                }
                PaymentPurpose::SpontaneousPayment(preimage) => {
                    debug!(hash = %payment_hash, "spontaneous payment — auto-claiming");
                    let _ = preimage;
                }
                _ => {
                    debug!(hash = %payment_hash, "payment claimable with unknown purpose");
                }
            }
        }

        Event::PaymentClaimed {
            payment_hash,
            amount_msat,
            ..
        } => {
            info!(
                hash = %payment_hash,
                amount_msat,
                "payment claimed"
            );
            let _ = event_tx
                .send(LightningEvent::PaymentReceived {
                    payment_hash: payment_hash.to_string(),
                    amount_msat,
                })
                .await;
        }

        Event::PaymentSent {
            payment_hash,
            fee_paid_msat,
            ..
        } => {
            info!(
                hash = %payment_hash,
                fee_msat = ?fee_paid_msat,
                "payment sent"
            );
            let _ = event_tx
                .send(LightningEvent::PaymentSent {
                    payment_hash: payment_hash.to_string(),
                    fee_paid_msat,
                })
                .await;
        }

        Event::PaymentFailed { payment_hash, .. } => {
            let hash_str = payment_hash
                .map(|h| h.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            warn!(hash = %hash_str, "payment failed");
            let _ = event_tx
                .send(LightningEvent::PaymentFailed {
                    payment_hash: hash_str,
                })
                .await;
        }

        Event::SpendableOutputs {
            outputs,
            channel_id,
        } => {
            // TODO: Sweep spendable outputs to wallet
            info!(
                count = outputs.len(),
                channel = ?channel_id,
                "spendable outputs available — sweep needed"
            );
        }

        Event::ChannelReady {
            channel_id,
            counterparty_node_id,
            ..
        } => {
            info!(
                channel = %hex::encode(channel_id.0),
                counterparty = %counterparty_node_id,
                "channel ready"
            );
        }

        Event::ChannelClosed {
            channel_id, reason, ..
        } => {
            let reason_str = format!("{:?}", reason);
            info!(
                channel = %hex::encode(channel_id.0),
                reason = %reason_str,
                "channel closed"
            );
            let _ = event_tx
                .send(LightningEvent::ChannelClosed {
                    channel_id: hex::encode(channel_id.0),
                    reason: reason_str,
                })
                .await;
        }

        _ => {
            debug!(?event, "unhandled LDK event");
        }
    }
}
