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
        for addr_str in &self.config.connect {
            if let Ok(addr) = addr_str.parse::<SocketAddr>() {
                let this = self.clone();
                tokio::spawn(async move {
                    this.connect_outbound(addr).await;
                });
            }
        }

        // If no manual peers, try DNS seeds
        if self.config.connect.is_empty() {
            let this = self.clone();
            tokio::spawn(async move {
                this.discover_peers().await;
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
                        let info = conn.info.clone();
                        let (msg_tx, msg_rx) = mpsc::channel(256);
                        peers.insert(peer_id, Peer::new(info.clone()));
                        peer_senders.insert(peer_id, msg_tx);
                        let _ = event_tx.send(PeerEvent::Connected(info)).await;
                        Self::run_peer_loop(&mut conn, msg_rx, &event_tx, &peers).await;
                        peer_senders.remove(&peer_id);
                        peers.remove(&peer_id);
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
                let info = conn.info.clone();
                let (msg_tx, msg_rx) = mpsc::channel(256);
                self.peers.insert(peer_id, Peer::new(info.clone()));
                self.peer_senders.insert(peer_id, msg_tx);
                let _ = self.event_tx.send(PeerEvent::Connected(info)).await;

                let event_tx = self.event_tx.clone();
                let peers = self.peers.clone();
                let peer_senders = self.peer_senders.clone();
                tokio::spawn(async move {
                    Self::run_peer_loop(&mut conn, msg_rx, &event_tx, &peers).await;
                    peer_senders.remove(&peer_id);
                    peers.remove(&peer_id);
                    let _ = event_tx.send(PeerEvent::Disconnected(peer_id)).await;
                });
            }
            Err(e) => {
                debug!(%addr, ?e, "outbound connection failed");
            }
        }
    }

    /// Main message loop for a single peer.
    ///
    /// Uses `tokio::select!` to concurrently:
    /// - Read inbound messages from the TCP stream and forward them to the node
    /// - Read outbound messages from the per-peer channel and write them to TCP
    async fn run_peer_loop(
        conn: &mut PeerConnection,
        mut outbound_rx: mpsc::Receiver<NetworkMessage>,
        event_tx: &mpsc::Sender<PeerEvent>,
        peers: &DashMap<PeerId, Peer>,
    ) {
        let peer_id = conn.info.id;
        loop {
            tokio::select! {
                // Inbound: read a message from the peer over TCP
                recv_result = conn.recv() => {
                    match recv_result {
                        Ok(msg) => {
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
