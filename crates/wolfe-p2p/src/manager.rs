use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::ServiceFlags;
use dashmap::DashMap;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::connection::PeerConnection;
use crate::error::P2pError;
use crate::peer::{Peer, PeerId, PeerInfo};
use wolfe_types::config::P2pConfig;

/// DNS seeds per network. Bitcoin Core-compatible peer discovery.
fn dns_seeds_for_network(network: bitcoin::Network) -> &'static [&'static str] {
    match network {
        bitcoin::Network::Bitcoin => &[
            "seed.bitcoin.sipa.be",
            "dnsseed.bluematt.me",
            "dnsseed.bitcoin.dashjr-list-of-hierarchical-deterministic-wallets.org",
            "seed.bitcoinstats.com",
            "seed.bitcoin.jonasschnelli.ch",
            "seed.btc.petertodd.net",
            "seed.bitcoin.sprovoost.nl",
            "dnsseed.emzy.de",
            "seed.bitcoin.wiz.biz",
        ],
        bitcoin::Network::Testnet => &[
            "testnet-seed.bitcoin.jonasschnelli.ch",
            "seed.tbtc.petertodd.net",
            "seed.testnet.bitcoin.sprovoost.nl",
            "testnet-seed.bluematt.me",
        ],
        bitcoin::Network::Signet => &["seed.signet.bitcoin.sprovoost.nl"],
        bitcoin::Network::Regtest => &[],
        // Catch future variants
        _ => &[],
    }
}

/// Default P2P port per network.
fn default_port_for_network(network: bitcoin::Network) -> u16 {
    match network {
        bitcoin::Network::Bitcoin => 8333,
        bitcoin::Network::Testnet => 18333,
        bitcoin::Network::Signet => 38333,
        bitcoin::Network::Regtest => 18444,
        _ => 8333,
    }
}

/// Events emitted by the peer manager for the node to handle.
#[derive(Debug)]
pub enum PeerEvent {
    /// A new peer has connected (inbound or outbound).
    Connected(PeerInfo),
    /// A peer has disconnected.
    Disconnected(PeerId),
    /// A message was received from a peer.
    Message(PeerId, NetworkMessage),
    /// A peer was banned for misbehavior.
    Banned(PeerId, String),
}

/// Manages all peer connections and the P2P event loop.
pub struct PeerManager {
    config: P2pConfig,
    network: bitcoin::Network,
    peers: Arc<DashMap<PeerId, Peer>>,
    /// Per-peer outbound message channels. When the node wants to send a message
    /// to a peer, it drops the message into that peer's channel; the peer's
    /// event loop picks it up and writes it to the TCP stream.
    peer_senders: Arc<DashMap<PeerId, mpsc::Sender<NetworkMessage>>>,
    next_peer_id: AtomicU64,
    event_tx: mpsc::Sender<PeerEvent>,
    event_rx: Option<mpsc::Receiver<PeerEvent>>,
    best_height: Arc<AtomicU64>,
    /// Our version nonces, used to detect self-connections.
    our_nonces: Arc<DashMap<u64, ()>>,
}

