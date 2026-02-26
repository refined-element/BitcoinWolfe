use bitcoin::p2p::ServiceFlags;
use std::net::SocketAddr;
use std::time::Instant;

/// Unique identifier for a connected peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerId(pub u64);

/// Static information about a connected peer, gathered during handshake.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub id: PeerId,
    pub addr: SocketAddr,
    pub user_agent: String,
    pub version: u32,
    pub services: ServiceFlags,
    pub start_height: i32,
    pub relay: bool,
    pub inbound: bool,
    pub v2_transport: bool,
    pub connected_at: Instant,
}

/// Represents a connected peer with its current state.
#[derive(Debug)]
pub struct Peer {
    pub info: PeerInfo,
    pub last_seen: Instant,
    pub last_ping: Option<Instant>,
    pub ping_nonce: Option<u64>,
    pub ping_latency_ms: Option<u64>,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub misbehavior_score: u32,
    pub syncing_headers: bool,
    pub syncing_blocks: bool,
}

impl Peer {
    pub fn new(info: PeerInfo) -> Self {
        Self {
            last_seen: info.connected_at,
            info,
            last_ping: None,
            ping_nonce: None,
            ping_latency_ms: None,
            bytes_sent: 0,
            bytes_received: 0,
            misbehavior_score: 0,
            syncing_headers: false,
            syncing_blocks: false,
        }
    }

    /// Record misbehavior. Returns true if the peer should be banned (score >= 100).
    pub fn record_misbehavior(&mut self, score: u32, _reason: &str) -> bool {
        self.misbehavior_score = self.misbehavior_score.saturating_add(score);
        self.misbehavior_score >= 100
    }

    pub fn uptime(&self) -> std::time::Duration {
        self.info.connected_at.elapsed()
    }
}
