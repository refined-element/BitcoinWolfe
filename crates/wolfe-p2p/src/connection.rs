use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::{address, ServiceFlags};
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;
use tracing::{debug, info, warn};

use crate::error::P2pError;
use crate::message::{magic_for_network, MessageCodec};
use crate::peer::{PeerId, PeerInfo};

/// Manages a single TCP connection to a Bitcoin peer.
pub struct PeerConnection {
    pub info: PeerInfo,
    codec: MessageCodec,
    reader: tokio::io::ReadHalf<TcpStream>,
    writer: tokio::io::WriteHalf<TcpStream>,
}

impl PeerConnection {
    /// Connect to a peer and perform the version handshake.
    pub async fn connect(
        addr: SocketAddr,
        network: bitcoin::Network,
        our_services: ServiceFlags,
        our_best_height: i32,
        peer_id: PeerId,
    ) -> Result<Self, P2pError> {
        debug!(%addr, "connecting to peer");

        let stream =
            tokio::time::timeout(std::time::Duration::from_secs(10), TcpStream::connect(addr))
                .await
                .map_err(|_| P2pError::Connection {
                    addr: addr.to_string(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "connection timed out",
                    ),
                })?
                .map_err(|e| P2pError::Connection {
                    addr: addr.to_string(),
                    source: e,
                })?;

        let magic = magic_for_network(network);
        let mut codec = MessageCodec::new(magic);
        let (mut reader, mut writer) = tokio::io::split(stream);

        // Build and send our version message
        let version_msg = build_version_message(addr, our_services, our_best_height);
        codec
            .write_message(&mut writer, NetworkMessage::Version(version_msg))
            .await?;

        // Read their version message
        let their_version = match codec.read_message(&mut reader).await? {
            NetworkMessage::Version(v) => v,
            other => {
                return Err(P2pError::Handshake {
                    addr: addr.to_string(),
                    reason: format!("expected version, got {:?}", other.cmd()),
                });
            }
        };

        // Send verack
        codec
            .write_message(&mut writer, NetworkMessage::Verack)
            .await?;

        // Read their verack
        match codec.read_message(&mut reader).await? {
            NetworkMessage::Verack => {}
            other => {
                warn!(%addr, cmd = ?other.cmd(), "expected verack, got something else");
            }
        }

        let info = PeerInfo {
            id: peer_id,
            addr,
            user_agent: their_version.user_agent.clone(),
            version: their_version.version as u32,
            services: their_version.services,
            start_height: their_version.start_height,
            relay: their_version.relay,
            inbound: false,
            v2_transport: false,
            connected_at: std::time::Instant::now(),
        };

        info!(
            %addr,
            user_agent = %their_version.user_agent,
            version = their_version.version,
            height = their_version.start_height,
            "peer connected"
        );

        Ok(Self {
            info,
            codec,
            reader,
            writer,
        })
    }

    /// Accept an inbound connection and perform handshake.
    pub async fn accept(
        stream: TcpStream,
        addr: SocketAddr,
        network: bitcoin::Network,
        our_services: ServiceFlags,
        our_best_height: i32,
        peer_id: PeerId,
    ) -> Result<Self, P2pError> {
        let magic = magic_for_network(network);
        let mut codec = MessageCodec::new(magic);
        let (mut reader, mut writer) = tokio::io::split(stream);

        // Read their version first (they initiated)
        let their_version = match codec.read_message(&mut reader).await? {
            NetworkMessage::Version(v) => v,
            other => {
                return Err(P2pError::Handshake {
                    addr: addr.to_string(),
                    reason: format!("expected version, got {:?}", other.cmd()),
                });
            }
        };

        // Send our version
        let version_msg = build_version_message(addr, our_services, our_best_height);
        codec
            .write_message(&mut writer, NetworkMessage::Version(version_msg))
            .await?;

        // Send verack
        codec
            .write_message(&mut writer, NetworkMessage::Verack)
            .await?;

        // Read their verack
        let _ = codec.read_message(&mut reader).await?;

        let info = PeerInfo {
            id: peer_id,
            addr,
            user_agent: their_version.user_agent.clone(),
            version: their_version.version as u32,
            services: their_version.services,
            start_height: their_version.start_height,
            relay: their_version.relay,
            inbound: true,
            v2_transport: false,
            connected_at: std::time::Instant::now(),
        };

        info!(
            %addr,
            user_agent = %their_version.user_agent,
            "inbound peer connected"
        );

        Ok(Self {
            info,
            codec,
            reader,
            writer,
        })
    }

    /// Send a message to this peer.
    pub async fn send(&mut self, msg: NetworkMessage) -> Result<(), P2pError> {
        self.codec.write_message(&mut self.writer, msg).await
    }

    /// Receive the next message from this peer.
    pub async fn recv(&mut self) -> Result<NetworkMessage, P2pError> {
        self.codec.read_message(&mut self.reader).await
    }
}

fn build_version_message(
    addr: SocketAddr,
    services: ServiceFlags,
    best_height: i32,
) -> VersionMessage {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let receiver = address::Address::new(&addr, ServiceFlags::NONE);
    let sender_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let sender = address::Address::new(&sender_addr, services);

    let nonce = rand::random::<u64>();

    VersionMessage {
        version: 70016,
        services,
        timestamp,
        receiver,
        sender,
        nonce,
        user_agent: wolfe_types::user_agent(),
        start_height: best_height,
        relay: true,
    }
}
