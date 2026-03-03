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

    /// Maximum bytes to scan forward looking for magic before giving up.
    const MAX_RESYNC_BYTES: usize = 1024 * 1024; // 1 MB

    /// Read the next P2P message from the stream.
    ///
    /// If the first 4 bytes don't match the expected network magic, scans
    /// forward byte-by-byte to re-synchronize the stream. This handles rare
    /// TCP framing issues during rapid block download (IBD).
    pub async fn read_message<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
    ) -> Result<NetworkMessage, P2pError> {
        // Read header (24 bytes for v1 protocol)
        // Magic(4) + Command(12) + Length(4) + Checksum(4)
        let mut header = [0u8; 24];
        reader.read_exact(&mut header).await?;

        // Validate network magic bytes (first 4 bytes must match our network)
        let expected = self.magic.to_bytes();
        if header[0..4] != expected {
            // Stream desync — scan forward to find the magic bytes.
            // Shift the 24 bytes we already read into a sliding window and
            // read one byte at a time until we find the magic sequence.
            let mut window = header.to_vec();
            let mut scanned = 0usize;

            loop {
                // Search for magic in the current window
                if let Some(pos) = window.windows(4).position(|w| w == expected) {
                    // Found magic at `pos`. We need bytes [pos..pos+24] as
                    // the full header. We may already have some of them.
                    let available = window.len() - pos;
                    if available >= 24 {
                        header.copy_from_slice(&window[pos..pos + 24]);
                    } else {
                        header[..available].copy_from_slice(&window[pos..]);
                        reader.read_exact(&mut header[available..]).await?;
                    }
                    tracing::debug!(skipped_bytes = scanned + pos, "re-synced P2P stream");
                    break;
                }

                scanned += window.len().saturating_sub(3);
                if scanned > Self::MAX_RESYNC_BYTES {
                    return Err(P2pError::InvalidMessage {
                        addr: String::new(),
                        reason: format!(
                            "could not re-sync stream after scanning {} bytes",
                            scanned
                        ),
                    });
                }

                // Keep last 3 bytes (magic could straddle the boundary) and
                // read a fresh chunk.
                let keep = window.len().min(3);
                let tail: Vec<u8> = window[window.len() - keep..].to_vec();
                window.clear();
                window.extend_from_slice(&tail);

                let mut chunk = [0u8; 512];
                let n = reader.read(&mut chunk).await?;
                if n == 0 {
                    return Err(P2pError::Io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "connection closed during stream re-sync",
                    )));
                }
                window.extend_from_slice(&chunk[..n]);
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::Hash as _;
    use bitcoin::p2p::address::Address;
    use bitcoin::p2p::message_blockdata::GetHeadersMessage;
    use bitcoin::p2p::message_network::VersionMessage;
    use bitcoin::p2p::{Magic, ServiceFlags};
    use bitcoin::Network;
    use std::io::Cursor;
    use std::net::SocketAddr;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    /// Build a MessageCodec wired to mainnet magic.
    fn mainnet_codec() -> MessageCodec {
        MessageCodec::new(Magic::BITCOIN)
    }

    /// Construct a representative VersionMessage for testing.
    fn sample_version_msg() -> NetworkMessage {
        let addr: SocketAddr = "127.0.0.1:8333".parse().unwrap();
        let address = Address::new(&addr, ServiceFlags::NONE);
        NetworkMessage::Version(VersionMessage::new(
            ServiceFlags::NETWORK,
            1_700_000_000, // timestamp
            address.clone(),
            address,
            0xCAFE_BEEF, // nonce
            "/BitcoinWolfe:0.1.0/".to_string(),
            850_000, // start_height
        ))
    }

    /// Round-trip helper: serialise `msg` via write_message, then deserialise
    /// it back with read_message and return the decoded message.
    async fn round_trip(msg: NetworkMessage) -> NetworkMessage {
        let codec = mainnet_codec();

        // Write into an in-memory buffer.
        let mut buf: Vec<u8> = Vec::new();
        codec.write_message(&mut buf, msg).await.unwrap();

        // Read back from the buffer via Cursor (implements AsyncRead).
        let mut reader = Cursor::new(buf);
        let mut read_codec = mainnet_codec();
        read_codec.read_message(&mut reader).await.unwrap()
    }

    // -----------------------------------------------------------------------
    // magic_for_network
    // -----------------------------------------------------------------------

    #[test]
    fn magic_for_network_mainnet() {
        assert_eq!(magic_for_network(Network::Bitcoin), Magic::BITCOIN);
    }

    #[test]
    fn magic_for_network_testnet() {
        assert_eq!(magic_for_network(Network::Testnet), Magic::TESTNET3);
    }

    #[test]
    fn magic_for_network_signet() {
        assert_eq!(magic_for_network(Network::Signet), Magic::SIGNET);
    }

    #[test]
    fn magic_for_network_regtest() {
        assert_eq!(magic_for_network(Network::Regtest), Magic::REGTEST);
    }

    #[test]
    fn magic_for_network_testnet4() {
        assert_eq!(magic_for_network(Network::Testnet4), Magic::TESTNET4);
    }

    // -----------------------------------------------------------------------
    // Round-trip tests (write then read)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn round_trip_version() {
        let original = sample_version_msg();
        let decoded = round_trip(original.clone()).await;
        assert_eq!(decoded, original);
    }

    #[tokio::test]
    async fn round_trip_verack() {
        let decoded = round_trip(NetworkMessage::Verack).await;
        assert_eq!(decoded, NetworkMessage::Verack);
    }

    #[tokio::test]
    async fn round_trip_ping() {
        let nonce: u64 = 0xDEAD_BEEF_CAFE_BABE;
        let decoded = round_trip(NetworkMessage::Ping(nonce)).await;
        assert_eq!(decoded, NetworkMessage::Ping(nonce));
    }

    #[tokio::test]
    async fn round_trip_pong() {
        let nonce: u64 = 42;
        let decoded = round_trip(NetworkMessage::Pong(nonce)).await;
        assert_eq!(decoded, NetworkMessage::Pong(nonce));
    }

    #[tokio::test]
    async fn round_trip_getheaders() {
        use bitcoin::BlockHash;

        let msg = NetworkMessage::GetHeaders(GetHeadersMessage::new(
            vec![BlockHash::all_zeros()],
            BlockHash::all_zeros(),
        ));
        let decoded = round_trip(msg.clone()).await;
        assert_eq!(decoded, msg);
    }

    #[tokio::test]
    async fn round_trip_sendheaders() {
        let decoded = round_trip(NetworkMessage::SendHeaders).await;
        assert_eq!(decoded, NetworkMessage::SendHeaders);
    }

    #[tokio::test]
    async fn round_trip_getaddr() {
        let decoded = round_trip(NetworkMessage::GetAddr).await;
        assert_eq!(decoded, NetworkMessage::GetAddr);
    }

    #[tokio::test]
    async fn round_trip_mempool() {
        let decoded = round_trip(NetworkMessage::MemPool).await;
        assert_eq!(decoded, NetworkMessage::MemPool);
    }

    // -----------------------------------------------------------------------
    // write_message serialisation sanity checks
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn write_message_starts_with_magic_bytes() {
        let codec = mainnet_codec();
        let mut buf: Vec<u8> = Vec::new();
        codec
            .write_message(&mut buf, NetworkMessage::Verack)
            .await
            .unwrap();

        // The first 4 bytes must be the mainnet magic.
        assert_eq!(&buf[0..4], &Magic::BITCOIN.to_bytes());
    }

    #[tokio::test]
    async fn write_message_header_payload_length_matches() {
        let codec = mainnet_codec();
        let msg = NetworkMessage::Ping(12345);
        let mut buf: Vec<u8> = Vec::new();
        codec.write_message(&mut buf, msg).await.unwrap();

        // Bytes 16..20 encode the payload length as little-endian u32.
        let payload_len = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]) as usize;

        // Ping payload is exactly 8 bytes (a u64 nonce).
        assert_eq!(payload_len, 8);
        // Total buffer: 24 header + 8 payload.
        assert_eq!(buf.len(), 24 + 8);
    }

    #[tokio::test]
    async fn write_verack_has_zero_length_payload() {
        let codec = mainnet_codec();
        let mut buf: Vec<u8> = Vec::new();
        codec
            .write_message(&mut buf, NetworkMessage::Verack)
            .await
            .unwrap();

        let payload_len = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]) as usize;
        assert_eq!(payload_len, 0);
        // Total buffer is exactly the 24-byte header.
        assert_eq!(buf.len(), 24);
    }

    // -----------------------------------------------------------------------
    // Error path: oversized message
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn read_rejects_oversized_payload() {
        // Craft a raw 24-byte header with a payload length exceeding 4 MB.
        let oversized_len: u32 = (4 * 1024 * 1024 + 1) as u32; // MAX_MESSAGE_SIZE + 1
        let mut header = [0u8; 24];

        // Set magic bytes (mainnet).
        header[0..4].copy_from_slice(&Magic::BITCOIN.to_bytes());
        // Command: "ping" padded with zeros (12 bytes).
        header[4..8].copy_from_slice(b"ping");
        // Payload length at bytes 16..20.
        header[16..20].copy_from_slice(&oversized_len.to_le_bytes());

        let mut reader = Cursor::new(header.to_vec());
        let mut codec = mainnet_codec();

        let result = codec.read_message(&mut reader).await;
        assert!(result.is_err(), "expected an error for oversized payload");

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("too large"),
            "error should mention 'too large', got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn read_accepts_payload_at_exact_limit() {
        // A payload of exactly MAX_MESSAGE_SIZE (4 MB) should pass the size
        // check. It will still fail at consensus decoding because we are not
        // providing a real message, but the size guard itself must not fire.
        let limit: u32 = (4 * 1024 * 1024) as u32;
        let mut header = [0u8; 24];
        header[0..4].copy_from_slice(&Magic::BITCOIN.to_bytes());
        header[4..8].copy_from_slice(b"ping");
        header[16..20].copy_from_slice(&limit.to_le_bytes());

        // We need header + `limit` bytes of payload. The codec will try to
        // read_exact that many bytes. We provide them (all zeros), which will
        // pass the size check but fail consensus decode -- and that is the
        // behaviour we want to verify: no "too large" error.
        let mut data = header.to_vec();
        data.resize(24 + limit as usize, 0);

        let mut reader = Cursor::new(data);
        let mut codec = mainnet_codec();

        let result = codec.read_message(&mut reader).await;
        // Must NOT be the "too large" variant.
        match &result {
            Err(P2pError::InvalidMessage { reason, .. }) => {
                panic!("size check should not fire at exact limit, got: {reason}");
            }
            // Any other error (Encode / Io) is fine -- the payload is garbage.
            Err(_) => {}
            // If it somehow decoded, that is also acceptable.
            Ok(_) => {}
        }
    }

    // -----------------------------------------------------------------------
    // Error path: truncated stream
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn read_errors_on_empty_stream() {
        let mut reader = Cursor::new(Vec::<u8>::new());
        let mut codec = mainnet_codec();

        let result = codec.read_message(&mut reader).await;
        assert!(result.is_err(), "reading from empty stream must fail");
    }

    #[tokio::test]
    async fn read_errors_on_truncated_header() {
        // Provide only 10 bytes -- less than the 24-byte header.
        let data = vec![0u8; 10];
        let mut reader = Cursor::new(data);
        let mut codec = mainnet_codec();

        let result = codec.read_message(&mut reader).await;
        assert!(result.is_err(), "truncated header must produce an error");
    }

    #[tokio::test]
    async fn read_errors_on_truncated_payload() {
        // Build a valid 24-byte header claiming 100 bytes of payload, but
        // only supply 50 actual payload bytes.
        let mut header = [0u8; 24];
        header[0..4].copy_from_slice(&Magic::BITCOIN.to_bytes());
        header[4..8].copy_from_slice(b"ping");
        let payload_len: u32 = 100;
        header[16..20].copy_from_slice(&payload_len.to_le_bytes());

        let mut data = header.to_vec();
        data.extend_from_slice(&[0u8; 50]); // only 50 of the claimed 100

        let mut reader = Cursor::new(data);
        let mut codec = mainnet_codec();

        let result = codec.read_message(&mut reader).await;
        assert!(result.is_err(), "truncated payload must produce an error");
    }

    // -----------------------------------------------------------------------
    // Round-trip via tokio::io::duplex (full-duplex async channel)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn round_trip_over_duplex_channel() {
        let (mut client, mut server) = tokio::io::duplex(8192);

        let write_codec = mainnet_codec();
        let original = NetworkMessage::Ping(0x1234_5678_9ABC_DEF0);

        // Writer side.
        write_codec
            .write_message(&mut client, original.clone())
            .await
            .unwrap();
        // Shutdown the write half so the reader sees EOF after the message.
        drop(client);

        // Reader side.
        let mut read_codec = mainnet_codec();
        let decoded = read_codec.read_message(&mut server).await.unwrap();
        assert_eq!(decoded, original);
    }

    // -----------------------------------------------------------------------
    // Multiple sequential messages on the same stream
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn sequential_messages_on_same_stream() {
        let codec = mainnet_codec();
        let messages: Vec<NetworkMessage> = vec![
            NetworkMessage::Verack,
            NetworkMessage::Ping(1),
            NetworkMessage::Pong(1),
            NetworkMessage::GetAddr,
            NetworkMessage::SendHeaders,
        ];

        // Write all messages into a single buffer.
        let mut buf: Vec<u8> = Vec::new();
        for msg in &messages {
            codec.write_message(&mut buf, msg.clone()).await.unwrap();
        }

        // Read them back one by one.
        let mut reader = Cursor::new(buf);
        let mut read_codec = mainnet_codec();
        for expected in &messages {
            let decoded = read_codec.read_message(&mut reader).await.unwrap();
            assert_eq!(&decoded, expected);
        }
    }

    // -----------------------------------------------------------------------
    // Codec isolation: two codecs with different magic bytes
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn read_rejects_wrong_network_magic() {
        // A codec configured for testnet must reject messages serialised
        // with mainnet magic. The resync logic scans for testnet magic but
        // the stream only contains mainnet bytes, so it hits EOF.
        let write_codec = MessageCodec::new(Magic::BITCOIN);
        let mut buf: Vec<u8> = Vec::new();
        write_codec
            .write_message(&mut buf, NetworkMessage::Verack)
            .await
            .unwrap();

        let mut reader = Cursor::new(buf);
        let mut read_codec = MessageCodec::new(Magic::TESTNET3);
        let result = read_codec.read_message(&mut reader).await;
        assert!(result.is_err(), "should reject wrong network magic");
    }

    #[tokio::test]
    async fn write_uses_codec_magic() {
        // A codec configured for testnet must embed testnet magic in the
        // serialised bytes, not mainnet magic.
        let codec = MessageCodec::new(Magic::TESTNET3);
        let mut buf: Vec<u8> = Vec::new();
        codec
            .write_message(&mut buf, NetworkMessage::Verack)
            .await
            .unwrap();

        assert_eq!(&buf[0..4], &Magic::TESTNET3.to_bytes());
    }

    // -----------------------------------------------------------------------
    // MessageCodec::new initialisation
    // -----------------------------------------------------------------------

    #[test]
    fn codec_new_sets_magic() {
        let codec = MessageCodec::new(Magic::SIGNET);
        assert_eq!(codec.magic, Magic::SIGNET);
    }

    #[test]
    fn codec_read_buf_starts_empty() {
        let codec = mainnet_codec();
        assert!(codec.read_buf.is_empty());
    }
}
