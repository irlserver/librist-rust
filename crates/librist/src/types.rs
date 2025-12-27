//! Common types and enums for librist.

/// RIST protocol profile.
///
/// The profile determines which features are available and
/// the level of protocol complexity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum Profile {
    /// Simple profile - basic functionality, minimal overhead.
    ///
    /// Best for simple point-to-point connections where
    /// advanced features are not needed.
    Simple = 0,

    /// Main profile - recommended for most applications.
    ///
    /// Provides a good balance of features and simplicity,
    /// including encryption and bonding support.
    #[default]
    Main = 1,

    /// Advanced profile - full feature set.
    ///
    /// Includes all Main profile features plus additional
    /// advanced capabilities for complex deployments.
    Advanced = 2,
}

impl From<Profile> for librist_sys::rist_profile {
    fn from(p: Profile) -> Self {
        match p {
            Profile::Simple => librist_sys::rist_profile::RIST_PROFILE_SIMPLE,
            Profile::Main => librist_sys::rist_profile::RIST_PROFILE_MAIN,
            Profile::Advanced => librist_sys::rist_profile::RIST_PROFILE_ADVANCED,
        }
    }
}

impl From<librist_sys::rist_profile> for Profile {
    fn from(p: librist_sys::rist_profile) -> Self {
        match p {
            librist_sys::rist_profile::RIST_PROFILE_SIMPLE => Profile::Simple,
            librist_sys::rist_profile::RIST_PROFILE_MAIN => Profile::Main,
            librist_sys::rist_profile::RIST_PROFILE_ADVANCED => Profile::Advanced,
            _ => Profile::Main,
        }
    }
}

/// NACK (Negative Acknowledgement) type for packet recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum NackType {
    /// Range-based NACKs.
    ///
    /// NACKs specify a range of missing sequence numbers.
    /// More efficient for consecutive packet loss.
    #[default]
    Range = 0,

    /// Bitmask-based NACKs.
    ///
    /// NACKs use a bitmask to indicate missing packets.
    /// More efficient for scattered packet loss.
    Bitmask = 1,
}

impl From<NackType> for librist_sys::rist_nack_type {
    fn from(n: NackType) -> Self {
        match n {
            NackType::Range => librist_sys::rist_nack_type::RIST_NACK_RANGE,
            NackType::Bitmask => librist_sys::rist_nack_type::RIST_NACK_BITMASK,
        }
    }
}

/// Connection status for peer connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionStatus {
    /// Connection established successfully.
    Established,
    /// Connection timed out (no response from peer).
    TimedOut,
    /// A client connected (receiver mode).
    ClientConnected,
    /// A client connection timed out (receiver mode).
    ClientTimedOut,
}

impl From<librist_sys::rist_connection_status> for ConnectionStatus {
    fn from(s: librist_sys::rist_connection_status) -> Self {
        match s {
            librist_sys::rist_connection_status::RIST_CONNECTION_ESTABLISHED => {
                ConnectionStatus::Established
            }
            librist_sys::rist_connection_status::RIST_CONNECTION_TIMED_OUT => {
                ConnectionStatus::TimedOut
            }
            librist_sys::rist_connection_status::RIST_CLIENT_CONNECTED => {
                ConnectionStatus::ClientConnected
            }
            librist_sys::rist_connection_status::RIST_CLIENT_TIMED_OUT => {
                ConnectionStatus::ClientTimedOut
            }
            _ => ConnectionStatus::TimedOut,
        }
    }
}

impl ConnectionStatus {
    /// Returns true if this status indicates a successful connection.
    pub fn is_connected(&self) -> bool {
        matches!(
            self,
            ConnectionStatus::Established | ConnectionStatus::ClientConnected
        )
    }

    /// Returns true if this status indicates a timeout/disconnection.
    pub fn is_disconnected(&self) -> bool {
        matches!(
            self,
            ConnectionStatus::TimedOut | ConnectionStatus::ClientTimedOut
        )
    }
}

