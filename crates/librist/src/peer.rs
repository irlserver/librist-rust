//! Peer configuration and management for RIST connections.

use crate::error::{check_result, Error, Result};
use crate::types::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr::{self, NonNull};

/// A handle to a RIST peer connection.
///
/// Peers represent remote endpoints in a RIST session. For senders,
/// peers are the destinations to send data to. For receivers, peers
/// are the sources to receive data from.
///
/// Peers are automatically destroyed when the parent context is dropped.
#[derive(Clone)]
pub struct PeerHandle {
    peer: NonNull<librist_sys::rist_peer>,
    ctx: NonNull<librist_sys::rist_ctx>,
}

impl PeerHandle {
    /// Creates a new peer handle from raw pointers.
    ///
    /// # Safety
    ///
    /// Both pointers must be valid and the peer must belong to the context.
    pub(crate) fn new(
        peer: *mut librist_sys::rist_peer,
        ctx: NonNull<librist_sys::rist_ctx>,
    ) -> Self {
        Self {
            peer: NonNull::new(peer).expect("peer should not be null"),
            ctx,
        }
    }

    /// Gets the unique peer ID.
    ///
    /// The peer ID is assigned by librist and can be used to identify
    /// peers in callbacks.
    pub fn id(&self) -> u32 {
        unsafe { librist_sys::rist_peer_get_id(self.peer.as_ptr()) }
    }

    /// Sets the peer weight for bonding.
    ///
    /// The weight determines how traffic is distributed among peers
    /// when bonding is enabled.
    ///
    /// - `weight = 0`: Duplicate mode - data is sent to all peers
    /// - `weight > 0`: Load balancing - higher weights receive more traffic
    ///
    /// # Example
    ///
    /// ```no_run
    /// use librist::PeerHandle;
    ///
    /// fn configure_bonding(primary: &PeerHandle, backup: &PeerHandle) {
    ///     // Primary link gets 3x the traffic
    ///     primary.set_weight(3).unwrap();
    ///     backup.set_weight(1).unwrap();
    /// }
    /// ```
    pub fn set_weight(&self, weight: u32) -> Result<()> {
        let ret = unsafe {
            librist_sys::rist_peer_weight_set(self.ctx.as_ptr(), self.peer.as_ptr(), weight)
        };
        check_result(ret)
    }

    /// Gets the CNAME (canonical name) for this peer.
    ///
    /// The CNAME is used in RTCP for identifying participants.
    pub fn cname(&self) -> Option<String> {
        let mut cname_ptr: *const c_char = ptr::null();
        let ret = unsafe { librist_sys::rist_peer_get_cname(self.peer.as_ptr(), &mut cname_ptr) };
        if ret == 0 || cname_ptr.is_null() {
            None
        } else {
            unsafe { CStr::from_ptr(cname_ptr) }
                .to_str()
                .ok()
                .map(String::from)
        }
    }

    /// Returns the raw peer pointer.
    pub(crate) fn as_raw(&self) -> *mut librist_sys::rist_peer {
        self.peer.as_ptr()
    }
}

// Safety: PeerHandle only contains raw pointers managed by librist
// and doesn't perform mutable operations without synchronization
unsafe impl Send for PeerHandle {}
unsafe impl Sync for PeerHandle {}

impl std::fmt::Debug for PeerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeerHandle")
            .field("id", &self.id())
            .field("cname", &self.cname())
            .finish()
    }
}

/// Configuration for a RIST peer.
///
/// Use [`PeerConfig::default()`] for sensible defaults, or create
/// a custom configuration with the builder methods.
///
/// # Example
///
/// ```
/// use librist::{PeerConfig, RecoveryMode};
///
/// let config = PeerConfig::default()
///     .with_address("192.168.1.100")
///     .with_port(5000)
///     .with_recovery_mode(RecoveryMode::Time)
///     .with_recovery_length(1000, 2000);
/// ```
#[derive(Debug, Clone)]
pub struct PeerConfig {
    pub(crate) raw: librist_sys::rist_peer_config,
}

