use std::sync::Arc;

use bitcoin::secp256k1::Secp256k1;
use lightning::chain::chaininterface::BroadcasterInterface;
use lightning::events::bump_transaction::BumpTransactionEvent;
use lightning::events::{Event, PaymentPurpose};
use lightning::sign::{KeysManager, OutputSpender, SignerProvider};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::broadcaster::WolfeBroadcaster;
use crate::fee_estimator::WolfeFeeEstimator;
use crate::types::WolfeChannelManager;
use wolfe_types::config::LightningConfig;

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

/// Context required by the event handler to act on LDK events.
pub(crate) struct EventContext {
    pub channel_manager: Arc<WolfeChannelManager>,
    pub keys_manager: Arc<KeysManager>,
    pub broadcaster: Arc<WolfeBroadcaster>,
    pub fee_estimator: Arc<WolfeFeeEstimator>,
    pub kv_store: Arc<crate::persister::WolfeKVStore>,
    pub config: LightningConfig,
    #[allow(dead_code)]
    pub network: bitcoin::Network,
}

impl EventContext {
    /// Persist channel manager immediately after a critical state change.
    fn persist_channel_manager(&self) {
        use lightning::util::ser::Writeable;
        let buf = self.channel_manager.encode();
        if let Err(e) = lightning::util::persist::KVStoreSync::write(
            self.kv_store.as_ref(),
            "channel_manager",
            "",
            "manager",
            buf,
        ) {
            warn!(?e, "failed to persist channel manager after state change");
        }
    }
}

/// Handle LDK events. Called by the background processor.
pub(crate) async fn handle_ldk_event(
    event: Event,
    ctx: &EventContext,
    event_tx: &mpsc::Sender<LightningEvent>,
) {
    match event {
        Event::FundingGenerationReady {
            temporary_channel_id,
            counterparty_node_id,
            channel_value_satoshis,
            ..
        } => {
            // Wallet funding not yet implemented — cancel the channel so it doesn't hang
            warn!(
                channel_value = channel_value_satoshis,
                counterparty = %counterparty_node_id,
                "FundingGenerationReady: wallet funding not yet implemented, cancelling channel"
            );
            let _ = ctx.channel_manager.force_close_broadcasting_latest_txn(
                &temporary_channel_id,
                &counterparty_node_id,
                "funding generation not supported yet".to_string(),
            );
        }

        Event::OpenChannelRequest {
            temporary_channel_id,
            counterparty_node_id,
            ..
        } => {
            if ctx.config.accept_inbound_channels {
                info!(
                    counterparty = %counterparty_node_id,
                    "accepting inbound channel request"
                );
                let user_channel_id: u128 = rand::random();
                if let Err(e) = ctx.channel_manager.accept_inbound_channel(
                    &temporary_channel_id,
                    &counterparty_node_id,
                    user_channel_id,
                    None,
                ) {
                    warn!(?e, "failed to accept inbound channel");
                }
            } else {
                info!(
                    counterparty = %counterparty_node_id,
                    "rejecting inbound channel (accept_inbound_channels=false)"
                );
            }
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

            match purpose {
                PaymentPurpose::Bolt11InvoicePayment {
                    payment_preimage, ..
                } => {
                    if let Some(preimage) = payment_preimage {
                        debug!(hash = %payment_hash, "auto-claiming with known preimage");
                        ctx.channel_manager.claim_funds(preimage);
                    } else {
                        warn!(hash = %payment_hash, "payment claimable but no preimage available");
                    }
                }
                PaymentPurpose::SpontaneousPayment(preimage) => {
                    debug!(hash = %payment_hash, "spontaneous payment — auto-claiming");
                    ctx.channel_manager.claim_funds(preimage);
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
            info!(
                count = outputs.len(),
                channel = ?channel_id,
                "sweeping spendable outputs"
            );

            let destination = match ctx.keys_manager.get_destination_script([0u8; 32]) {
                Ok(script) => script,
                Err(_) => {
                    warn!("failed to get destination script for sweeping");
                    return;
                }
            };

            let fee_rate = ctx
                .fee_estimator
                .sweep_fee_rate();

            let descriptor_refs: Vec<_> = outputs.iter().collect();
            let secp = Secp256k1::new();

            match ctx.keys_manager.spend_spendable_outputs(
                &descriptor_refs,
                Vec::new(),
                destination,
                fee_rate,
                None,
                &secp,
            ) {
                Ok(tx) => {
                    info!(txid = %tx.compute_txid(), "broadcasting sweep transaction");
                    ctx.broadcaster
                        .broadcast_transactions(&[&tx]);
                }
                Err(_) => {
                    warn!("failed to create sweep transaction for spendable outputs");
                }
            }
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
            ctx.persist_channel_manager();
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
            ctx.persist_channel_manager();
            let _ = event_tx
                .send(LightningEvent::ChannelClosed {
                    channel_id: hex::encode(channel_id.0),
                    reason: reason_str,
                })
                .await;
        }

        Event::BumpTransaction(bump) => {
            match &bump {
                BumpTransactionEvent::ChannelClose { .. } => {
                    warn!("BumpTransaction(ChannelClose) received — anchor bumping not yet supported");
                }
                BumpTransactionEvent::HTLCResolution { .. } => {
                    warn!("BumpTransaction(HTLCResolution) received — anchor bumping not yet supported");
                }
            }
        }

        _ => {
            debug!(?event, "unhandled LDK event");
        }
    }
}