/// Recovery mode for packet loss recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum RecoveryMode {
    /// Recovery mode not configured.
    Unconfigured = 0,
    /// Recovery disabled - no retransmissions.
    Disabled = 1,
    /// Time-based recovery (recommended).
    ///
    /// Recovery buffer size is specified in milliseconds.
    #[default]
    Time = 2,
}

impl From<RecoveryMode> for librist_sys::rist_recovery_mode {
    fn from(m: RecoveryMode) -> Self {
        match m {
            RecoveryMode::Unconfigured => {
                librist_sys::rist_recovery_mode::RIST_RECOVERY_MODE_UNCONFIGURED
            }
            RecoveryMode::Disabled => librist_sys::rist_recovery_mode::RIST_RECOVERY_MODE_DISABLED,
            RecoveryMode::Time => librist_sys::rist_recovery_mode::RIST_RECOVERY_MODE_TIME,
        }
    }
}

impl From<librist_sys::rist_recovery_mode> for RecoveryMode {
    fn from(m: librist_sys::rist_recovery_mode) -> Self {
        match m {
            librist_sys::rist_recovery_mode::RIST_RECOVERY_MODE_UNCONFIGURED => {
                RecoveryMode::Unconfigured
            }
            librist_sys::rist_recovery_mode::RIST_RECOVERY_MODE_DISABLED => RecoveryMode::Disabled,
            librist_sys::rist_recovery_mode::RIST_RECOVERY_MODE_TIME => RecoveryMode::Time,
            _ => RecoveryMode::Time,
        }
    }
}

/// Congestion control mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum CongestionControl {
    /// Congestion control disabled.
    Off = 0,
    /// Normal congestion control (recommended).
    #[default]
    Normal = 1,
    /// Aggressive congestion control.
    ///
    /// Reduces bitrate more aggressively when congestion is detected.
    Aggressive = 2,
}

impl From<CongestionControl> for librist_sys::rist_congestion_control_mode {
    fn from(c: CongestionControl) -> Self {
        match c {
            CongestionControl::Off => {
                librist_sys::rist_congestion_control_mode::RIST_CONGESTION_CONTROL_MODE_OFF
            }
            CongestionControl::Normal => {
                librist_sys::rist_congestion_control_mode::RIST_CONGESTION_CONTROL_MODE_NORMAL
            }
            CongestionControl::Aggressive => {
                librist_sys::rist_congestion_control_mode::RIST_CONGESTION_CONTROL_MODE_AGGRESSIVE
            }
        }
    }
}

/// Timing mode for data delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum TimingMode {
    /// Use source timestamps.
    ///
    /// Data is delivered based on the original source timing.
    #[default]
    Source = 0,
    /// Use arrival timestamps.
    ///
    /// Data is delivered based on when it arrived at the receiver.
    Arrival = 1,
    /// Use RTC (Real-Time Clock).
    Rtc = 2,
}

impl From<TimingMode> for librist_sys::rist_timing_mode {
    fn from(t: TimingMode) -> Self {
        match t {
            TimingMode::Source => librist_sys::rist_timing_mode::RIST_TIMING_MODE_SOURCE,
            TimingMode::Arrival => librist_sys::rist_timing_mode::RIST_TIMING_MODE_ARRIVAL,
            TimingMode::Rtc => librist_sys::rist_timing_mode::RIST_TIMING_MODE_RTC,
        }
    }
}

bitflags::bitflags! {
    /// Flags for sender data blocks.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct SenderDataFlags: u32 {
        /// Use the provided sequence number.
        const USE_SEQ = 1;
        /// The payload needs to be freed by librist.
        const NEED_FREE = 2;
    }
}

bitflags::bitflags! {
    /// Flags for receiver data blocks.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ReceiverDataFlags: u32 {
        /// There was a discontinuity before this packet.
        const DISCONTINUITY = 1 << 0;
        /// This is the start of a flow buffer.
        const FLOW_BUFFER_START = 1 << 1;
        /// Buffer overflow occurred.
        const OVERFLOW = 1 << 2;
    }
}

