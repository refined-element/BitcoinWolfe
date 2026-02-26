use bitcoin::consensus::{Decodable, Encodable};
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::Magic;
use bytes::BytesMut;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::P2pError;

/// Maximum size of a single P2P message (4MB, matching Bitcoin Core).
const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

/// Codec for reading/writing Bitcoin P2P messages over a TCP stream.
pub struct MessageCodec {
    magic: Magic,
    read_buf: BytesMut,
}

impl MessageCodec {
    pub fn new(magic: Magic) -> Self {
        Self {
            magic,
            read_buf: BytesMut::with_capacity(1024 * 64),
        }
    }

    /// Read the next P2P message from the stream.
    pub async fn read_message<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
    ) -> Result<NetworkMessage, P2pError> {
        // Read header (24 bytes for v1 protocol)
        // Magic(4) + Command(12) + Length(4) + Checksum(4)
        let mut header = [0u8; 24];
        reader.read_exact(&mut header).await?;

        // Parse payload length from header bytes 16..20 (little-endian u32)
        let payload_len =
            u32::from_le_bytes([header[16], header[17], header[18], header[19]]) as usize;

        if payload_len > MAX_MESSAGE_SIZE {
            return Err(P2pError::InvalidMessage {
                addr: String::new(),
                reason: format!("message too large: {} bytes", payload_len),
            });
        }

        // Read the full payload
        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            reader.read_exact(&mut payload).await?;
        }

        // Combine header + payload for decoding
        self.read_buf.clear();
        self.read_buf.extend_from_slice(&header);
        self.read_buf.extend_from_slice(&payload);

        let raw: RawNetworkMessage =
            Decodable::consensus_decode(&mut self.read_buf.as_ref()).map_err(P2pError::Encode)?;

        Ok(raw.payload().clone())
    }

    /// Write a P2P message to the stream.
    pub async fn write_message<W: AsyncWrite + Unpin>(
        &self,
        writer: &mut W,
        msg: NetworkMessage,
    ) -> Result<(), P2pError> {
        let raw = RawNetworkMessage::new(self.magic, msg);
        let mut buf = Vec::new();
        raw.consensus_encode(&mut buf)
            .map_err(|e| P2pError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        writer.write_all(&buf).await?;
        writer.flush().await?;
        Ok(())
    }
}

/// Determine the network magic bytes for a given bitcoin network.
pub fn magic_for_network(network: bitcoin::Network) -> Magic {
    Magic::from_params(network.params())
}
