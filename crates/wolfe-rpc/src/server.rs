use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

use wolfe_consensus::ConsensusEngine;
use wolfe_lightning::LightningManager;
use wolfe_mempool::Mempool;
use wolfe_types::config::{NostrConfig, RpcConfig};
use wolfe_wallet::NodeWallet;

use crate::handlers;

/// Shared node state accessible from RPC handlers.
pub struct NodeState {
    pub chain: String,
    pub mempool: Arc<Mempool>,
    peer_infos: parking_lot::RwLock<Vec<wolfe_types::PeerInfoSnapshot>>,
    pub started_at: Instant,
    best_height: AtomicU64,
    best_hash: parking_lot::RwLock<String>,
    syncing: AtomicBool,
    shutdown: Option<Arc<AtomicBool>>,
    consensus: Option<Arc<ConsensusEngine>>,
    wallet: Option<Arc<std::sync::Mutex<NodeWallet>>>,
    lightning: Option<Arc<LightningManager>>,
}

impl NodeState {
    pub fn new(chain: String, mempool: Arc<Mempool>) -> Self {
        Self {
            chain,
            mempool,
            peer_infos: parking_lot::RwLock::new(Vec::new()),
            started_at: Instant::now(),
            best_height: AtomicU64::new(0),
            best_hash: parking_lot::RwLock::new(String::new()),
            syncing: AtomicBool::new(true),
            shutdown: None,
            consensus: None,
            wallet: None,
            lightning: None,
        }
    }

    pub fn set_consensus(&mut self, engine: Arc<ConsensusEngine>) {
        self.consensus = Some(engine);
    }

    pub fn consensus(&self) -> Option<&Arc<ConsensusEngine>> {
        self.consensus.as_ref()
    }

    pub fn set_wallet(&mut self, wallet: Arc<std::sync::Mutex<NodeWallet>>) {
        self.wallet = Some(wallet);
    }

    pub fn wallet(&self) -> Option<&Arc<std::sync::Mutex<NodeWallet>>> {
        self.wallet.as_ref()
    }

    pub fn set_lightning(&mut self, manager: Arc<LightningManager>) {
        self.lightning = Some(manager);
    }

    pub fn lightning(&self) -> Option<&Arc<LightningManager>> {
        self.lightning.as_ref()
    }

    pub fn set_shutdown_flag(&mut self, flag: Arc<AtomicBool>) {
        self.shutdown = Some(flag);
    }

