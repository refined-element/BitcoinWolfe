use axum::extract::State;
use axum::Json;
use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::error::RpcError;
use crate::server::NodeState;

// ─── REST API Handlers ─────────────────────────────────────────────────────

/// GET /api/info - Node information
pub async fn get_info(State(state): State<Arc<NodeState>>) -> Json<Value> {
    let info = state.get_info();
    Json(json!(info))
}

/// GET /api/blockchain - Blockchain overview
pub async fn get_blockchain(State(state): State<Arc<NodeState>>) -> Json<Value> {
    Json(json!({
        "chain": state.chain,
        "blocks": state.best_height(),
        "headers": state.headers_height(),
        "best_block_hash": state.best_hash(),
        "syncing": state.is_syncing(),
    }))
}

/// GET /api/mempool - Mempool information
pub async fn get_mempool(State(state): State<Arc<NodeState>>) -> Json<Value> {
    let mempool = &state.mempool;
    Json(json!({
        "size": mempool.len(),
        "bytes": mempool.size_bytes(),
        "policy": {
            "min_fee_rate": mempool.policy().config().min_fee_rate,
            "datacarrier": mempool.policy().config().datacarrier,
            "max_datacarrier_bytes": mempool.policy().config().max_datacarrier_bytes,
            "full_rbf": mempool.policy().config().full_rbf,
        }
    }))
}

/// GET /api/peers - Connected peers
pub async fn get_peers(State(state): State<Arc<NodeState>>) -> Json<Value> {
    let peers = state.peer_infos();
    let peer_list: Vec<Value> = peers
        .iter()
        .map(|p| {
            json!({
                "addr": p.addr.to_string(),
                "user_agent": p.user_agent,
                "version": p.version,
                "inbound": p.inbound,
                "v2_transport": p.v2_transport,
                "start_height": p.start_height,
            })
        })
        .collect();

    Json(json!({
        "count": peer_list.len(),
        "peers": peer_list,
    }))
}

/// GET /api/lightning/info - Lightning node information
pub async fn get_lightning_info(State(state): State<Arc<NodeState>>) -> Json<Value> {
    match state.lightning() {
        Some(ln) => {
            let node_id = ln.node_id();
            let channels = ln.channel_manager().list_channels();
            let peers = ln.peer_manager().list_peers();
            let active = channels.iter().filter(|c| c.is_usable).count();
            Json(json!({
                "enabled": true,
                "node_id": node_id.to_string(),
                "num_channels": channels.len(),
                "num_active_channels": active,
                "num_peers": peers.len(),
            }))
        }
        None => Json(json!({ "enabled": false })),
    }
}

/// GET /api/lightning/channels - Lightning channel list
pub async fn get_lightning_channels(State(state): State<Arc<NodeState>>) -> Json<Value> {
    match state.lightning() {
        Some(ln) => {
            let channels = ln.channel_manager().list_channels();
            let result: Vec<Value> = channels
                .iter()
                .map(|c| {
                    json!({
                        "channel_id": hex::encode(c.channel_id.0),
                        "counterparty": c.counterparty.node_id.to_string(),
                        "capacity_sat": c.channel_value_satoshis,
                        "outbound_capacity_msat": c.outbound_capacity_msat,
                        "inbound_capacity_msat": c.inbound_capacity_msat,
                        "is_usable": c.is_usable,
                        "is_outbound": c.is_outbound,
                        "is_channel_ready": c.is_channel_ready,
                    })
                })
                .collect();
            Json(json!({ "channels": result }))
        }
        None => Json(json!({ "error": "lightning not enabled" })),
    }
}

// ─── JSON-RPC Handler ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: Option<String>,
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

/// POST / - JSON-RPC endpoint (Bitcoin Core compatible)
pub async fn json_rpc(
    State(state): State<Arc<NodeState>>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    let result = dispatch_rpc(&state, &req.method, req.params.as_ref()).await;

    match result {
        Ok(value) => Json(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: Some(value),
            error: None,
        }),
        Err(e) => {
            let (code, message) = match &e {
                RpcError::MethodNotFound(m) => (-32601, m.clone()),
                RpcError::InvalidParams(m) => (-32602, m.clone()),
                RpcError::Internal(m) => (-32603, m.clone()),
                RpcError::NotFound(m) => (-1, m.clone()),
                RpcError::Wallet(m) => (-4, m.clone()),
                RpcError::Lightning(m) => (-5, m.clone()),
            };
            Json(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: None,
                error: Some(json!({ "code": code, "message": message })),
            })
        }
    }
}