impl PeerManager {
    pub fn new(config: P2pConfig, network: bitcoin::Network) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);
        Self {
            config,
            network,
            peers: Arc::new(DashMap::new()),
            peer_senders: Arc::new(DashMap::new()),
            next_peer_id: AtomicU64::new(1),
            event_tx,
            event_rx: Some(event_rx),
            best_height: Arc::new(AtomicU64::new(0)),
            our_nonces: Arc::new(DashMap::new()),
        }
    }

    /// Take the event receiver (can only be called once).
    pub fn take_event_rx(&mut self) -> Option<mpsc::Receiver<PeerEvent>> {
        self.event_rx.take()
    }

    /// Update our best known block height (for version messages).
    pub fn set_best_height(&self, height: u64) {
        self.best_height.store(height, Ordering::Relaxed);
    }

    /// Number of currently connected peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Get info about all connected peers.
    pub fn connected_peers(&self) -> Vec<PeerInfo> {
        self.peers.iter().map(|p| p.info.clone()).collect()
    }

    fn next_id(&self) -> PeerId {
        PeerId(self.next_peer_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Start the P2P manager: listen for inbound, connect to outbound.
    pub async fn start(self: Arc<Self>) -> Result<(), P2pError> {
        info!(
            listen = %self.config.listen,
            max_inbound = self.config.max_inbound,
            max_outbound = self.config.max_outbound,
            "starting P2P manager"
        );

        let this = self.clone();
        // Spawn the inbound listener
        let listen_addr = self.config.listen.clone();
        tokio::spawn(async move {
            if let Err(e) = this.listen_inbound(&listen_addr).await {
                error!(?e, "inbound listener failed");
            }
        });

        // Connect to manually specified peers
        let manual_addrs: Vec<SocketAddr> = self
            .config
            .connect
            .iter()
            .filter_map(|s| s.parse::<SocketAddr>().ok())
            .collect();

        for addr in &manual_addrs {
            let this = self.clone();
            let addr = *addr;
            tokio::spawn(async move {
                this.connect_outbound(addr).await;
            });
        }

        // If no manual peers, try DNS seeds
        if self.config.connect.is_empty() {
            let this = self.clone();
            tokio::spawn(async move {
                this.discover_peers().await;
            });
        }

        // Spawn reconnection loop for maintaining outbound peer count
        {
            let this = self.clone();
            let manual_addrs = manual_addrs.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
                loop {
                    interval.tick().await;
                    let outbound_count = this.peers.iter().filter(|p| !p.info.inbound).count();
                    if outbound_count < this.config.max_outbound {
                        let needed = this.config.max_outbound - outbound_count;
                        debug!(
                            outbound_count,
                            needed, "reconnection: need more outbound peers"
                        );

                        // Try manual peers first
                        for addr in &manual_addrs {
                            let already_connected = this.peers.iter().any(|p| p.info.addr == *addr);
                            if !already_connected {
                                this.connect_outbound(*addr).await;
                            }
                        }

                        // If still short and no manual peers, re-discover via DNS
                        let new_outbound = this.peers.iter().filter(|p| !p.info.inbound).count();
                        if new_outbound < this.config.max_outbound && manual_addrs.is_empty() {
                            this.discover_peers().await;
                        }
                    }
                }
            });
        }

        Ok(())
    }

    /// Listen for inbound peer connections.
    async fn listen_inbound(&self, listen_addr: &str) -> Result<(), P2pError> {
        let listener = TcpListener::bind(listen_addr).await?;
        info!(%listen_addr, "listening for inbound peers");

        loop {
            let (stream, addr) = listener.accept().await?;

            let inbound_count = self.peers.iter().filter(|p| p.info.inbound).count();
            if inbound_count >= self.config.max_inbound {
                debug!(%addr, "rejecting inbound peer (at max)");
                continue;
            }

            let peer_id = self.next_id();
            let network = self.network;
            let height = self.best_height.load(Ordering::Relaxed) as i32;
            let event_tx = self.event_tx.clone();
            let peers = self.peers.clone();
            let peer_senders = self.peer_senders.clone();

            let our_nonces = self.our_nonces.clone();
            tokio::spawn(async move {
                match PeerConnection::accept(
                    stream,
                    addr,
                    network,
                    ServiceFlags::NETWORK | ServiceFlags::WITNESS,
                    height,
                    peer_id,
                )
                .await
                {
                    Ok(mut conn) => {
                        // Register our nonce for self-connection detection
                        our_nonces.insert(conn.our_nonce, ());

                        // Check for self-connection
                        if our_nonces.contains_key(&conn.their_nonce) {
                            info!(%addr, "detected self-connection (nonce match) — dropping");
                            our_nonces.remove(&conn.our_nonce);
                            return;
                        }

                        let info = conn.info.clone();
                        let (msg_tx, msg_rx) = mpsc::channel(256);
                        peers.insert(peer_id, Peer::new(info.clone()));
                        peer_senders.insert(peer_id, msg_tx);
                        let _ = event_tx.send(PeerEvent::Connected(info)).await;
                        Self::run_peer_loop(&mut conn, msg_rx, &event_tx, &peers).await;
                        peer_senders.remove(&peer_id);
                        peers.remove(&peer_id);
                        our_nonces.remove(&conn.our_nonce);
                        let _ = event_tx.send(PeerEvent::Disconnected(peer_id)).await;
                    }
                    Err(e) => {
                        debug!(%addr, ?e, "inbound handshake failed");
                    }
                }
            });
        }
    }

    /// Connect to an outbound peer.
    async fn connect_outbound(&self, addr: SocketAddr) {
        let outbound_count = self.peers.iter().filter(|p| !p.info.inbound).count();
        if outbound_count >= self.config.max_outbound {
            return;
        }

        let peer_id = self.next_id();
        let height = self.best_height.load(Ordering::Relaxed) as i32;

        match PeerConnection::connect(
            addr,
            self.network,
            ServiceFlags::NETWORK | ServiceFlags::WITNESS,
            height,
            peer_id,
        )
        .await
        {
            Ok(mut conn) => {
                // Register our nonce for self-connection detection
                self.our_nonces.insert(conn.our_nonce, ());

                // Check for self-connection
                if self.our_nonces.contains_key(&conn.their_nonce) {
                    info!(%addr, "detected self-connection (nonce match) — dropping");
                    self.our_nonces.remove(&conn.our_nonce);
                    return;
                }

                let info = conn.info.clone();
                let (msg_tx, msg_rx) = mpsc::channel(256);
                self.peers.insert(peer_id, Peer::new(info.clone()));
                self.peer_senders.insert(peer_id, msg_tx);
                let _ = self.event_tx.send(PeerEvent::Connected(info)).await;

                let event_tx = self.event_tx.clone();
                let peers = self.peers.clone();
                let peer_senders = self.peer_senders.clone();
                let our_nonces = self.our_nonces.clone();
                let our_nonce = conn.our_nonce;
                tokio::spawn(async move {
                    Self::run_peer_loop(&mut conn, msg_rx, &event_tx, &peers).await;
                    peer_senders.remove(&peer_id);
                    peers.remove(&peer_id);
                    our_nonces.remove(&our_nonce);
                    let _ = event_tx.send(PeerEvent::Disconnected(peer_id)).await;
                });
            }
            Err(e) => {
                debug!(%addr, ?e, "outbound connection failed");
            }
        }
    }

    /// Maximum time to wait for a message from a peer before considering
    /// the connection stale. Matches Bitcoin Core's default timeout (~20 min).
    const PEER_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20 * 60);

    /// Interval between keepalive pings (~2 min, matching Bitcoin Core).
    const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(120);

    /// If a ping response hasn't arrived within this time, consider the peer stale.
    const PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

    /// Maximum *non-block* messages per second from a single peer.
    ///
    /// Block messages (which we explicitly requested via getdata) are exempt
    /// because during IBD early blocks are tiny and arrive at 10k+ per second.
    /// This limit only applies to unsolicited protocol messages (inv, addr,
    /// headers, tx, etc.) to protect against flooding attacks.
    const MAX_PROTOCOL_MESSAGES_PER_SEC: u32 = 2000;

    /// Main message loop for a single peer.
    ///
    /// Uses `tokio::select!` to concurrently:
    /// - Read inbound messages from the TCP stream and forward them to the node
    /// - Read outbound messages from the per-peer channel and write them to TCP
    /// - Send periodic pings to detect stale peers
    async fn run_peer_loop(
        conn: &mut PeerConnection,
        mut outbound_rx: mpsc::Receiver<NetworkMessage>,
        event_tx: &mpsc::Sender<PeerEvent>,
        peers: &DashMap<PeerId, Peer>,
    ) {
        let peer_id = conn.info.id;
        let mut ping_interval = tokio::time::interval(Self::PING_INTERVAL);
        // Skip the first immediate tick
        ping_interval.tick().await;

        // Rate limiter for non-block protocol messages only.
        // Block messages are exempt because we explicitly requested them and
        // during IBD early blocks (height < 200k) are tiny, arriving at 10k+/sec.
        let mut rate_window_start = std::time::Instant::now();
        let mut rate_count: u32 = 0;

        loop {
            tokio::select! {
                // Inbound: read a message from the peer over TCP (with timeout)
                recv_result = tokio::time::timeout(Self::PEER_READ_TIMEOUT, conn.recv()) => {
                let recv_result = match recv_result {
                    Ok(inner) => inner,
                    Err(_) => {
                        info!(peer = ?peer_id, "peer timed out (no messages for 20 min)");
                        break;
                    }
                };
                    match recv_result {
                        Ok(msg) => {
                            // Rate limiting for non-block protocol messages.
                            // Block and transaction messages are exempt since we
                            // request them ourselves via getdata.
                            let is_data_msg = matches!(
                                &msg,
                                NetworkMessage::Block(_)
                                    | NetworkMessage::Tx(_)
                                    | NetworkMessage::Headers(_)
                            );

                            if !is_data_msg {
                                if rate_window_start.elapsed() >= std::time::Duration::from_secs(1) {
                                    rate_window_start = std::time::Instant::now();
                                    rate_count = 0;
                                }
                                rate_count += 1;
                                if rate_count > Self::MAX_PROTOCOL_MESSAGES_PER_SEC {
                                    warn!(peer = ?peer_id, rate = rate_count, "peer exceeding protocol message rate limit — disconnecting");
                                    break;
                                }
                            }

                            // Update last-seen timestamp
                            if let Some(mut peer) = peers.get_mut(&peer_id) {
                                peer.last_seen = std::time::Instant::now();
                            }

                            match &msg {
                                NetworkMessage::Ping(nonce) => {
                                    if let Err(e) = conn.send(NetworkMessage::Pong(*nonce)).await {
                                        debug!(?e, "failed to send pong");
                                        break;
                                    }
                                    continue;
                                }
                                NetworkMessage::Pong(nonce) => {
                                    if let Some(mut peer) = peers.get_mut(&peer_id) {
                                        if peer.ping_nonce == Some(*nonce) {
                                            if let Some(sent) = peer.last_ping {
                                                peer.ping_latency_ms =
                                                    Some(sent.elapsed().as_millis() as u64);
                                            }
                                            peer.ping_nonce = None;
                                        }
                                    }
                                    continue;
                                }
                                _ => {}
                            }

                            // Forward all other messages to the node
                            if event_tx
                                .send(PeerEvent::Message(peer_id, msg))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(P2pError::Disconnected) => {
                            info!(peer = ?peer_id, "peer disconnected");
                            break;
                        }
                        Err(P2pError::Encode(_)) => {
                            // Checksum or decode error — the message bytes were
                            // already fully consumed from the stream so it's still
                            // aligned for the next message. Skip and continue.
                            debug!(peer = ?peer_id, "skipping corrupted message (checksum/decode error)");
                            continue;
                        }
                        Err(e) => {
                            warn!(peer = ?peer_id, ?e, "peer error");
                            break;
                        }
                    }
                }

                // Outbound: the node wants to send a message to this peer
                Some(msg) = outbound_rx.recv() => {
                    if let Err(e) = conn.send(msg).await {
                        warn!(peer = ?peer_id, ?e, "failed to send message to peer");
                        break;
                    }
                }

                // Keepalive: send periodic pings
                _ = ping_interval.tick() => {
                    // Check if previous ping is still outstanding (stale peer)
                    if let Some(peer) = peers.get(&peer_id) {
                        if let Some(last_ping) = peer.last_ping {
                            if peer.ping_nonce.is_some() && last_ping.elapsed() > Self::PING_TIMEOUT {
                                info!(peer = ?peer_id, "peer failed to respond to ping — disconnecting");
                                break;
                            }
                        }
                    }

                    let nonce: u64 = rand::random();
                    if let Some(mut peer) = peers.get_mut(&peer_id) {
                        peer.last_ping = Some(std::time::Instant::now());
                        peer.ping_nonce = Some(nonce);
                    }
                    if let Err(e) = conn.send(NetworkMessage::Ping(nonce)).await {
                        debug!(peer = ?peer_id, ?e, "failed to send ping");
                        break;
                    }
                }
            }
        }
    }

    /// Discover peers via DNS seeds.
    async fn discover_peers(&self) {
        let port = default_port_for_network(self.network);

        let seeds: Vec<String> = if self.config.dns_seeds.is_empty() {
            dns_seeds_for_network(self.network)
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            self.config.dns_seeds.clone()
        };

        if seeds.is_empty() {
            info!("no DNS seeds configured for this network");
            return;
        }

        for seed in &seeds {
            debug!(seed, "resolving DNS seed");
            match tokio::net::lookup_host(format!("{}:{}", seed, port)).await {
                Ok(addrs) => {
                    let addrs: Vec<SocketAddr> = addrs.collect();
                    info!(seed, count = addrs.len(), "resolved DNS seed");

                    for addr in addrs.into_iter().take(3) {
                        let outbound_count = self.peers.iter().filter(|p| !p.info.inbound).count();
                        if outbound_count >= self.config.max_outbound {
                            return;
                        }
                        self.connect_outbound(addr).await;
                    }
                }
                Err(e) => {
                    debug!(seed, ?e, "DNS seed resolution failed");
                }
            }
        }
    }

    /// Send a message to a specific peer.
    pub async fn send_to_peer(&self, peer_id: PeerId, msg: NetworkMessage) -> Result<(), P2pError> {
        match self.peer_senders.get(&peer_id) {
            Some(sender) => {
                sender
                    .send(msg)
                    .await
                    .map_err(|_| P2pError::ChannelClosed)?;
                Ok(())
            }
            None => {
                debug!(peer = ?peer_id, "send_to_peer: peer not found");
                Err(P2pError::Disconnected)
            }
        }
    }

    /// Broadcast a message to all connected peers.
    pub async fn broadcast(&self, msg: NetworkMessage) -> Result<(), P2pError> {
        for entry in self.peer_senders.iter() {
            // Best-effort: skip peers whose channel is full
            let _ = entry.value().try_send(msg.clone());
        }
        Ok(())
    }
}
