use axum::extract::State;
use axum::Json;
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
    _params: Option<&Value>,
) -> Result<Value, RpcError> {
    match method {
        "getblockchaininfo" => Ok(json!({
            "chain": state.chain,
            "blocks": state.best_height(),
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

        "uptime" => {
            let uptime = state.started_at.elapsed().as_secs();
            Ok(json!(uptime))
        }

        "stop" => Ok(json!("BitcoinWolfe server stopping")),

        _ => Err(RpcError::MethodNotFound(format!(
            "method '{}' not found",
            method
        ))),
    }
}
