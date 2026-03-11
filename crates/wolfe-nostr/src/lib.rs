pub mod error;
pub mod events;
pub mod nip98;

use std::sync::Arc;
use std::time::Duration;

use nostr_sdk::prelude::*;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use wolfe_mempool::Mempool;

use crate::error::NostrError;

/// A message sent from the main event loop to the Nostr bridge.
#[derive(Debug)]
pub enum NostrEvent {
    /// A new block was validated.
    BlockValidated {
        height: u64,
        hash: String,
        timestamp: u64,
        tx_count: usize,
        size: usize,
    },
    /// A Lightning channel was opened.
    LightningChannelOpened {
        channel_id: String,
        counterparty: String,
        capacity_sat: u64,
    },
    /// A Lightning payment was received.
    LightningPaymentReceived {
        payment_hash: String,
        amount_msat: u64,
    },
}

/// The Nostr bridge: connects to relays and publishes node events.
pub struct NostrBridge {
    client: Arc<Client>,
    network: String,
    event_rx: mpsc::Receiver<NostrEvent>,
    mempool: Arc<Mempool>,
    fee_oracle_interval_secs: u64,
    keys: Keys,
    profile_name: Option<String>,
    profile_about: Option<String>,
}

/// Handle for sending events to the Nostr bridge from the main loop.
#[derive(Clone)]
pub struct NostrSender {
    tx: mpsc::Sender<NostrEvent>,
}

impl NostrSender {
    pub async fn send(&self, event: NostrEvent) {
        if let Err(e) = self.tx.send(event).await {
            debug!("nostr channel closed: {}", e);
        }
    }
}

impl NostrBridge {
    /// Create a new Nostr bridge.
    ///
    /// If `secret_key` is provided (hex or nsec), uses that identity.
    /// Otherwise generates an ephemeral keypair (logged at startup so the user
    /// can find their npub).
    ///
    /// Returns `(bridge, sender, client)` where `client` is a shared handle
    /// that can be used by RPC handlers to publish events or query relays.
    pub async fn new(
        secret_key: Option<&str>,
        relays: &[String],
        network: String,
        mempool: Arc<Mempool>,
        fee_oracle_interval_secs: u64,
        profile_name: Option<String>,
        profile_about: Option<String>,
    ) -> Result<(Self, NostrSender, Arc<Client>), NostrError> {
        let keys = match secret_key {
            Some(sk) => Keys::parse(sk).map_err(|e| NostrError::InvalidKey(e.to_string()))?,
            None => {
                let keys = Keys::generate();
                info!(
                    npub = %keys.public_key().to_bech32().unwrap_or_default(),
                    "generated ephemeral Nostr identity (set nostr.secret_key to persist)"
                );
                keys
            }
        };

        let client = Arc::new(Client::builder().signer(keys.clone()).build());

        for relay_url in relays {
            client
                .add_relay(relay_url)
                .await
                .map_err(|e| NostrError::RelayConnection(format!("{}: {}", relay_url, e)))?;
        }

        let (tx, rx) = mpsc::channel(256);
        let shared_client = client.clone();

        Ok((
            Self {
                client,
                network,
                event_rx: rx,
                mempool,
                fee_oracle_interval_secs,
                keys,
                profile_name,
                profile_about,
            },
            NostrSender { tx },
            shared_client,
        ))
    }

    /// Public key of this bridge's identity.
    pub fn public_key(&self) -> PublicKey {
        self.keys.public_key()
    }

    /// Get the bridge's keys for external use.
    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    /// Run the Nostr bridge. This blocks until the channel is closed.
    pub async fn run(mut self) {
        // Connect to all configured relays
        self.client.connect().await;
        info!(
            npub = %self.keys.public_key().to_bech32().unwrap_or_default(),
            "nostr bridge connected to relays"
        );

        // Publish profile metadata (NIP-01 kind 0) if configured
        if self.profile_name.is_some() || self.profile_about.is_some() {
            let mut metadata = Metadata::new();
            if let Some(ref name) = self.profile_name {
                metadata = metadata.name(name);
            }
            if let Some(ref about) = self.profile_about {
                metadata = metadata.about(about);
            }
            match self.client.set_metadata(&metadata).await {
                Ok(output) => {
                    info!(
                        event_id = %output.val,
                        name = ?self.profile_name,
                        "published nostr profile metadata"
                    );
                }
                Err(e) => {
                    warn!(?e, "failed to publish nostr profile metadata");
                }
            }
        }

        let mut fee_oracle_interval =
            tokio::time::interval(Duration::from_secs(self.fee_oracle_interval_secs));
        // Don't fire immediately — wait for the first tick
        fee_oracle_interval.tick().await;

        loop {
            tokio::select! {
                // Process events from the main loop
                msg = self.event_rx.recv() => {
                    match msg {
                        Some(event) => self.handle_event(event).await,
                        None => {
                            info!("nostr bridge shutting down (channel closed)");
                            break;
                        }
                    }
                }
                // Periodically publish mempool fee oracle
                _ = fee_oracle_interval.tick() => {
                    self.publish_fee_oracle().await;
                }
            }
        }

        // Disconnect gracefully
        self.client.disconnect().await;
    }

    async fn handle_event(&self, event: NostrEvent) {
        match event {
            NostrEvent::BlockValidated {
                height,
                hash,
                timestamp,
                tx_count,
                size,
            } => {
                let builder = events::block_announcement(
                    height,
                    &hash,
                    timestamp,
                    tx_count,
                    size,
                    &self.network,
                );
                match self.client.send_event_builder(builder).await {
                    Ok(output) => {
                        debug!(
                            height,
                            event_id = %output.val,
                            "published block announcement"
                        );
                    }
                    Err(e) => {
                        warn!(height, ?e, "failed to publish block announcement");
                    }
                }
            }
            NostrEvent::LightningChannelOpened {
                channel_id,
                counterparty,
                capacity_sat,
            } => {
                info!(
                    channel_id,
                    counterparty, capacity_sat, "lightning channel opened (nostr event)"
                );
            }
            NostrEvent::LightningPaymentReceived {
                payment_hash,
                amount_msat,
            } => {
                info!(
                    payment_hash,
                    amount_msat, "lightning payment received (nostr event)"
                );
            }
        }
    }

    async fn publish_fee_oracle(&self) {
        let size = self.mempool.len();
        if size == 0 {
            return; // Nothing interesting to publish
        }

        let bytes = self.mempool.total_bytes();

        // Build fee histogram from mempool data
        let fee_buckets = self.mempool.fee_histogram();
        let min_fee_rate = self.mempool.min_fee_rate();

        let builder =
            events::mempool_fee_oracle(size, bytes, min_fee_rate, &fee_buckets, &self.network);

        match self.client.send_event_builder(builder).await {
            Ok(output) => {
                debug!(
                    txs = size,
                    event_id = %output.val,
                    "published mempool fee oracle"
                );
            }
            Err(e) => {
                warn!(?e, "failed to publish mempool fee oracle");
            }
        }
    }
}