/// Route a JSON-RPC method to its handler.
async fn dispatch_rpc(
    state: &NodeState,
    method: &str,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    match method {
        "getblockchaininfo" => Ok(json!({
            "chain": state.chain,
            "blocks": state.best_height(),
            "headers": state.headers_height(),
            "bestblockhash": state.best_hash(),
            "initialblockdownload": state.is_syncing(),
            "warnings": "",
        })),

        "getnetworkinfo" => Ok(json!({
            "version": wolfe_types::VERSION,
            "subversion": wolfe_types::user_agent(),
            "protocolversion": 70016,
            "connections": state.peer_count(),
        })),

        "getmempoolinfo" => Ok(json!({
            "loaded": true,
            "size": state.mempool.len(),
            "bytes": state.mempool.size_bytes(),
            "mempoolminfee": state.mempool.policy().config().min_fee_rate / 100_000.0,
        })),

        "getpeerinfo" => {
            let peers: Vec<Value> = state
                .peer_infos()
                .iter()
                .map(|p| {
                    json!({
                        "addr": p.addr.to_string(),
                        "subver": p.user_agent,
                        "version": p.version,
                        "inbound": p.inbound,
                        "startingheight": p.start_height,
                    })
                })
                .collect();
            Ok(json!(peers))
        }

        "getblockcount" => Ok(json!(state.best_height())),

        "getbestblockhash" => Ok(json!(state.best_hash())),

        "getblock" => {
            let blockhash = get_param_str(params, 0)
                .ok_or_else(|| RpcError::InvalidParams("missing blockhash".to_string()))?;
            let verbosity = get_param_i64(params, 1).unwrap_or(1);

            let engine = state
                .consensus()
                .ok_or_else(|| RpcError::Internal("consensus engine not available".to_string()))?;

            // Parse the hex hash
            let hash_bytes: [u8; 32] = hex::decode(blockhash)
                .map_err(|_| RpcError::InvalidParams("invalid blockhash hex".to_string()))?
                .try_into()
                .map_err(|_| RpcError::InvalidParams("blockhash must be 32 bytes".to_string()))?;

            // Reverse for internal byte order
            let mut hash_internal = hash_bytes;
            hash_internal.reverse();

            let block_info = engine
                .get_block_by_hash(&hash_internal)
                .ok_or_else(|| RpcError::NotFound("block not found".to_string()))?;

            if verbosity == 0 {
                // Return raw hex
                let block_data = engine
                    .read_block_data_at_height(block_info.height as u32)
                    .map_err(|e| RpcError::Internal(format!("failed to read block: {}", e)))?;
                let raw = block_data
                    .consensus_encode()
                    .map_err(|e| RpcError::Internal(format!("failed to encode block: {}", e)))?;
                Ok(json!(hex::encode(raw)))
            } else {
                // Return block info
                Ok(json!({
                    "hash": blockhash,
                    "height": block_info.height,
                    "confirmations": state.best_height() as i64 - block_info.height as i64 + 1,
                }))
            }
        }

        "getrawtransaction" => {
            let txid_hex = get_param_str(params, 0)
                .ok_or_else(|| RpcError::InvalidParams("missing txid".to_string()))?;

            // Check mempool first
            let txid_bytes = hex::decode(txid_hex)
                .map_err(|_| RpcError::InvalidParams("invalid txid hex".to_string()))?;
            if txid_bytes.len() != 32 {
                return Err(RpcError::InvalidParams("txid must be 32 bytes".to_string()));
            }
            let mut txid_arr = [0u8; 32];
            txid_arr.copy_from_slice(&txid_bytes);
            txid_arr.reverse();
            let txid = bitcoin::Txid::from_raw_hash(
                bitcoin::hashes::sha256d::Hash::from_byte_array(txid_arr),
            );

            if let Some(entry) = state.mempool.get(&txid) {
                let raw_hex = hex::encode(serialize(&entry.tx));
                let verbose = get_param_bool(params, 1).unwrap_or(false);
                if verbose {
                    Ok(json!({
                        "txid": txid_hex,
                        "hex": raw_hex,
                        "size": entry.size_vbytes,
                        "confirmations": 0,
                    }))
                } else {
                    Ok(json!(raw_hex))
                }
            } else {
                Err(RpcError::NotFound(format!(
                    "transaction {} not found",
                    txid_hex
                )))
            }
        }

        "sendrawtransaction" => {
            let hex_tx = get_param_str(params, 0)
                .ok_or_else(|| RpcError::InvalidParams("missing hex transaction".to_string()))?;

            let tx_bytes = hex::decode(hex_tx)
                .map_err(|_| RpcError::InvalidParams("invalid hex".to_string()))?;

            let tx: bitcoin::Transaction = deserialize(&tx_bytes)
                .map_err(|e| RpcError::InvalidParams(format!("invalid transaction: {}", e)))?;

            let txid = tx.compute_txid();

            // Add to mempool (fee=0, mempool policy will enforce min fee rate)
            state
                .mempool
                .add(tx, 0)
                .map_err(|e| RpcError::Internal(format!("mempool rejection: {}", e)))?;

            Ok(json!(txid.to_string()))
        }

        "getrawmempool" => {
            let entries = state.mempool.get_sorted_by_fee_rate();
            let txids: Vec<String> = entries.iter().map(|e| e.txid.to_string()).collect();
            Ok(json!(txids))
        }

        // ── Wallet RPCs ──────────────────────────────────────────────────
        "getbalance" => {
            let wallet = state
                .wallet()
                .ok_or_else(|| RpcError::Wallet("wallet not loaded".to_string()))?;
            let w = wallet
                .lock()
                .map_err(|e| RpcError::Wallet(format!("wallet lock: {}", e)))?;
            let balance = w.balance();
            // Return confirmed balance in BTC (sat / 1e8)
            Ok(json!(balance.confirmed as f64 / 100_000_000.0))
        }

        "getwalletinfo" => {
            let wallet = state
                .wallet()
                .ok_or_else(|| RpcError::Wallet("wallet not loaded".to_string()))?;
            let w = wallet
                .lock()
                .map_err(|e| RpcError::Wallet(format!("wallet lock: {}", e)))?;
            let balance = w.balance();
            Ok(json!({
                "balance": balance.confirmed as f64 / 100_000_000.0,
                "unconfirmed_balance": balance.trusted_pending as f64 / 100_000_000.0,
                "immature_balance": balance.immature as f64 / 100_000_000.0,
                "txcount": w.list_transactions().len(),
            }))
        }

        "getnewaddress" => {
            let wallet = state
                .wallet()
                .ok_or_else(|| RpcError::Wallet("wallet not loaded".to_string()))?;
            let mut w = wallet
                .lock()
                .map_err(|e| RpcError::Wallet(format!("wallet lock: {}", e)))?;
            let addr = w
                .new_address()
                .map_err(|e| RpcError::Wallet(format!("address generation: {}", e)))?;
            Ok(json!(addr))
        }

        "listtransactions" => {
            let wallet = state
                .wallet()
                .ok_or_else(|| RpcError::Wallet("wallet not loaded".to_string()))?;
            let w = wallet
                .lock()
                .map_err(|e| RpcError::Wallet(format!("wallet lock: {}", e)))?;
            let txs = w.list_transactions();
            let result: Vec<Value> = txs
                .iter()
                .map(|tx| {
                    json!({
                        "txid": tx.txid,
                        "confirmed": tx.confirmed,
                    })
                })
                .collect();
            Ok(json!(result))
        }

        "walletcreatefundedpsbt" => {
            let wallet = state
                .wallet()
                .ok_or_else(|| RpcError::Wallet("wallet not loaded".to_string()))?;

            // params: [inputs (ignored), [{address: amount}], locktime (ignored), options]
            let outputs = params
                .and_then(|p| p.as_array())
                .and_then(|a| a.get(1))
                .and_then(|v| v.as_array())
                .ok_or_else(|| {
                    RpcError::InvalidParams("missing outputs array at param index 1".to_string())
                })?;

            // We support a single output for now
            if outputs.is_empty() {
                return Err(RpcError::InvalidParams("empty outputs".to_string()));
            }

            let first = outputs[0]
                .as_object()
                .ok_or_else(|| RpcError::InvalidParams("output must be an object".to_string()))?;

            let (address, amount_btc) = first
                .iter()
                .next()
                .ok_or_else(|| RpcError::InvalidParams("empty output object".to_string()))?;

            let amount_sat = (amount_btc
                .as_f64()
                .ok_or_else(|| RpcError::InvalidParams("amount must be a number".to_string()))?
                * 100_000_000.0) as u64;

            let fee_rate = params
                .and_then(|p| p.as_array())
                .and_then(|a| a.get(3))
                .and_then(|v| v.as_object())
                .and_then(|o| o.get("fee_rate"))
                .and_then(|v| v.as_f64())
                .unwrap_or(1.0) as f32;

            let mut w = wallet
                .lock()
                .map_err(|e| RpcError::Wallet(format!("wallet lock: {}", e)))?;

            let psbt_b64 = w
                .create_psbt(address, amount_sat, fee_rate, false)
                .map_err(|e| RpcError::Wallet(format!("psbt creation: {}", e)))?;

            Ok(json!({
                "psbt": psbt_b64,
                "fee": 0, // BDK handles fee internally; actual fee embedded in PSBT
                "changepos": -1,
            }))
        }

        "walletprocesspsbt" => {
            let wallet = state
                .wallet()
                .ok_or_else(|| RpcError::Wallet("wallet not loaded".to_string()))?;

            let psbt_b64 = get_param_str(params, 0)
                .ok_or_else(|| RpcError::InvalidParams("missing psbt string".to_string()))?;

            let mut w = wallet
                .lock()
                .map_err(|e| RpcError::Wallet(format!("wallet lock: {}", e)))?;

            let result = w
                .sign_psbt_base64(psbt_b64)
                .map_err(|e| RpcError::Wallet(format!("signing: {}", e)))?;

            Ok(json!({
                "psbt": result.0,
                "complete": result.1,
            }))
        }

        // ── Lightning RPCs ────────────────────────────────────────────
        "ln_getinfo" => {
            let ln = state
                .lightning()
                .ok_or_else(|| RpcError::Lightning("lightning not enabled".to_string()))?;
            let node_id = ln.node_id();
            let channels = ln.channel_manager().list_channels();
            let peers = ln.peer_manager().list_peers();
            let active = channels.iter().filter(|c| c.is_usable).count();
            Ok(json!({
                "node_id": node_id.to_string(),
                "num_channels": channels.len(),
                "num_active_channels": active,
                "num_peers": peers.len(),
            }))
        }

        "ln_listchannels" => {
            let ln = state
                .lightning()
                .ok_or_else(|| RpcError::Lightning("lightning not enabled".to_string()))?;
            let channels = ln.channel_manager().list_channels();
            let result: Vec<Value> = channels
                .iter()
                .map(|c| {
                    json!({
                        "channel_id": hex::encode(c.channel_id.0),
                        "counterparty": c.counterparty.node_id.to_string(),
                        "capacity_sat": c.channel_value_satoshis,
                        "outbound_capacity_msat": c.outbound_capacity_msat,
                        "inbound_capacity_msat": c.inbound_capacity_msat,
                        "is_usable": c.is_usable,
                        "is_outbound": c.is_outbound,
                        "is_channel_ready": c.is_channel_ready,
                        "short_channel_id": c.short_channel_id,
                    })
                })
                .collect();
            Ok(json!(result))
        }

        "ln_listpeers" => {
            let ln = state
                .lightning()
                .ok_or_else(|| RpcError::Lightning("lightning not enabled".to_string()))?;
            let peers = ln.peer_manager().list_peers();
            let result: Vec<Value> = peers
                .iter()
                .map(|p| {
                    json!({
                        "node_id": p.counterparty_node_id.to_string(),
                        "address": p.socket_address.as_ref().map(|a| a.to_string()),
                        "inbound": p.is_inbound_connection,
                    })
                })
                .collect();
            Ok(json!(result))
        }

        "ln_connect" => {
            let ln = state
                .lightning()
                .ok_or_else(|| RpcError::Lightning("lightning not enabled".to_string()))?;

            // Accept "pubkey@host:port" as single param or [pubkey, addr] as two params
            let (pubkey_hex, addr_str) = match get_param_str(params, 0) {
                Some(s) if s.contains('@') => {
                    let parts: Vec<&str> = s.splitn(2, '@').collect();
                    (parts[0].to_string(), parts[1].to_string())
                }
                Some(pk) => {
                    let addr = get_param_str(params, 1)
                        .ok_or_else(|| RpcError::InvalidParams("missing address".to_string()))?;
                    (pk.to_string(), addr.to_string())
                }
                None => {
                    return Err(RpcError::InvalidParams(
                        "missing node_id (pubkey@host:port)".to_string(),
                    ))
                }
            };

            let pubkey: bitcoin::secp256k1::PublicKey = pubkey_hex
                .parse()
                .map_err(|e| RpcError::InvalidParams(format!("invalid pubkey: {}", e)))?;
            let addr: std::net::SocketAddr = addr_str
                .parse()
                .map_err(|e| RpcError::InvalidParams(format!("invalid address: {}", e)))?;

            ln.connect_peer(pubkey, addr)
                .await
                .map_err(|e| RpcError::Lightning(e.to_string()))?;

            Ok(json!({ "connected": true }))
        }

        "ln_openchannel" => {
            let ln = state
                .lightning()
                .ok_or_else(|| RpcError::Lightning("lightning not enabled".to_string()))?;

            let pubkey_hex = get_param_str(params, 0)
                .ok_or_else(|| RpcError::InvalidParams("missing node_id".to_string()))?;
            let amount_sat = get_param_i64(params, 1)
                .ok_or_else(|| RpcError::InvalidParams("missing amount_sat".to_string()))?
                as u64;
            let push_msat = get_param_i64(params, 2).unwrap_or(0) as u64;

            let pubkey: bitcoin::secp256k1::PublicKey = pubkey_hex
                .parse()
                .map_err(|e| RpcError::InvalidParams(format!("invalid pubkey: {}", e)))?;

            let channel_id = ln
                .open_channel(pubkey, amount_sat, push_msat)
                .map_err(|e| RpcError::Lightning(e.to_string()))?;

            Ok(json!({ "channel_id": channel_id }))
        }

        "ln_closechannel" => {
            let ln = state
                .lightning()
                .ok_or_else(|| RpcError::Lightning("lightning not enabled".to_string()))?;

            let channel_id_hex = get_param_str(params, 0)
                .ok_or_else(|| RpcError::InvalidParams("missing channel_id".to_string()))?;
            let counterparty_hex = get_param_str(params, 1)
                .ok_or_else(|| RpcError::InvalidParams("missing counterparty node_id".to_string()))?;
            let force = get_param_bool(params, 2).unwrap_or(false);

            let channel_id_bytes: [u8; 32] = hex::decode(channel_id_hex)
                .map_err(|_| RpcError::InvalidParams("invalid channel_id hex".to_string()))?
                .try_into()
                .map_err(|_| RpcError::InvalidParams("channel_id must be 32 bytes".to_string()))?;

            let channel_id = lightning::ln::types::ChannelId(channel_id_bytes);
            let counterparty: bitcoin::secp256k1::PublicKey = counterparty_hex
                .parse()
                .map_err(|e| RpcError::InvalidParams(format!("invalid pubkey: {}", e)))?;

            ln.close_channel(channel_id, counterparty, force)
                .map_err(|e| RpcError::Lightning(e.to_string()))?;

            Ok(json!({ "closing": true, "force": force }))
        }

        "ln_invoice" => {
            let ln = state
                .lightning()
                .ok_or_else(|| RpcError::Lightning("lightning not enabled".to_string()))?;

            let amount_msat = get_param_i64(params, 0).map(|v| v as u64);
            let description = get_param_str(params, 1).unwrap_or("wolfe invoice");
            let expiry_secs = get_param_i64(params, 2).map(|v| v as u32);

            let invoice = ln
                .create_invoice(amount_msat, description, expiry_secs)
                .map_err(|e| RpcError::Lightning(e.to_string()))?;

            Ok(json!({ "invoice": invoice }))
        }

        "ln_pay" => {
            let ln = state
                .lightning()
                .ok_or_else(|| RpcError::Lightning("lightning not enabled".to_string()))?;

            let invoice_str = get_param_str(params, 0)
                .ok_or_else(|| RpcError::InvalidParams("missing invoice".to_string()))?;

            let payment_id = ln
                .pay_invoice(invoice_str)
                .map_err(|e| RpcError::Lightning(e.to_string()))?;

            Ok(json!({ "payment_id": payment_id }))
        }

        "createwallet" => {
            // Error if wallet already exists
            if state.wallet().is_some() {
                return Err(RpcError::Wallet("wallet already loaded".to_string()));
            }

            let (wallet, mnemonic) = wolfe_wallet::NodeWallet::create_new(
                &state.wallet_db_path,
                state.network,
            )
            .map_err(|e| RpcError::Wallet(format!("wallet creation failed: {}", e)))?;

            let wallet = Arc::new(std::sync::Mutex::new(wallet));

            // Inject wallet into NodeState
            state.set_wallet(wallet.clone());

            // Inject wallet into Lightning manager if available
            if let Some(ln) = state.lightning() {
                ln.set_wallet(wallet);
            }

            Ok(json!({
                "mnemonic": mnemonic.to_string(),
                "message": "BACKUP THIS SEED PHRASE — it will NOT be shown again"
            }))
        }

        "importwallet" => {
            // Error if wallet already exists
            if state.wallet().is_some() {
                return Err(RpcError::Wallet("wallet already loaded — stop node, delete wallet DB, then retry".to_string()));
            }

            let mnemonic_str = get_param_str(params, 0)
                .ok_or_else(|| RpcError::InvalidParams("missing mnemonic seed phrase".to_string()))?;

            let mnemonic: wolfe_wallet::Mnemonic = mnemonic_str
                .parse()
                .map_err(|e| RpcError::InvalidParams(format!("invalid mnemonic: {}", e)))?;

            let wallet = wolfe_wallet::NodeWallet::from_mnemonic(
                &state.wallet_db_path,
                state.network,
                &mnemonic,
            )
            .map_err(|e| RpcError::Wallet(format!("wallet import failed: {}", e)))?;

            let wallet = Arc::new(std::sync::Mutex::new(wallet));

            // Inject wallet into NodeState
            state.set_wallet(wallet.clone());

            // Inject wallet into Lightning manager if available
            if let Some(ln) = state.lightning() {
                ln.set_wallet(wallet);
            }

            Ok(json!({
                "message": "wallet imported successfully — rescan will happen as blocks sync"
            }))
        }

        "rescanblockchain" => {
            let wallet = state
                .wallet()
                .ok_or_else(|| RpcError::Wallet("wallet not loaded".to_string()))?;
            let engine = state
                .consensus()
                .ok_or_else(|| RpcError::Internal("consensus engine not available".to_string()))?;

            let chain_height = engine.chain_height();
            if chain_height <= 0 {
                return Err(RpcError::Internal("chain not synced yet".to_string()));
            }
            let tip = chain_height as u32;

            let start = get_param_i64(params, 0).unwrap_or(0) as u32;
            let stop = get_param_i64(params, 1).map(|v| v as u32).unwrap_or(tip);

            let mut scanned = 0u32;
            let mut found_txs = 0usize;

            for height in start..=stop {
                let kernel_block = engine
                    .read_block_data_at_height(height)
                    .map_err(|e| RpcError::Internal(format!("block read at {}: {}", height, e)))?;
                let bytes = kernel_block
                    .consensus_encode()
                    .map_err(|e| RpcError::Internal(format!("block encode at {}: {}", height, e)))?;
                let block: bitcoin::Block = deserialize(&bytes)
                    .map_err(|e| RpcError::Internal(format!("block deserialize at {}: {}", height, e)))?;

                let mut w = wallet
                    .lock()
                    .map_err(|e| RpcError::Wallet(format!("wallet lock: {}", e)))?;

                let before = w.list_transactions().len();
                if let Err(e) = w.rescan_block(&block, height, state.network) {
                    tracing::warn!(height, ?e, "rescan: wallet failed to process block");
                }
                let after = w.list_transactions().len();
                if after > before {
                    found_txs += after - before;
                }
                scanned += 1;
            }

            Ok(json!({
                "start_height": start,
                "stop_height": stop,
                "blocks_scanned": scanned,
                "transactions_found": found_txs,
            }))
        }

        "uptime" => {
            let uptime = state.started_at.elapsed().as_secs();
            Ok(json!(uptime))
        }

        "stop" => {
            let triggered = state.trigger_shutdown();
            if triggered {
                Ok(json!("BitcoinWolfe server stopping"))
            } else {
                Err(RpcError::Internal("shutdown flag not wired".to_string()))
            }
        }

        _ => Err(RpcError::MethodNotFound(format!(
            "method '{}' not found",
            method
        ))),
    }
}

// ─── Parameter extraction helpers ────────────────────────────────────────────

fn get_param_str(params: Option<&Value>, index: usize) -> Option<&str> {
    params?.as_array()?.get(index)?.as_str()
}

fn get_param_i64(params: Option<&Value>, index: usize) -> Option<i64> {
    params?.as_array()?.get(index)?.as_i64()
}

fn get_param_bool(params: Option<&Value>, index: usize) -> Option<bool> {
    params?.as_array()?.get(index)?.as_bool()
}
