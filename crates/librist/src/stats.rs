//! Statistics types for RIST sessions.

use std::ffi::CStr;

/// Statistics for a sender peer.
///
/// These statistics are provided via the stats callback on the sender.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SenderStats {
    /// The peer ID this stats relates to.
    pub peer_id: u32,
    /// The CNAME (canonical name) of the peer.
    pub cname: String,
    /// Current bandwidth in bytes per second.
    pub bandwidth: u64,
    /// Bandwidth used for retransmissions in bytes per second.
    pub retry_bandwidth: u64,
    /// Total packets sent.
    pub sent_packets: u64,
    /// Total packets acknowledged by receiver.
    pub received_packets: u64,
    /// Total packets retransmitted.
    pub retransmitted_packets: u64,
    /// Link quality (0.0 - 100.0, where 100.0 is perfect).
    pub quality: f64,
    /// Current round-trip time in milliseconds.
    pub rtt_ms: u32,
}

impl SenderStats {
    /// Creates stats from the raw librist structure.
    pub(crate) fn from_raw(raw: &librist_sys::rist_stats_sender_peer) -> Self {
        let cname = unsafe {
            CStr::from_ptr(raw.cname.as_ptr())
                .to_string_lossy()
                .into_owned()
        };

        Self {
            peer_id: raw.peer_id,
            cname,
            bandwidth: raw.bandwidth as u64,
            retry_bandwidth: raw.retry_bandwidth as u64,
            sent_packets: raw.sent,
            received_packets: raw.received,
            retransmitted_packets: raw.retransmitted,
            quality: raw.quality,
            rtt_ms: raw.rtt,
        }
    }

    /// Returns the packet loss ratio (0.0 - 1.0).
    pub fn loss_ratio(&self) -> f64 {
        if self.sent_packets == 0 {
            0.0
        } else {
            1.0 - (self.received_packets as f64 / self.sent_packets as f64)
        }
    }

    /// Returns the retransmission ratio (0.0 - 1.0).
    pub fn retransmit_ratio(&self) -> f64 {
        if self.sent_packets == 0 {
            0.0
        } else {
            self.retransmitted_packets as f64 / self.sent_packets as f64
        }
    }
}

/// Statistics for a single receiver peer.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReceiverPeerStats {
    /// The peer ID.
    pub peer_id: u32,
    /// Total data received in bytes.
    pub received_data: u64,
    /// Total RTCP packets received.
    pub received_rtcp: u32,
    /// Total RTCP packets sent.
    pub sent_rtcp: u32,
    /// Current round-trip time in microseconds.
    pub rtt_us: u64,
    /// Average round-trip time in microseconds.
    pub avg_rtt_us: f64,
    /// Current bandwidth in bytes per second.
    pub bandwidth: u64,
    /// Average bandwidth in bytes per second.
    pub avg_bandwidth: u64,
}

impl ReceiverPeerStats {
    /// Creates stats from the raw librist structure.
    pub(crate) fn from_raw(raw: &librist_sys::rist_stats_receiver_peer) -> Self {
        Self {
            peer_id: raw.peer_id,
            received_data: raw.received_data,
            received_rtcp: raw.received_rtcp,
            sent_rtcp: raw.sent_rtcp,
            rtt_us: raw.rtt,
            avg_rtt_us: raw.avg_rtt,
            bandwidth: raw.bandwidth as u64,
            avg_bandwidth: raw.avg_bandwidth as u64,
        }
    }

    /// Returns the RTT in milliseconds.
    pub fn rtt_ms(&self) -> f64 {
        self.rtt_us as f64 / 1000.0
    }

    /// Returns the average RTT in milliseconds.
    pub fn avg_rtt_ms(&self) -> f64 {
        self.avg_rtt_us / 1000.0
    }
}

/// Statistics for a receiver flow.
///
/// A flow represents a complete data stream from one or more peers.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReceiverStats {
    /// The flow ID.
    pub flow_id: u32,
    /// The CNAME (canonical name) of the flow.
    pub cname: String,
    /// Flow status (implementation-specific).
    pub status: i32,
    /// Current bandwidth in bytes per second.
    pub bandwidth: u64,
    /// Bandwidth used for retransmissions in bytes per second.
    pub retry_bandwidth: u64,
    /// Total packets sent (by sender).
    pub sent_packets: u64,
    /// Total packets received.
    pub received_packets: u64,
    /// Packets currently missing.
    pub missing_packets: u32,
    /// Packets received out of order.
    pub reordered_packets: u32,
    /// Packets successfully recovered via retransmission.
    pub recovered_packets: u32,
    /// Packets recovered on first retry.
    pub recovered_one_retry: u32,
    /// Packets lost (could not be recovered).
    pub lost_packets: u32,
    /// Link quality (0.0 - 100.0, where 100.0 is perfect).
    pub quality: f64,
    /// Minimum inter-packet spacing in nanoseconds.
    pub min_inter_packet_spacing_ns: u64,
    /// Current inter-packet spacing in nanoseconds.
    pub cur_inter_packet_spacing_ns: u64,
    /// Maximum inter-packet spacing in nanoseconds.
    pub max_inter_packet_spacing_ns: u64,
    /// Current round-trip time in milliseconds.
    pub rtt_ms: u32,
    /// Average receiver buffer fill level in microseconds (dynamic buffer depth).
    pub avg_buffer_time_us: u64,
    /// Per-peer statistics.
    pub peers: Vec<ReceiverPeerStats>,
}