impl Default for PeerConfig {
    fn default() -> Self {
        let mut raw: librist_sys::rist_peer_config = unsafe { std::mem::zeroed() };
        unsafe {
            librist_sys::rist_peer_config_defaults_set(&mut raw);
        }
        Self { raw }
    }
}

impl PeerConfig {
    /// Creates a new peer configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses a RIST URL into a peer configuration.
    ///
    /// # URL Format
    ///
    /// ```text
    /// rist://[user:pass@]host:port[?options]
    /// ```
    ///
    /// For listener mode, prefix with `@`:
    /// ```text
    /// rist://@:5000
    /// ```
    ///
    /// # Options
    ///
    /// - `bandwidth=<kbps>` - Maximum recovery bandwidth
    /// - `buffer=<ms>` or `buffer-min=<ms>&buffer-max=<ms>` - Buffer size
    /// - `secret=<key>` - Encryption secret
    /// - `aes-type=<128|256>` - AES key size
    /// - `weight=<n>` - Bonding weight
    /// - `cname=<name>` - RTCP canonical name
    /// - `rtt-min=<ms>&rtt-max=<ms>` - RTT bounds
    ///
    /// # Example
    ///
    /// ```
    /// use librist::PeerConfig;
    ///
    /// // Simple receiver listener
    /// let config = PeerConfig::from_url("rist://@:5000").unwrap();
    ///
    /// // Sender with options
    /// let config = PeerConfig::from_url(
    ///     "rist://server.example.com:5000?bandwidth=50000&buffer=1000"
    /// ).unwrap();
    /// ```
    pub fn from_url(url: &str) -> Result<Self> {
        let url_cstr =
            CString::new(url).map_err(|_| Error::InvalidUrl(url.to_string()))?;

        let mut config: *mut librist_sys::rist_peer_config = ptr::null_mut();
        let ret = unsafe { librist_sys::rist_parse_address2(url_cstr.as_ptr(), &mut config) };

        if ret != 0 || config.is_null() {
            return Err(Error::UrlParseFailed(url.to_string()));
        }

        // Copy the config and free the original
        let result = Self {
            raw: unsafe { *config },
        };
        unsafe {
            librist_sys::rist_peer_config_free2(&mut config);
        }

        Ok(result)
    }

    /// Sets the address/hostname.
    pub fn with_address(mut self, address: &str) -> Self {
        let bytes = address.as_bytes();
        let len = bytes.len().min(librist_sys::RIST_MAX_STRING_LONG as usize - 1);
        self.raw.address[..len].copy_from_slice(unsafe {
            std::slice::from_raw_parts(bytes.as_ptr() as *const c_char, len)
        });
        self.raw.address[len] = 0;
        self
    }

    /// Sets the port number.
    pub fn with_port(mut self, port: u16) -> Self {
        self.raw.physical_port = port;
        self
    }

    /// Sets the virtual destination port.
    pub fn with_virtual_dst_port(mut self, port: u16) -> Self {
        self.raw.virt_dst_port = port;
        self
    }

    /// Sets whether this peer should initiate the connection.
    ///
    /// - `true`: This side initiates (sender mode or pull receiver)
    /// - `false`: This side listens (receiver listener mode)
    pub fn with_initiate_connection(mut self, initiate: bool) -> Self {
        self.raw.initiate_conn = if initiate { 1 } else { 0 };
        self
    }

    /// Sets the recovery mode.
    pub fn with_recovery_mode(mut self, mode: RecoveryMode) -> Self {
        self.raw.recovery_mode = mode.into();
        self
    }

    /// Sets the recovery buffer length range in milliseconds.
    pub fn with_recovery_length(mut self, min_ms: u32, max_ms: u32) -> Self {
        self.raw.recovery_length_min = min_ms;
        self.raw.recovery_length_max = max_ms;
        self
    }

    /// Sets the maximum recovery bandwidth in kbps.
    pub fn with_recovery_max_bitrate(mut self, kbps: u32) -> Self {
        self.raw.recovery_maxbitrate = kbps;
        self
    }

    /// Sets the return path bandwidth in kbps (for receiver to sender).
    pub fn with_recovery_return_bitrate(mut self, kbps: u32) -> Self {
        self.raw.recovery_maxbitrate_return = kbps;
        self
    }