    pub fn trigger_shutdown(&self) -> bool {
        if let Some(ref flag) = self.shutdown {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn best_height(&self) -> u64 {
        self.best_height.load(Ordering::Relaxed)
    }

    pub fn set_best_height(&self, height: u64) {
        self.best_height.store(height, Ordering::Relaxed);
    }

    pub fn best_hash(&self) -> String {
        self.best_hash.read().clone()
    }

    pub fn set_best_hash(&self, hash: String) {
        *self.best_hash.write() = hash;
    }

    pub fn is_syncing(&self) -> bool {
        self.syncing.load(Ordering::Relaxed)
    }

    pub fn set_syncing(&self, syncing: bool) {
        self.syncing.store(syncing, Ordering::Relaxed);
    }

    pub fn peer_count(&self) -> usize {
        self.peer_infos.read().len()
    }

    pub fn peer_infos(&self) -> Vec<wolfe_types::PeerInfoSnapshot> {
        self.peer_infos.read().clone()
    }

    pub fn set_peer_infos(&self, infos: Vec<wolfe_types::PeerInfoSnapshot>) {
        *self.peer_infos.write() = infos;
    }

    pub fn add_peer_info(&self, info: wolfe_types::PeerInfoSnapshot) {
        self.peer_infos.write().push(info);
    }

    pub fn remove_peer_info(&self, addr: std::net::SocketAddr) {
        self.peer_infos.write().retain(|p| p.addr != addr);
    }

    pub fn get_info(&self) -> serde_json::Value {
        serde_json::json!({
            "version": wolfe_types::VERSION,
            "user_agent": wolfe_types::user_agent(),
            "chain": self.chain,
            "blocks": self.best_height(),
            "best_block_hash": self.best_hash(),
            "mempool_size": self.mempool.len(),
            "peers": self.peer_count(),
            "uptime_secs": self.started_at.elapsed().as_secs(),
            "syncing": self.is_syncing(),
        })
    }
}

/// The RPC server: serves both REST API and JSON-RPC endpoints.
pub struct RpcServer {
    config: RpcConfig,
    nostr_config: NostrConfig,
    state: Arc<NodeState>,
}

impl RpcServer {
    pub fn new(config: RpcConfig, state: Arc<NodeState>) -> Self {
        Self {
            config,
            nostr_config: NostrConfig::default(),
            state,
        }
    }

    pub fn with_nostr_config(mut self, nostr_config: NostrConfig) -> Self {
        self.nostr_config = nostr_config;
        self
    }

    /// Start the RPC server. This will block until the server is shut down.
    pub async fn start(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let state = self.state;

        // Build expected credentials for HTTP Basic auth
        let expected_credentials = match (&self.config.user, &self.config.password) {
            (Some(user), Some(pass)) => {
                use base64::Engine as _;
                let encoded = base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", user, pass));
                Some(format!("Basic {}", encoded))
            }
            _ => {
                warn!("RPC server starting without authentication — set rpc.user and rpc.password in config");
                None
            }
        };

        // Parse NIP-98 allowed pubkeys
        let nip98_enabled = self.nostr_config.nip98_auth;
        let nip98_pubkeys: Vec<nostr_sdk::prelude::PublicKey> = if nip98_enabled {
            self.nostr_config
                .allowed_pubkeys
                .iter()
                .filter_map(|pk| nostr_sdk::prelude::PublicKey::parse(pk).ok())
                .collect()
        } else {
            vec![]
        };
        let nip98_pubkeys = Arc::new(nip98_pubkeys);
        let listen_addr_for_nip98 = Arc::new(self.config.listen.clone());

        let auth_credentials = expected_credentials.clone();
        let nip98_pks = nip98_pubkeys.clone();
        let nip98_listen = listen_addr_for_nip98.clone();
        let nip98_on = nip98_enabled;
        let mut app = Router::new()
            // JSON-RPC endpoint (Bitcoin Core compatible)
            .route("/", post(handlers::json_rpc))
            // REST API endpoints
            .route("/api/info", get(handlers::get_info))
            .route("/api/blockchain", get(handlers::get_blockchain))
            .route("/api/mempool", get(handlers::get_mempool))
            .route("/api/peers", get(handlers::get_peers))
            .route("/api/lightning/info", get(handlers::get_lightning_info))
            .route("/api/lightning/channels", get(handlers::get_lightning_channels))
            .with_state(state)
            .layer(middleware::from_fn(move |req, next| {
                let creds = auth_credentials.clone();
                let pks = nip98_pks.clone();
                let listen = nip98_listen.clone();
                auth_middleware(creds, nip98_on, pks, listen, req, next)
            }))
            .layer(TraceLayer::new_for_http());

        if !self.config.cors_origins.is_empty() {
            let origins: Vec<_> = self
                .config
                .cors_origins
                .iter()
                .filter_map(|o| o.parse::<axum::http::HeaderValue>().ok())
                .collect();
            app = app.layer(
                CorsLayer::new()
                    .allow_origin(origins)
                    .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                    .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::AUTHORIZATION]),
            );
        }

        let addr = &self.config.listen;
        let listener = tokio::net::TcpListener::bind(addr).await?;

        let auth_status = if expected_credentials.is_some() {
            "enabled"
        } else {
            "DISABLED"
        };
        info!(%addr, rest = self.config.rest_enabled, auth = auth_status, "RPC server listening");

        axum::serve(listener, app).await?;

        Ok(())
    }
}

/// Unified authentication middleware supporting both HTTP Basic and NIP-98 Nostr auth.
///
/// Authentication order:
/// 1. If no auth configured (no Basic creds AND nip98 disabled): allow all
/// 2. If `Authorization: Nostr <base64>` header present and NIP-98 enabled: verify NIP-98
/// 3. If `Authorization: Basic <base64>` header present and Basic creds configured: verify Basic
/// 4. Otherwise: reject
async fn auth_middleware(
    basic_expected: Option<String>,
    nip98_enabled: bool,
    nip98_pubkeys: Arc<Vec<nostr_sdk::prelude::PublicKey>>,
    listen_addr: Arc<String>,
    req: Request,
    next: Next,
) -> Response {
    // No auth configured at all — allow everything
    if basic_expected.is_none() && !nip98_enabled {
        return next.run(req).await;
    }

    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let method = req.method().to_string();
    let uri = req.uri().to_string();

    if let Some(ref header) = auth_header {
        // Try NIP-98 auth first
        if nip98_enabled && header.starts_with("Nostr ") {
            let token = &header[6..];
            let url = format!("http://{}{}", listen_addr, uri);
            match wolfe_nostr::nip98::verify_nip98(token, &url, &method, &nip98_pubkeys) {
                Ok(_pubkey) => return next.run(req).await,
                Err(e) => {
                    warn!("NIP-98 auth failed: {}", e);
                    return unauthorized_response();
                }
            }
        }

        // Try Basic auth
        if let Some(ref expected) = basic_expected {
            if header == expected {
                return next.run(req).await;
            }
        }
    }

    // If we have any auth method configured but none matched, reject
    warn!("RPC authentication failed");
    unauthorized_response()
}

fn unauthorized_response() -> Response {
    Response::builder()
        .status(axum::http::StatusCode::UNAUTHORIZED)
        .header("WWW-Authenticate", "Basic realm=\"wolfe-rpc\", Nostr")
        .body(axum::body::Body::from(
            serde_json::json!({
                "error": {
                    "code": -32600,
                    "message": "unauthorized"
                }
            })
            .to_string(),
        ))
        .unwrap()
}
