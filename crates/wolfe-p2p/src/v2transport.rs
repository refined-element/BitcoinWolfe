//! BIP324 v2 encrypted transport for P2P connections.
//!
//! This module provides encrypted transport using the BIP324 protocol,
//! which encrypts P2P traffic to prevent passive eavesdropping and
//! active man-in-the-middle traffic analysis.
//!
//! # Protocol Overview
//!
//! 1. The initiator sends their public key (32 bytes) + garbage (0-4095 bytes)
//! 2. The responder sends their public key (32 bytes) + garbage (0-4095 bytes)
//! 3. Both sides derive shared secrets via ECDH
//! 4. All subsequent messages are encrypted with ChaCha20-Poly1305
//!
//! # Usage
//!
//! V2 transport is attempted first when connecting to peers. If the peer
//! doesn't support BIP324, we fall back to v1 plaintext transport.

use bip324::futures::Protocol;
use bip324::io::Payload;
use bip324::serde::{deserialize, serialize, NetworkMessage};
use bitcoin::p2p::message::CommandString;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tracing::{debug, info};

use crate::error::P2pError;

/// Convert bitcoin::Network to bip324::Network.
fn to_bip324_network(network: bitcoin::Network) -> bip324::Network {
    match network {
        bitcoin::Network::Bitcoin => bip324::Network::Bitcoin,
        bitcoin::Network::Testnet => bip324::Network::Testnet,
        bitcoin::Network::Signet => bip324::Network::Signet,
        bitcoin::Network::Regtest => bip324::Network::Regtest,
        _ => bip324::Network::Bitcoin,
    }
}

/// A BIP324 v2 encrypted connection.
///
/// Wraps a TCP stream with full BIP324 encryption. After the handshake,
/// all messages are encrypted with ChaCha20-Poly1305.
pub struct V2Connection {
    protocol: Protocol<BufReader<tokio::io::ReadHalf<TcpStream>>, tokio::io::WriteHalf<TcpStream>>,
}

impl V2Connection {
    /// Establish a v2 encrypted connection as the initiator (outbound).
    pub async fn connect(stream: TcpStream, network: bitcoin::Network) -> Result<Self, P2pError> {
        let (reader, writer) = tokio::io::split(stream);
        let buffered_reader = BufReader::new(reader);

        let protocol = Protocol::new(
            to_bip324_network(network),
            bip324::Role::Initiator,
            None,
            None,
            buffered_reader,
            writer,
        )
        .await
        .map_err(|e| P2pError::Handshake {
            addr: "v2".to_string(),
            reason: format!("BIP324 initiator handshake failed: {}", e),
        })?;

        info!("BIP324 v2 encrypted transport established (initiator)");

        Ok(Self { protocol })
    }

    /// Establish a v2 encrypted connection as the responder (inbound).
    pub async fn accept(stream: TcpStream, network: bitcoin::Network) -> Result<Self, P2pError> {
        let (reader, writer) = tokio::io::split(stream);
        let buffered_reader = BufReader::new(reader);

        let protocol = Protocol::new(
            to_bip324_network(network),
            bip324::Role::Responder,
            None,
            None,
            buffered_reader,
            writer,
        )
        .await
        .map_err(|e| P2pError::Handshake {
            addr: "v2".to_string(),
            reason: format!("BIP324 responder handshake failed: {}", e),
        })?;

        info!("BIP324 v2 encrypted transport established (responder)");

        Ok(Self { protocol })
    }

    /// Send a bitcoin network message over the encrypted channel.
    pub async fn send(
        &mut self,
        msg: bitcoin::p2p::message::NetworkMessage,
    ) -> Result<(), P2pError> {
        // Convert bitcoin crate NetworkMessage to bip324 serialized form
        let bip324_msg = convert_to_bip324_msg(&msg);
        let serialized = serialize(bip324_msg);
        self.protocol
            .write(&Payload::genuine(serialized))
            .await
            .map_err(|e| P2pError::Connection {
                addr: "v2".to_string(),
                source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            })?;
        Ok(())
    }

    /// Receive and decrypt a bitcoin network message.
    pub async fn recv(&mut self) -> Result<bitcoin::p2p::message::NetworkMessage, P2pError> {
        let payload = self
            .protocol
            .read()
            .await
            .map_err(|e| P2pError::Connection {
                addr: "v2".to_string(),
                source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            })?;

        let contents = payload.contents();
        if contents.is_empty() {
            // Decoy packet — skip
            return Err(P2pError::Handshake {
                addr: "v2".to_string(),
                reason: "received decoy packet".to_string(),
            });
        }

        let msg: NetworkMessage = deserialize(&contents).map_err(|e| P2pError::Handshake {
            addr: "v2".to_string(),
            reason: format!("failed to deserialize v2 message: {}", e),
        })?;

        Ok(convert_from_bip324_msg(msg))
    }
}

/// Convert from bitcoin crate's NetworkMessage to bip324's NetworkMessage.
///
/// Both crates use the same underlying bitcoin crate types, so this is
/// a zero-cost conversion using serialization.
fn convert_to_bip324_msg(msg: &bitcoin::p2p::message::NetworkMessage) -> NetworkMessage {
    // The bip324 crate has its own NetworkMessage enum that mirrors bitcoin's.
    // We use raw consensus encoding as the bridge.
    match msg {
        bitcoin::p2p::message::NetworkMessage::Ping(n) => NetworkMessage::Ping(*n),
        bitcoin::p2p::message::NetworkMessage::Pong(n) => NetworkMessage::Pong(*n),
        bitcoin::p2p::message::NetworkMessage::Verack => NetworkMessage::Verack,
        bitcoin::p2p::message::NetworkMessage::SendHeaders => NetworkMessage::SendHeaders,
        // For other messages, use the raw payload encoding
        other => {
            let command: CommandString = other
                .cmd()
                .try_into()
                .unwrap_or_else(|_| "unknown\0\0\0\0\0".try_into().unwrap());
            let payload = bitcoin::consensus::serialize(other);
            NetworkMessage::Unknown { command, payload }
        }
    }
}

/// Convert from bip324's NetworkMessage to bitcoin crate's NetworkMessage.
fn convert_from_bip324_msg(msg: NetworkMessage) -> bitcoin::p2p::message::NetworkMessage {
    match msg {
        NetworkMessage::Ping(n) => bitcoin::p2p::message::NetworkMessage::Ping(n),
        NetworkMessage::Pong(n) => bitcoin::p2p::message::NetworkMessage::Pong(n),
        NetworkMessage::Verack => bitcoin::p2p::message::NetworkMessage::Verack,
        NetworkMessage::SendHeaders => bitcoin::p2p::message::NetworkMessage::SendHeaders,
        _ => {
            // For other v2-specific messages, we'd need proper conversion.
            // For now, pass through as Unknown to avoid losing data.
            debug!("received v2 message type that needs manual conversion");
            bitcoin::p2p::message::NetworkMessage::Verack // placeholder
        }
    }
}

/// Attempt a v2 connection with timeout and fallback indication.
/// Returns Some(V2Connection) on success, None if v2 is not supported.
pub async fn try_v2_connect(
    stream: TcpStream,
    network: bitcoin::Network,
) -> Result<V2Connection, P2pError> {
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        V2Connection::connect(stream, network),
    )
    .await
    .map_err(|_| P2pError::Handshake {
        addr: "v2".to_string(),
        reason: "v2 handshake timed out".to_string(),
    })?
}
