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
}
