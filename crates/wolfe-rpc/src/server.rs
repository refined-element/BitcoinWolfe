use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use wolfe_mempool::Mempool;
use wolfe_types::config::RpcConfig;

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
    state: Arc<NodeState>,
}

impl RpcServer {
    pub fn new(config: RpcConfig, state: Arc<NodeState>) -> Self {
        Self { config, state }
    }

    /// Start the RPC server. This will block until the server is shut down.
    pub async fn start(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let state = self.state;

        let mut app = Router::new()
            // JSON-RPC endpoint (Bitcoin Core compatible)
            .route("/", post(handlers::json_rpc))
            // REST API endpoints
            .route("/api/info", get(handlers::get_info))
            .route("/api/blockchain", get(handlers::get_blockchain))
            .route("/api/mempool", get(handlers::get_mempool))
            .route("/api/peers", get(handlers::get_peers))
            .with_state(state)
            .layer(TraceLayer::new_for_http());

        if !self.config.cors_origins.is_empty() {
            app = app.layer(CorsLayer::permissive());
        }

        let addr = &self.config.listen;
        let listener = tokio::net::TcpListener::bind(addr).await?;

        info!(%addr, rest = self.config.rest_enabled, "RPC server listening");

        axum::serve(listener, app).await?;

        Ok(())
    }
}