/// Default virtual source port (1971).
pub const DEFAULT_VIRT_SRC_PORT: u16 = librist_sys::RIST_DEFAULT_VIRT_SRC_PORT as u16;

/// Default virtual destination port (1968).
pub const DEFAULT_VIRT_DST_PORT: u16 = librist_sys::RIST_DEFAULT_VIRT_DST_PORT as u16;

/// Default recovery max bitrate in kbps (100000 = 100 Mbps).
pub const DEFAULT_RECOVERY_MAX_BITRATE: u32 = librist_sys::RIST_DEFAULT_RECOVERY_MAXBITRATE;

/// Default minimum recovery buffer length in milliseconds.
pub const DEFAULT_RECOVERY_LENGTH_MIN: u32 = librist_sys::RIST_DEFAULT_RECOVERY_LENGTH_MIN;

/// Default maximum recovery buffer length in milliseconds.
pub const DEFAULT_RECOVERY_LENGTH_MAX: u32 = librist_sys::RIST_DEFAULT_RECOVERY_LENGTH_MAX;

/// Default reorder buffer in milliseconds.
pub const DEFAULT_RECOVERY_REORDER_BUFFER: u32 = librist_sys::RIST_DEFAULT_RECOVERY_REORDER_BUFFER;

/// Default minimum RTT in milliseconds.
pub const DEFAULT_RECOVERY_RTT_MIN: u32 = librist_sys::RIST_DEFAULT_RECOVERY_RTT_MIN;

/// Default maximum RTT in milliseconds.
pub const DEFAULT_RECOVERY_RTT_MAX: u32 = librist_sys::RIST_DEFAULT_RECOVERY_RTT_MAX;

/// Default minimum retries.
pub const DEFAULT_MIN_RETRIES: u32 = librist_sys::RIST_DEFAULT_MIN_RETRIES;

/// Default maximum retries.
pub const DEFAULT_MAX_RETRIES: u32 = librist_sys::RIST_DEFAULT_MAX_RETRIES;

/// Default session timeout in milliseconds.
pub const DEFAULT_SESSION_TIMEOUT: u32 = librist_sys::RIST_DEFAULT_SESSION_TIMEOUT;

/// Default keepalive interval in milliseconds.
pub const DEFAULT_KEEPALIVE_INTERVAL: u32 = librist_sys::RIST_DEFAULT_KEEPALIVE_INTERVAL;

// ============================================================================
// PeerInfo - Information about a peer for authentication callbacks
// ============================================================================

/// Information about a connecting peer provided to authentication callbacks.
///
/// This struct contains metadata extracted from the librist `rist_peer` structure,
/// providing safe access to peer information without exposing the raw C pointer.
///
/// # Fields
///
/// - `id` - A locally-generated auto-incrementing identifier (not cryptographically secure)
/// - `cname` - The RTCP Canonical Name (SDES CNAME), a user-configurable identifier
///
/// # Authentication Notes
///
/// The `id` field is assigned sequentially by librist when peers connect and is primarily
/// useful for tracking peers within a session. It should **not** be used for authentication
/// decisions as it's just an auto-incrementing counter.
///
/// The `cname` field comes from the RTCP SDES (Source Description) and is configurable
/// by the connecting peer via the URL `cname=` parameter. This can be useful for
/// identifying peers by a human-readable name, but note that it can be freely set
/// by the sender and should not be trusted for security purposes.
///
/// For actual authentication, consider:
/// - IP-based filtering using the connection IP provided to the callback
/// - SRP authentication via [`crate::SrpCredentials`] (requires `srp` feature)
/// - Pre-shared encryption keys via URL `secret=` parameter
///
/// # Example
///
/// ```no_run
/// use librist::{RistReceiver, Profile, PeerInfo};
///
/// let receiver = RistReceiver::builder()
///     .profile(Profile::Main)
///     .on_auth_connect(|conn_ip, conn_port, local_ip, local_port, peer| {
///         println!("Connection from {}:{}", conn_ip, conn_port);
///         println!("Peer ID: {}", peer.id);
///         if let Some(cname) = &peer.cname {
///             println!("Peer CNAME: {}", cname);
///         }
///         // Accept connections from localhost only
///         conn_ip == "127.0.0.1"
///     })
///     .build()?;
/// # Ok::<(), librist::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PeerInfo {
    /// Locally-generated peer identifier (auto-incrementing counter).
    ///
    /// This ID is assigned by librist when a peer connects and is unique within
    /// the context's lifetime. It's useful for correlating connect/disconnect
    /// events but should not be used for authentication.
    pub id: u32,

    /// RTCP Canonical Name (SDES CNAME) of the peer, if available.
    ///
    /// This is a user-configurable string set via the `cname=` URL parameter
    /// on the sender side. It can be used for peer identification but should
    /// not be trusted for security purposes as it can be freely set.
    pub cname: Option<String>,
}

