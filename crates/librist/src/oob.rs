//! Out-of-band (OOB) data support for RIST.
//!
//! OOB data is transmitted via the RTCP channel (not the RTP data channel)
//! with no buffering or retransmission. This makes it suitable for low-latency
//! signaling and control messages, but unsuitable for reliable data transfer.
//!
//! # Requirements
//!
//! - Only available with Main or Advanced profile (not Simple)
//! - Must enable OOB by setting a callback before calling `start()`
//! - Maximum payload size is approximately 10KB (RIST_MAX_PACKET_SIZE - 16)
//!
//! # Example
//!
//! ```no_run
//! use librist::{RistReceiver, RistSender, Profile, OobBlock};
//!
//! // Receiver with OOB callback
//! let receiver = RistReceiver::builder()
//!     .profile(Profile::Main)
//!     .on_oob(|block| {
//!         println!("Received OOB: {} bytes", block.payload().len());
//!     })
//!     .build()?;
//!
//! // Sender can send OOB data
//! let sender = RistSender::builder()
//!     .profile(Profile::Main)
//!     .enable_oob()  // Required to enable OOB channel
//!     .build()?;
//!
//! // After starting, send OOB data
//! sender.start()?;
//! sender.send_oob(b"Hello via OOB")?;
//! # Ok::<(), librist::Error>(())
//! ```

use crate::peer::PeerHandle;

/// Maximum OOB payload size (approximately 10KB).
///
/// This is `RIST_MAX_PACKET_SIZE - 16` to account for the GRE header overhead.
pub const MAX_OOB_PAYLOAD_SIZE: usize = 10000;

/// An out-of-band data block.
///
/// OOB blocks are used to send and receive out-of-band data via the RTCP channel.
/// Unlike regular data, OOB data:
/// - Bypasses the jitter buffer (no delay)
/// - Has no packet recovery (lost packets are not retransmitted)
/// - Is limited to approximately 10KB per message
#[derive(Debug, Clone)]
pub struct OobBlock {
    /// The payload data
    payload: Vec<u8>,
    /// NTP timestamp (set by librist on receive)
    ts_ntp: u64,
    /// Peer ID that sent this data (on receive) or target peer (on send)
    peer_id: Option<u32>,
}

impl OobBlock {
    /// Creates a new OOB block with the given payload.
    ///
    /// # Panics
    ///
    /// Panics if payload exceeds `MAX_OOB_PAYLOAD_SIZE`.
    pub fn new(payload: impl Into<Vec<u8>>) -> Self {
        let payload = payload.into();
        assert!(
            payload.len() <= MAX_OOB_PAYLOAD_SIZE,
            "OOB payload exceeds maximum size of {} bytes",
            MAX_OOB_PAYLOAD_SIZE
        );
        Self {
            payload,
            ts_ntp: 0,
            peer_id: None,
        }
    }

    /// Creates an OOB block targeting a specific peer.
    ///
    /// If no peer is specified, the data is sent to the default peer.
    pub fn with_peer(mut self, peer: &PeerHandle) -> Self {
        self.peer_id = Some(peer.id());
        self
    }

    /// Returns the payload data.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Returns the NTP timestamp (set by librist on receive).
    pub fn timestamp_ntp(&self) -> u64 {
        self.ts_ntp
    }

    /// Returns the peer ID (sender on receive, target on send).
    pub fn peer_id(&self) -> Option<u32> {
        self.peer_id
    }

    /// Creates an OOB block from received librist data.
    ///
    /// # Safety
    ///
    /// The raw block must be valid and point to valid memory.
    pub(crate) unsafe fn from_raw(raw: *const librist_sys::rist_oob_block) -> Self {
        let block = unsafe { &*raw };
        let payload = if block.payload.is_null() || block.payload_len == 0 {
            Vec::new()
        } else {
            unsafe {
                std::slice::from_raw_parts(block.payload as *const u8, block.payload_len).to_vec()
            }
        };

        let peer_id = if block.peer.is_null() {
            None
        } else {
            Some(unsafe { librist_sys::rist_peer_get_id(block.peer) })
        };

        Self {
            payload,
            ts_ntp: block.ts_ntp,
            peer_id,
        }
    }
}

/// Builder for creating OOB blocks with a fluent API.
#[derive(Debug, Default)]
pub struct OobBlockBuilder {
    peer_id: Option<u32>,
}

impl OobBlockBuilder {
    /// Creates a new OOB block builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the target peer for the OOB data.
    pub fn peer(mut self, peer: &PeerHandle) -> Self {
        self.peer_id = Some(peer.id());
        self
    }

    /// Builds an OOB block with the given payload.
    ///
    /// # Panics
    ///
    /// Panics if payload exceeds `MAX_OOB_PAYLOAD_SIZE`.
    pub fn build(self, payload: impl Into<Vec<u8>>) -> OobBlock {
        let mut block = OobBlock::new(payload);
        block.peer_id = self.peer_id;
        block
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oob_block_new() {
        let block = OobBlock::new(b"test data".to_vec());
        assert_eq!(block.payload(), b"test data");
        assert_eq!(block.timestamp_ntp(), 0);
        assert_eq!(block.peer_id(), None);
    }

    #[test]
    fn test_oob_block_builder() {
        let block = OobBlockBuilder::new().build(b"hello".to_vec());
        assert_eq!(block.payload(), b"hello");
    }

    #[test]
    #[should_panic(expected = "OOB payload exceeds maximum size")]
    fn test_oob_block_too_large() {
        let large_payload = vec![0u8; MAX_OOB_PAYLOAD_SIZE + 1];
        let _block = OobBlock::new(large_payload);
    }

    #[test]
    fn test_oob_block_max_size() {
        let max_payload = vec![0u8; MAX_OOB_PAYLOAD_SIZE];
        let block = OobBlock::new(max_payload);
        assert_eq!(block.payload().len(), MAX_OOB_PAYLOAD_SIZE);
    }
}
