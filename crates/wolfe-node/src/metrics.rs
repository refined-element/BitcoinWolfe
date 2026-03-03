//! Prometheus metrics for the BitcoinWolfe node.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tracing::{error, info};

/// All node metrics, updated by various subsystems.
pub struct NodeMetrics {
    pub started_at: Instant,
    // Sync
    pub headers_height: Arc<AtomicU64>,
    pub blocks_height: Arc<AtomicU64>,
    // P2P
    pub peers_connected: Arc<AtomicU64>,
    pub peers_inbound: Arc<AtomicU64>,
    pub peers_outbound: Arc<AtomicU64>,
    pub messages_received: Arc<AtomicU64>,
    pub _messages_sent: Arc<AtomicU64>,
    pub _bytes_received: Arc<AtomicU64>,
    pub _bytes_sent: Arc<AtomicU64>,
    // Mempool
    pub mempool_txs: Arc<AtomicU64>,
    pub mempool_bytes: Arc<AtomicU64>,
    // RPC
    pub rpc_requests: Arc<AtomicU64>,
    // Lightning
    pub ln_channels_active: Arc<AtomicU64>,
    pub ln_peers_connected: Arc<AtomicU64>,
    pub ln_capacity_sat: Arc<AtomicU64>,
}

impl NodeMetrics {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
            headers_height: Arc::new(AtomicU64::new(0)),
            blocks_height: Arc::new(AtomicU64::new(0)),
            peers_connected: Arc::new(AtomicU64::new(0)),
            peers_inbound: Arc::new(AtomicU64::new(0)),
            peers_outbound: Arc::new(AtomicU64::new(0)),
            messages_received: Arc::new(AtomicU64::new(0)),
            _messages_sent: Arc::new(AtomicU64::new(0)),
            _bytes_received: Arc::new(AtomicU64::new(0)),
            _bytes_sent: Arc::new(AtomicU64::new(0)),
            mempool_txs: Arc::new(AtomicU64::new(0)),
            mempool_bytes: Arc::new(AtomicU64::new(0)),
            rpc_requests: Arc::new(AtomicU64::new(0)),
            ln_channels_active: Arc::new(AtomicU64::new(0)),
            ln_peers_connected: Arc::new(AtomicU64::new(0)),
            ln_capacity_sat: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Render all metrics in Prometheus exposition format.
    pub fn render(&self) -> String {
        let uptime = self.started_at.elapsed().as_secs();
        let mut out = String::with_capacity(2048);

        out.push_str("# HELP wolfe_uptime_seconds Node uptime in seconds.\n");
        out.push_str("# TYPE wolfe_uptime_seconds gauge\n");
        out.push_str(&format!("wolfe_uptime_seconds {}\n\n", uptime));

        out.push_str("# HELP wolfe_headers_height Current best header height.\n");
        out.push_str("# TYPE wolfe_headers_height gauge\n");
        out.push_str(&format!(
            "wolfe_headers_height {}\n\n",
            self.headers_height.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP wolfe_blocks_height Current validated block height.\n");
        out.push_str("# TYPE wolfe_blocks_height gauge\n");
        out.push_str(&format!(
            "wolfe_blocks_height {}\n\n",
            self.blocks_height.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP wolfe_peers_connected Total connected peers.\n");
        out.push_str("# TYPE wolfe_peers_connected gauge\n");
        out.push_str(&format!(
            "wolfe_peers_connected {}\n\n",
            self.peers_connected.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP wolfe_peers_inbound Inbound peer connections.\n");
        out.push_str("# TYPE wolfe_peers_inbound gauge\n");
        out.push_str(&format!(
            "wolfe_peers_inbound {}\n\n",
            self.peers_inbound.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP wolfe_peers_outbound Outbound peer connections.\n");
        out.push_str("# TYPE wolfe_peers_outbound gauge\n");
        out.push_str(&format!(
            "wolfe_peers_outbound {}\n\n",
            self.peers_outbound.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP wolfe_p2p_messages_received_total Total P2P messages received.\n");
        out.push_str("# TYPE wolfe_p2p_messages_received_total counter\n");
        out.push_str(&format!(
            "wolfe_p2p_messages_received_total {}\n\n",
            self.messages_received.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP wolfe_mempool_transactions Current mempool transaction count.\n");
        out.push_str("# TYPE wolfe_mempool_transactions gauge\n");
        out.push_str(&format!(
            "wolfe_mempool_transactions {}\n\n",
            self.mempool_txs.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP wolfe_mempool_bytes Current mempool size in bytes.\n");
        out.push_str("# TYPE wolfe_mempool_bytes gauge\n");
        out.push_str(&format!(
            "wolfe_mempool_bytes {}\n\n",
            self.mempool_bytes.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP wolfe_rpc_requests_total Total RPC requests served.\n");
        out.push_str("# TYPE wolfe_rpc_requests_total counter\n");
        out.push_str(&format!(
            "wolfe_rpc_requests_total {}\n\n",
            self.rpc_requests.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP wolfe_ln_channels_active Active Lightning channels.\n");
        out.push_str("# TYPE wolfe_ln_channels_active gauge\n");
        out.push_str(&format!(
            "wolfe_ln_channels_active {}\n\n",
            self.ln_channels_active.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP wolfe_ln_peers_connected Connected Lightning peers.\n");
        out.push_str("# TYPE wolfe_ln_peers_connected gauge\n");
        out.push_str(&format!(
            "wolfe_ln_peers_connected {}\n\n",
            self.ln_peers_connected.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP wolfe_ln_capacity_sat Total Lightning channel capacity in satoshis.\n",
        );
        out.push_str("# TYPE wolfe_ln_capacity_sat gauge\n");
        out.push_str(&format!(
            "wolfe_ln_capacity_sat {}\n\n",
            self.ln_capacity_sat.load(Ordering::Relaxed)
        ));

        out
    }
}

/// Start an HTTP metrics server using axum for proper request handling.
pub async fn serve_metrics(listen_addr: String, metrics: Arc<NodeMetrics>) {
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::Router;

    async fn metrics_handler(State(metrics): State<Arc<NodeMetrics>>) -> impl IntoResponse {
        let body = metrics.render();
        (
            StatusCode::OK,
            [("Content-Type", "text/plain; version=0.0.4; charset=utf-8")],
            body,
        )
    }

    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(metrics);

    let listener = match tokio::net::TcpListener::bind(&listen_addr).await {
        Ok(l) => l,
        Err(e) => {
            error!(%listen_addr, ?e, "failed to bind metrics server");
            return;
        }
    };

    info!(%listen_addr, "metrics server listening");

    if let Err(e) = axum::serve(listener, app).await {
        error!(?e, "metrics server failed");
    }
}