impl PeerInfo {
    /// Creates a new `PeerInfo` with the given ID and no CNAME.
    pub fn new(id: u32) -> Self {
        Self { id, cname: None }
    }

    /// Creates a new `PeerInfo` with the given ID and CNAME.
    pub fn with_cname(id: u32, cname: impl Into<String>) -> Self {
        Self {
            id,
            cname: Some(cname.into()),
        }
    }

    /// Extracts peer information from a raw librist peer pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `peer` is either null or points to a valid
    /// `rist_peer` structure that remains valid for the duration of this call.
    pub(crate) unsafe fn from_raw(peer: *const librist_sys::rist_peer) -> Self {
        if peer.is_null() {
            return Self { id: 0, cname: None };
        }

        let id = unsafe { librist_sys::rist_peer_get_id(peer) };

        let cname = unsafe {
            let mut cname_ptr: *const std::os::raw::c_char = std::ptr::null();
            let len = librist_sys::rist_peer_get_cname(peer, &mut cname_ptr);
            if len > 0 && !cname_ptr.is_null() {
                std::ffi::CStr::from_ptr(cname_ptr)
                    .to_str()
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_owned())
            } else {
                None
            }
        };

        Self { id, cname }
    }
}

impl Default for PeerInfo {
    fn default() -> Self {
        Self { id: 0, cname: None }
    }
}

impl std::fmt::Display for PeerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.cname {
            Some(cname) => write!(f, "Peer(id={}, cname={})", self.id, cname),
            None => write!(f, "Peer(id={})", self.id),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_default() {
        assert_eq!(Profile::default(), Profile::Main);
    }

    #[test]
    fn test_connection_status_helpers() {
        assert!(ConnectionStatus::Established.is_connected());
        assert!(ConnectionStatus::ClientConnected.is_connected());
        assert!(!ConnectionStatus::TimedOut.is_connected());

        assert!(ConnectionStatus::TimedOut.is_disconnected());
        assert!(ConnectionStatus::ClientTimedOut.is_disconnected());
        assert!(!ConnectionStatus::Established.is_disconnected());
    }

    #[test]
    fn test_peer_info_new() {
        let peer = PeerInfo::new(42);
        assert_eq!(peer.id, 42);
        assert_eq!(peer.cname, None);
    }

    #[test]
    fn test_peer_info_with_cname() {
        let peer = PeerInfo::with_cname(123, "my-sender");
        assert_eq!(peer.id, 123);
        assert_eq!(peer.cname, Some("my-sender".to_string()));
    }

    #[test]
    fn test_peer_info_default() {
        let peer = PeerInfo::default();
        assert_eq!(peer.id, 0);
        assert_eq!(peer.cname, None);
    }

    #[test]
    fn test_peer_info_display() {
        let peer1 = PeerInfo::new(42);
        assert_eq!(format!("{}", peer1), "Peer(id=42)");

        let peer2 = PeerInfo::with_cname(123, "test");
        assert_eq!(format!("{}", peer2), "Peer(id=123, cname=test)");
    }

    #[test]
    fn test_peer_info_from_null() {
        let peer = unsafe { PeerInfo::from_raw(std::ptr::null()) };
        assert_eq!(peer.id, 0);
        assert_eq!(peer.cname, None);
    }
}