    /// Sets the reorder buffer in milliseconds.
    pub fn with_reorder_buffer(mut self, ms: u32) -> Self {
        self.raw.recovery_reorder_buffer = ms;
        self
    }

    /// Sets the RTT (round-trip time) bounds in milliseconds.
    pub fn with_rtt_range(mut self, min_ms: u32, max_ms: u32) -> Self {
        self.raw.recovery_rtt_min = min_ms;
        self.raw.recovery_rtt_max = max_ms;
        self
    }

    /// Sets the bonding weight.
    ///
    /// - `0`: Duplicate mode (send to all peers)
    /// - `>0`: Load balancing weight
    pub fn with_weight(mut self, weight: u32) -> Self {
        self.raw.weight = weight;
        self
    }

    /// Sets the encryption secret.
    ///
    /// The secret is used for AES encryption when `key_size` is set.
    pub fn with_secret(mut self, secret: &str) -> Self {
        let bytes = secret.as_bytes();
        let len = bytes.len().min(librist_sys::RIST_MAX_STRING_SHORT as usize - 1);
        self.raw.secret[..len].copy_from_slice(unsafe {
            std::slice::from_raw_parts(bytes.as_ptr() as *const c_char, len)
        });
        self.raw.secret[len] = 0;
        self
    }

    /// Sets the AES key size (0, 128, or 256).
    ///
    /// - `0`: No encryption
    /// - `128`: AES-128 encryption
    /// - `256`: AES-256 encryption
    pub fn with_key_size(mut self, bits: i32) -> Self {
        self.raw.key_size = bits;
        self
    }

    /// Sets the CNAME (canonical name) for RTCP.
    pub fn with_cname(mut self, cname: &str) -> Self {
        let bytes = cname.as_bytes();
        let len = bytes.len().min(librist_sys::RIST_MAX_STRING_SHORT as usize - 1);
        self.raw.cname[..len].copy_from_slice(unsafe {
            std::slice::from_raw_parts(bytes.as_ptr() as *const c_char, len)
        });
        self.raw.cname[len] = 0;
        self
    }

    /// Sets the congestion control mode.
    pub fn with_congestion_control(mut self, mode: CongestionControl) -> Self {
        self.raw.congestion_control_mode = mode.into();
        self
    }

    /// Sets the retry count range.
    pub fn with_retries(mut self, min: u32, max: u32) -> Self {
        self.raw.min_retries = min;
        self.raw.max_retries = max;
        self
    }

    /// Sets the session timeout in milliseconds.
    pub fn with_session_timeout(mut self, ms: u32) -> Self {
        self.raw.session_timeout = ms;
        self
    }

    /// Sets the keepalive interval in milliseconds.
    pub fn with_keepalive_interval(mut self, ms: u32) -> Self {
        self.raw.keepalive_interval = ms;
        self
    }

    /// Sets the timing mode.
    pub fn with_timing_mode(mut self, mode: TimingMode) -> Self {
        self.raw.timing_mode = mode.into();
        self
    }

    /// Sets the multicast interface.
    pub fn with_multicast_interface(mut self, iface: &str) -> Self {
        let bytes = iface.as_bytes();
        let len = bytes.len().min(librist_sys::RIST_MAX_STRING_SHORT as usize - 1);
        self.raw.miface[..len].copy_from_slice(unsafe {
            std::slice::from_raw_parts(bytes.as_ptr() as *const c_char, len)
        });
        self.raw.miface[len] = 0;
        self
    }

    /// Returns a reference to the raw configuration.
    pub(crate) fn as_raw(&self) -> &librist_sys::rist_peer_config {
        &self.raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_config_defaults() {
        let config = PeerConfig::default();
        assert_eq!(
            config.raw.recovery_mode,
            librist_sys::rist_recovery_mode::RIST_RECOVERY_MODE_TIME
        );
    }

    #[test]
    fn test_peer_config_builder() {
        let config = PeerConfig::default()
            .with_port(5000)
            .with_weight(2)
            .with_key_size(256);

        assert_eq!(config.raw.physical_port, 5000);
        assert_eq!(config.raw.weight, 2);
        assert_eq!(config.raw.key_size, 256);
    }
}