impl ReceiverStats {
    /// Creates stats from the raw librist structure.
    ///
    /// # Safety
    ///
    /// The raw stats must be valid and the peers array must be properly allocated.
    pub(crate) unsafe fn from_raw(raw: &librist_sys::rist_stats_receiver_flow) -> Self {
        // SAFETY: cname is a valid C string within the struct
        let cname = unsafe {
            CStr::from_ptr(raw.cname.as_ptr())
                .to_string_lossy()
                .into_owned()
        };

        // Convert peer stats
        let peers = if raw.peer_count > 0 && !raw.peers.is_null() {
            // SAFETY: peers pointer is valid and peer_count is accurate
            let peer_slice =
                unsafe { std::slice::from_raw_parts(raw.peers, raw.peer_count as usize) };
            peer_slice.iter().map(ReceiverPeerStats::from_raw).collect()
        } else {
            Vec::new()
        };

        Self {
            flow_id: raw.flow_id,
            cname,
            status: raw.status,
            bandwidth: raw.bandwidth as u64,
            retry_bandwidth: raw.retry_bandwidth as u64,
            sent_packets: raw.sent,
            received_packets: raw.received,
            missing_packets: raw.missing,
            reordered_packets: raw.reordered,
            recovered_packets: raw.recovered,
            recovered_one_retry: raw.recovered_one_retry,
            lost_packets: raw.lost,
            quality: raw.quality,
            min_inter_packet_spacing_ns: raw.min_inter_packet_spacing,
            cur_inter_packet_spacing_ns: raw.cur_inter_packet_spacing,
            max_inter_packet_spacing_ns: raw.max_inter_packet_spacing,
            rtt_ms: raw.rtt,
            avg_buffer_time_us: raw.avg_buffer_time,
            peers,
        }
    }

    /// Returns the packet loss ratio (0.0 - 1.0).
    pub fn loss_ratio(&self) -> f64 {
        let total = self.received_packets + self.lost_packets as u64;
        if total == 0 {
            0.0
        } else {
            self.lost_packets as f64 / total as f64
        }
    }

    /// Returns the recovery success ratio (0.0 - 1.0).
    ///
    /// This is the ratio of recovered packets to total lost + recovered packets.
    pub fn recovery_ratio(&self) -> f64 {
        let total = self.recovered_packets + self.lost_packets;
        if total == 0 {
            1.0 // No losses = 100% recovery
        } else {
            self.recovered_packets as f64 / total as f64
        }
    }
}

/// Wrapper for librist stats that handles cleanup.
pub(crate) struct StatsWrapper {
    stats: *const librist_sys::rist_stats,
}

impl StatsWrapper {
    /// Creates a new stats wrapper from a raw pointer.
    ///
    /// # Safety
    ///
    /// The pointer must be valid and will be freed when this wrapper is dropped.
    pub(crate) unsafe fn from_raw(stats: *const librist_sys::rist_stats) -> Self {
        Self { stats }
    }

    /// Returns the raw stats pointer.
    #[allow(dead_code)]
    pub(crate) fn as_raw(&self) -> *const librist_sys::rist_stats {
        self.stats
    }

    /// Gets the stats type.
    pub(crate) fn stats_type(&self) -> librist_sys::rist_stats_type {
        unsafe { (*self.stats).stats_type }
    }

    /// Extracts sender stats if this is a sender stats type.
    pub(crate) fn as_sender_stats(&self) -> Option<SenderStats> {
        if self.stats_type() == librist_sys::rist_stats_type::RIST_STATS_SENDER_PEER {
            Some(SenderStats::from_raw(unsafe {
                &(*self.stats).stats.sender_peer
            }))
        } else {
            None
        }
    }

    /// Extracts receiver stats if this is a receiver stats type.
    pub(crate) fn as_receiver_stats(&self) -> Option<ReceiverStats> {
        if self.stats_type() == librist_sys::rist_stats_type::RIST_STATS_RECEIVER_FLOW {
            Some(unsafe { ReceiverStats::from_raw(&(*self.stats).stats.receiver_flow) })
        } else {
            None
        }
    }
}

impl Drop for StatsWrapper {
    fn drop(&mut self) {
        if !self.stats.is_null() {
            unsafe {
                librist_sys::rist_stats_free(self.stats);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sender_stats_ratios() {
        let stats = SenderStats {
            peer_id: 1,
            cname: "test".to_string(),
            bandwidth: 1000000,
            retry_bandwidth: 10000,
            sent_packets: 1000,
            received_packets: 990,
            retransmitted_packets: 15,
            quality: 99.0,
            rtt_ms: 50,
        };

        assert!((stats.loss_ratio() - 0.01).abs() < 0.001);
        assert!((stats.retransmit_ratio() - 0.015).abs() < 0.001);
    }

    #[test]
    fn test_receiver_stats_ratios() {
        let stats = ReceiverStats {
            flow_id: 1,
            cname: "test".to_string(),
            status: 0,
            bandwidth: 1000000,
            retry_bandwidth: 10000,
            sent_packets: 1000,
            received_packets: 990,
            missing_packets: 0,
            reordered_packets: 5,
            recovered_packets: 8,
            recovered_one_retry: 6,
            lost_packets: 2,
            quality: 99.0,
            min_inter_packet_spacing_ns: 1000,
            cur_inter_packet_spacing_ns: 1500,
            max_inter_packet_spacing_ns: 2000,
            rtt_ms: 50,
            avg_buffer_time_us: 0,
            peers: vec![],
        };

        assert!((stats.recovery_ratio() - 0.8).abs() < 0.001);
    }
}
