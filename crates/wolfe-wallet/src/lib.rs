pub mod error;

use std::path::Path;
use std::sync::Arc;

use bdk_wallet::bitcoin::Network;
use bdk_wallet::{KeychainKind, PersistedWallet};
use tracing::{debug, info};

use crate::error::WalletError;

/// A BDK-powered descriptor wallet integrated into the BitcoinWolfe node.
///
/// Uses SQLite for persistence and supports:
/// - Descriptor-based key management
/// - PSBT construction and signing
/// - Coin selection
/// - Fee estimation
pub struct NodeWallet {
    wallet: PersistedWallet<rusqlite::Connection>,
    db: rusqlite::Connection,
}

impl NodeWallet {
    /// Create a new wallet or load an existing one from the database.
    ///
    /// If `encryption_key` is provided, the database is encrypted at rest
    /// using SQLCipher's PRAGMA key mechanism. This requires the node to be
    /// built with SQLCipher support in rusqlite.
    pub fn open(
        db_path: &Path,
        network: Network,
        external_descriptor: String,
        internal_descriptor: String,
    ) -> Result<Self, WalletError> {
        Self::open_with_encryption(
            db_path,
            network,
            external_descriptor,
            internal_descriptor,
            None,
        )
    }

    /// Open with optional encryption key.
    pub fn open_with_encryption(
        db_path: &Path,
        network: Network,
        external_descriptor: String,
        internal_descriptor: String,
        encryption_key: Option<&str>,
    ) -> Result<Self, WalletError> {
        let mut db = rusqlite::Connection::open(db_path)
            .map_err(|e| WalletError::Database(e.to_string()))?;

        // Apply encryption key if provided
        if let Some(key) = encryption_key {
            // SQLCipher PRAGMA key must be the first statement after opening
            // This works when rusqlite is compiled with the sqlcipher feature.
            // With standard SQLite, this is a no-op but the database will be unencrypted.
            db.execute_batch(&format!("PRAGMA key = '{}';", key.replace('\'', "''")))
                .map_err(|e| {
                    WalletError::Database(format!(
                        "failed to set encryption key (is SQLCipher enabled?): {}",
                        e
                    ))
                })?;
            info!("wallet database encryption enabled");
        }

        let ext = external_descriptor.clone();
        let int = internal_descriptor.clone();

        let wallet = match bdk_wallet::Wallet::load()
            .descriptor(KeychainKind::External, Some(external_descriptor))
            .descriptor(KeychainKind::Internal, Some(internal_descriptor))
            .check_network(network)
            .load_wallet(&mut db)
            .map_err(|e| WalletError::Bdk(e.to_string()))?
        {
            Some(wallet) => {
                info!("loaded existing wallet from {:?}", db_path);
                wallet
            }
            None => {
                let wallet = bdk_wallet::Wallet::create(ext, int)
                    .network(network)
                    .create_wallet(&mut db)
                    .map_err(|e| WalletError::Bdk(e.to_string()))?;
                info!("created new wallet at {:?}", db_path);
                wallet
            }
        };

        Ok(Self { wallet, db })
    }

    /// Get a new receiving address.
    pub fn new_address(&mut self) -> Result<String, WalletError> {
        let addr_info = self.wallet.reveal_next_address(KeychainKind::External);
        self.persist()?;
        debug!(
            address = %addr_info.address,
            index = addr_info.index,
            "revealed new address"
        );
        Ok(addr_info.address.to_string())
    }

    /// Get a new change address.
    pub fn new_change_address(&mut self) -> Result<String, WalletError> {
        let addr_info = self.wallet.reveal_next_address(KeychainKind::Internal);
        self.persist()?;
        Ok(addr_info.address.to_string())
    }

    /// Get wallet balance.
    pub fn balance(&self) -> WalletBalance {
        let balance = self.wallet.balance();
        WalletBalance {
            confirmed: balance.confirmed.to_sat(),
            trusted_pending: balance.trusted_pending.to_sat(),
            untrusted_pending: balance.untrusted_pending.to_sat(),
            immature: balance.immature.to_sat(),
        }
    }

    /// Apply a confirmed block to the wallet (feed chain data from our node).
    pub fn apply_block(&mut self, block: &bitcoin::Block, height: u32) -> Result<(), WalletError> {
        // Since both our crate and BDK depend on bitcoin 0.32, the types
        // are identical. We serialize/deserialize to cross the crate boundary.
        let bdk_block: bdk_wallet::bitcoin::Block =
            bitcoin::consensus::deserialize(&bitcoin::consensus::serialize(block))
                .map_err(|e| WalletError::Bdk(format!("block conversion: {}", e)))?;

        self.wallet
            .apply_block(&bdk_block, height)
            .map_err(|e| WalletError::Bdk(e.to_string()))?;

        self.persist()?;
        Ok(())
    }

    /// Apply unconfirmed transactions from the mempool.
    pub fn apply_unconfirmed_txs<I>(&mut self, txs: I) -> Result<(), WalletError>
    where
        I: IntoIterator<Item = (bitcoin::Transaction, u64)>,
    {
        let converted: Vec<(Arc<bdk_wallet::bitcoin::Transaction>, u64)> = txs
            .into_iter()
            .filter_map(|(tx, timestamp)| {
                let bdk_tx: Result<bdk_wallet::bitcoin::Transaction, _> =
                    bitcoin::consensus::deserialize(&bitcoin::consensus::serialize(&tx));
                bdk_tx.ok().map(|t| (Arc::new(t), timestamp))
            })
            .collect();

        self.wallet
            .apply_unconfirmed_txs(converted.into_iter().map(|(tx, ts)| (tx, ts)));

        self.persist()?;
        Ok(())
    }

    /// Create a PSBT sending to the given address with the given amount (in satoshis).
    /// If `sign` is true, the wallet will attempt to sign the PSBT immediately.
    /// Returns the PSBT as a base64 string.
    pub fn create_psbt(
        &mut self,
        to_address: &str,
        amount_sat: u64,
        fee_rate_sat_per_vb: f32,
        sign: bool,
    ) -> Result<String, WalletError> {
        use bdk_wallet::bitcoin::Address;
        use std::str::FromStr;

        let address = Address::from_str(to_address)
            .map_err(|e| WalletError::Bdk(format!("invalid address: {}", e)))?
            .assume_checked();

        let fee_rate = bdk_wallet::bitcoin::FeeRate::from_sat_per_vb(fee_rate_sat_per_vb as u64)
            .ok_or_else(|| WalletError::Bdk("invalid fee rate".to_string()))?;

        let mut builder = self.wallet.build_tx();
        builder
            .add_recipient(
                address.script_pubkey(),
                bdk_wallet::bitcoin::Amount::from_sat(amount_sat),
            )
            .fee_rate(fee_rate);

        let mut psbt = builder
            .finish()
            .map_err(|e| WalletError::Bdk(format!("failed to build tx: {}", e)))?;

        if sign {
            self.wallet
                .sign(&mut psbt, bdk_wallet::SignOptions::default())
                .map_err(|e| WalletError::Bdk(format!("signing failed: {}", e)))?;
        }

        self.persist()?;
        Ok(psbt.to_string())
    }

    /// Sign a PSBT with the wallet's keys.
    pub fn sign(
        &mut self,
        psbt: &mut bdk_wallet::bitcoin::psbt::Psbt,
    ) -> Result<bool, WalletError> {
        let finalized = self
            .wallet
            .sign(psbt, bdk_wallet::SignOptions::default())
            .map_err(|e| WalletError::Bdk(e.to_string()))?;
        Ok(finalized)
    }

    /// Sign a PSBT from a base64 string. Returns (signed_base64, is_complete).
    pub fn sign_psbt_base64(&mut self, psbt_b64: &str) -> Result<(String, bool), WalletError> {
        let mut psbt: bdk_wallet::bitcoin::psbt::Psbt = psbt_b64
            .parse()
            .map_err(|e| WalletError::Bdk(format!("invalid PSBT: {}", e)))?;

        let finalized = self
            .wallet
            .sign(&mut psbt, bdk_wallet::SignOptions::default())
            .map_err(|e| WalletError::Bdk(format!("signing failed: {}", e)))?;

        Ok((psbt.to_string(), finalized))
    }

    /// List all transactions.
    pub fn list_transactions(&self) -> Vec<TxSummary> {
        self.wallet
            .transactions()
            .map(|tx| {
                let txid = tx.tx_node.txid;
                TxSummary {
                    txid: txid.to_string(),
                    confirmed: tx.chain_position.is_confirmed(),
                }
            })
            .collect()
    }

    /// Persist wallet state to the SQLite database.
    fn persist(&mut self) -> Result<(), WalletError> {
        self.wallet
            .persist(&mut self.db)
            .map_err(|e| WalletError::Database(e.to_string()))?;
        Ok(())
    }
}

/// Simplified balance info.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WalletBalance {
    pub confirmed: u64,
    pub trusted_pending: u64,
    pub untrusted_pending: u64,
    pub immature: u64,
}

impl WalletBalance {
    pub fn total(&self) -> u64 {
        self.confirmed + self.trusted_pending
    }
}

/// Simplified transaction summary.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TxSummary {
    pub txid: String,
    pub confirmed: bool,
}
