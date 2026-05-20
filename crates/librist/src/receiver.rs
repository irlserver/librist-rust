//! RIST receiver context and builder.

use crate::callbacks::{
    AuthConnectCallback, AuthDisconnectCallback, ConnectionCallback, DataCallback, LogCallback,
    OobCallback, ReceiverCallbacks, StatsCallback, auth_connect_trampoline,
    auth_disconnect_trampoline, receiver_connection_trampoline, receiver_data_trampoline,
    receiver_oob_trampoline, receiver_stats_trampoline,
};
use crate::data::DataBlock;
use crate::error::{Error, Result, check_result};
use crate::logging::{LogLevel, LoggingSettings};
use crate::oob::OobBlock;
use crate::peer::{PeerConfig, PeerHandle};
use crate::stats::ReceiverStats;
use crate::types::{ConnectionStatus, NackType, PeerInfo, Profile};
use parking_lot::Mutex;
use std::os::raw::{c_int, c_void};
use std::ptr::{self, NonNull};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// ============================================================================
// RistReceiver
// ============================================================================

/// A RIST receiver context for receiving data.
///
/// The receiver manages connections from one or more sender peers and
/// handles packet recovery and reordering.
///
/// # Thread Safety
///
/// `RistReceiver` is `Send + Sync` and can be shared between threads
/// using `Arc<RistReceiver>`.
///
/// # Example
///
/// ```no_run
/// use librist::{RistReceiver, Profile};
///
/// let receiver = RistReceiver::builder()
///     .profile(Profile::Main)
///     .build()?;
///
/// // Listen on port 5000
/// receiver.add_peer("rist://@:5000")?;
///
/// // Start the receiver
/// receiver.start()?;
///
/// // Receive data
/// loop {
///     match receiver.recv(1000) {
///         Ok(block) => println!("Received {} bytes", block.payload().len()),
///         Err(librist::Error::Timeout) => continue,
///         Err(e) => return Err(e),
///     }
/// }
/// # Ok::<(), librist::Error>(())
/// ```
pub struct RistReceiver {
    ctx: NonNull<librist_sys::rist_ctx>,
    profile: Profile,
    started: AtomicBool,
    oob_enabled: AtomicBool,
    peers: Mutex<Vec<PeerHandle>>,
    pub(crate) callbacks: Arc<Mutex<ReceiverCallbacks>>,
    /// Number of times we've called Arc::into_raw for callbacks (need to reclaim in Drop)
    callback_arc_count: AtomicU32,
    #[allow(dead_code)]
    logging: Option<Box<LoggingSettings>>,
}

impl RistReceiver {
    /// Creates a new builder for configuring a receiver.
    pub fn builder() -> ReceiverBuilder {
        ReceiverBuilder::default()
    }

    /// Returns the RIST profile being used.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Adds a peer using a RIST URL.
    ///
    /// For listener mode, prefix with `@`:
    /// `rist://@:5000`
    pub fn add_peer(&self, url: &str) -> Result<PeerHandle> {
        let config = PeerConfig::from_url(url)?;
        self.add_peer_with_config(&config)
    }

    /// Adds a peer with explicit configuration.
    pub fn add_peer_with_config(&self, config: &PeerConfig) -> Result<PeerHandle> {
        let mut peer: *mut librist_sys::rist_peer = ptr::null_mut();
        let ret =
            unsafe { librist_sys::rist_peer_create(self.ctx.as_ptr(), &mut peer, config.as_raw()) };

        if ret != 0 || peer.is_null() {
            return Err(Error::PeerCreationFailed);
        }

        let handle = PeerHandle::new(peer, self.ctx);
        self.peers.lock().push(handle.clone());
        Ok(handle)
    }

    /// Starts the receiver.
    pub fn start(&self) -> Result<()> {
        if self.started.swap(true, Ordering::AcqRel) {
            return Err(Error::AlreadyStarted);
        }
        let ret = unsafe { librist_sys::rist_start(self.ctx.as_ptr()) };
        if ret != 0 {
            self.started.store(false, Ordering::Release);
            check_result(ret)
        } else {
            Ok(())
        }
    }

    /// Returns whether the receiver has been started.
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }

    /// Receives data from the RIST connection (blocking with timeout).
    ///
    /// # Arguments
    ///
    /// * `timeout_ms` - Timeout in milliseconds (-1 for infinite, 0 for non-blocking)
    pub fn recv(&self, timeout_ms: i32) -> Result<DataBlock> {
        if !self.started.load(Ordering::Acquire) {
            return Err(Error::NotStarted);
        }

        let mut block: *mut librist_sys::rist_data_block = ptr::null_mut();
        let ret = unsafe {
            librist_sys::rist_receiver_data_read2(self.ctx.as_ptr(), &mut block, timeout_ms)
        };

        if ret < 0 {
            check_result(ret)?;
        }
        if ret == 0 || block.is_null() {
            return Err(Error::Timeout);
        }

        Ok(DataBlock::from_received(block))
    }

    /// Attempts to receive data without blocking.
    pub fn try_recv(&self) -> Result<Option<DataBlock>> {
        match self.recv(0) {
            Ok(block) => Ok(Some(block)),
            Err(Error::Timeout) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Sends out-of-band data to all peers.
    ///
    /// OOB data is transmitted via the RTCP channel with no buffering or
    /// retransmission. This is suitable for low-latency signaling.
    ///
    /// # Requirements
    ///
    /// - OOB must be enabled via `on_oob()` in the builder
    /// - Only works with Main or Advanced profile
    /// - Must be called after `start()`
    ///
    /// # Returns
    ///
    /// The number of bytes written.
    pub fn send_oob(&self, data: &[u8]) -> Result<usize> {
        self.send_oob_block(&OobBlock::new(data.to_vec()))
    }

    /// Sends out-of-band data to a specific peer.
    ///
    /// # Returns
    ///
    /// The number of bytes written.
    pub fn send_oob_to_peer(&self, data: &[u8], peer: &PeerHandle) -> Result<usize> {
        self.send_oob_block(&OobBlock::new(data.to_vec()).with_peer(peer))
    }

    /// Sends an OOB block with optional peer targeting.
    ///
    /// If the block has a peer ID set (via [`OobBlock::with_peer`]), the data
    /// will be sent only to that peer. Otherwise, it's sent to all peers.
    pub fn send_oob_block(&self, block: &OobBlock) -> Result<usize> {
        if !self.started.load(Ordering::Acquire) {
            return Err(Error::NotStarted);
        }
        if !self.oob_enabled.load(Ordering::Acquire) {
            return Err(Error::OobNotEnabled);
        }
        if self.profile == Profile::Simple {
            return Err(Error::ProfileNotSupported);
        }

        // Look up peer pointer if peer_id is specified
        let peer_ptr = if let Some(peer_id) = block.peer_id() {
            let peers = self.peers.lock();
            peers
                .iter()
                .find(|p| p.id() == peer_id)
                .map(|p| p.as_raw())
                .ok_or(Error::PeerNotFound)?
        } else {
            ptr::null_mut()
        };

        let oob_block = librist_sys::rist_oob_block {
            peer: peer_ptr,
            payload: block.payload().as_ptr() as *const c_void,
            payload_len: block.payload().len(),
            ts_ntp: 0,
        };

        let ret = unsafe { librist_sys::rist_oob_write(self.ctx.as_ptr(), &oob_block) };
        if ret < 0 {
            check_result(ret)?;
        }
        Ok(ret as usize)
    }

    /// Sets the NACK type.
    pub fn set_nack_type(&self, nack_type: NackType) -> Result<()> {
        let ret = unsafe {
            librist_sys::rist_receiver_nack_type_set(self.ctx.as_ptr(), nack_type.into())
        };
        check_result(ret)
    }

    /// Sets the output FIFO buffer size.
    pub fn set_output_fifo_size(&self, size: u32) -> Result<()> {
        let ret =
            unsafe { librist_sys::rist_receiver_set_output_fifo_size(self.ctx.as_ptr(), size) };
        check_result(ret)
    }

    /// Sets the recovery buffer RTT multiplier.
    ///
    /// Controls how aggressively the auto-scaling buffer grows relative to the
    /// measured RTT (`buffer = multiplier * smoothed_rtt + reorder_buffer`).
    /// Defaults to 7 per the RIST spec; lower values (2-3) suit low-latency LAN
    /// scenarios. Only effective when auto-scaling is enabled, i.e. the buffer's
    /// min and max recovery lengths differ. May be called before or after start.
    ///
    /// `multiplier` must be >= 1.
    pub fn set_recovery_rtt_multiplier(&self, multiplier: i32) -> Result<()> {
        let ret = unsafe {
            librist_sys::rist_recovery_rtt_multiplier_set(self.ctx.as_ptr(), multiplier)
        };
        check_result(ret)
    }

    /// Returns the number of connected peers.
    pub fn peer_count(&self) -> usize {
        self.peers.lock().len()
    }

    /// Returns raw context pointer (for advanced use).
    ///
    /// # Safety
    ///
    /// This is exposed for advanced users who need direct access to the
    /// underlying librist context. Use with caution.
    pub fn raw_ctx(&self) -> *mut librist_sys::rist_ctx {
        self.ctx.as_ptr()
    }

    fn setup_data_callback(&self) -> Result<()> {
        let callbacks = Arc::clone(&self.callbacks);
        let ctx_ptr = Arc::into_raw(callbacks) as *mut c_void;
        self.callback_arc_count.fetch_add(1, Ordering::SeqCst);

        let ret = unsafe {
            librist_sys::rist_receiver_data_callback_set2(
                self.ctx.as_ptr(),
                Some(receiver_data_trampoline),
                ctx_ptr,
            )
        };
        check_result(ret)
    }

    fn setup_stats_callback(&self, interval_ms: u32) -> Result<()> {
        let callbacks = Arc::clone(&self.callbacks);
        let ctx_ptr = Arc::into_raw(callbacks) as *mut c_void;
        self.callback_arc_count.fetch_add(1, Ordering::SeqCst);

        let ret = unsafe {
            librist_sys::rist_stats_callback_set(
                self.ctx.as_ptr(),
                interval_ms as c_int,
                Some(receiver_stats_trampoline),
                ctx_ptr,
            )
        };
        check_result(ret)
    }

    fn setup_connection_callback(&self) -> Result<()> {
        let callbacks = Arc::clone(&self.callbacks);
        let ctx_ptr = Arc::into_raw(callbacks) as *mut c_void;
        self.callback_arc_count.fetch_add(1, Ordering::SeqCst);

        let ret = unsafe {
            librist_sys::rist_connection_status_callback_set(
                self.ctx.as_ptr(),
                Some(receiver_connection_trampoline),
                ctx_ptr,
            )
        };
        check_result(ret)
    }

    fn setup_auth_callback(&self) -> Result<()> {
        let callbacks = Arc::clone(&self.callbacks);
        let ctx_ptr = Arc::into_raw(callbacks) as *mut c_void;
        self.callback_arc_count.fetch_add(1, Ordering::SeqCst);

        let ret = unsafe {
            librist_sys::rist_auth_handler_set(
                self.ctx.as_ptr(),
                Some(auth_connect_trampoline),
                Some(auth_disconnect_trampoline),
                ctx_ptr,
            )
        };
        check_result(ret)
    }

    fn setup_oob_callback(&self) -> Result<()> {
        if self.profile == Profile::Simple {
            return Err(Error::ProfileNotSupported);
        }

        let callbacks = Arc::clone(&self.callbacks);
        let ctx_ptr = Arc::into_raw(callbacks) as *mut c_void;
        self.callback_arc_count.fetch_add(1, Ordering::SeqCst);

        let ret = unsafe {
            librist_sys::rist_oob_callback_set(
                self.ctx.as_ptr(),
                Some(receiver_oob_trampoline),
                ctx_ptr,
            )
        };
        if ret == 0 {
            self.oob_enabled.store(true, Ordering::Release);
        }
        check_result(ret)
    }
}

impl Drop for RistReceiver {
    fn drop(&mut self) {
        self.peers.lock().clear();
        // First destroy the context - this ensures no more callbacks will be called
        unsafe {
            librist_sys::rist_destroy(self.ctx.as_ptr());
        }
        // Now reclaim the Arc references we leaked via Arc::into_raw
        let count = self.callback_arc_count.load(Ordering::SeqCst);
        for _ in 0..count {
            // SAFETY: We called Arc::into_raw this many times, so we need to reclaim them
            let _ = unsafe { Arc::from_raw(Arc::as_ptr(&self.callbacks)) };
        }
    }
}

unsafe impl Send for RistReceiver {}
unsafe impl Sync for RistReceiver {}

// ============================================================================
// ReceiverBuilder
// ============================================================================

/// Builder for configuring a RIST receiver.
#[derive(Default)]
pub struct ReceiverBuilder {
    profile: Profile,
    log_level: LogLevel,
    log_callback: Option<LogCallback>,
    #[cfg(feature = "tracing")]
    use_tracing: bool,
    nack_type: NackType,
    fifo_size: Option<u32>,
    stats_interval_ms: Option<u32>,
    stats_callback: Option<StatsCallback<ReceiverStats>>,
    connection_callback: Option<ConnectionCallback>,
    data_callback: Option<DataCallback>,
    auth_connect_callback: Option<AuthConnectCallback>,
    auth_disconnect_callback: Option<AuthDisconnectCallback>,
    oob_callback: Option<OobCallback>,
}

impl ReceiverBuilder {
    /// Sets the RIST profile.
    pub fn profile(mut self, profile: Profile) -> Self {
        self.profile = profile;
        self
    }

    /// Sets the log level.
    pub fn log_level(mut self, level: LogLevel) -> Self {
        self.log_level = level;
        self
    }

    /// Sets a callback for log messages.
    pub fn on_log<F>(mut self, callback: F) -> Self
    where
        F: Fn(LogLevel, &str) + Send + Sync + 'static,
    {
        self.log_callback = Some(Box::new(callback));
        self
    }

    /// Enables logging via the `tracing` crate instead of `log`.
    ///
    /// This is only available when the `tracing` feature is enabled.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use librist::{RistReceiver, Profile, LogLevel};
    ///
    /// let receiver = RistReceiver::builder()
    ///     .profile(Profile::Main)
    ///     .log_level(LogLevel::Debug)
    ///     .use_tracing()
    ///     .build()?;
    /// # Ok::<(), librist::Error>(())
    /// ```
    #[cfg(feature = "tracing")]
    pub fn use_tracing(mut self) -> Self {
        self.use_tracing = true;
        self
    }

    /// Sets the NACK type.
    pub fn nack_type(mut self, nack_type: NackType) -> Self {
        self.nack_type = nack_type;
        self
    }

    /// Sets the output FIFO buffer size.
    pub fn fifo_size(mut self, size: u32) -> Self {
        self.fifo_size = Some(size);
        self
    }

    /// Sets the stats callback with reporting interval.
    pub fn on_stats<F>(mut self, interval_ms: u32, callback: F) -> Self
    where
        F: Fn(&ReceiverStats) + Send + Sync + 'static,
    {
        self.stats_interval_ms = Some(interval_ms);
        self.stats_callback = Some(Box::new(callback));
        self
    }

    /// Sets a callback for connection status changes.
    pub fn on_connection<F>(mut self, callback: F) -> Self
    where
        F: Fn(u32, ConnectionStatus) + Send + Sync + 'static,
    {
        self.connection_callback = Some(Box::new(callback));
        self
    }

    /// Sets a callback for received data.
    ///
    /// When set, data is delivered via callback instead of polling with `recv()`.
    pub fn on_data<F>(mut self, callback: F) -> Self
    where
        F: Fn(DataBlock) + Send + Sync + 'static,
    {
        self.data_callback = Some(Box::new(callback));
        self
    }

    /// Sets a callback for authenticating incoming peer connections.
    ///
    /// The callback receives connection information and returns `true` to accept
    /// the connection or `false` to reject it.
    ///
    /// # Arguments
    ///
    /// The callback receives:
    /// - `conn_ip` - The connecting peer's IP address
    /// - `conn_port` - The connecting peer's port
    /// - `local_ip` - The local IP address
    /// - `local_port` - The local port
    /// - `peer` - A [`PeerInfo`] struct containing the peer's ID and CNAME
    ///
    /// # Peer Information
    ///
    /// The [`PeerInfo`] struct provides access to:
    /// - `id` - A locally-assigned auto-incrementing identifier (not for auth)
    /// - `cname` - The RTCP Canonical Name (SDES CNAME), a user-configurable identifier
    ///
    /// Note: The peer ID is just an auto-incrementing counter and should not be used
    /// for authentication decisions. The CNAME is user-configurable via the URL
    /// `cname=` parameter and can be used for peer identification.
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
    ///         println!("Peer: {}", peer);
    ///         if let Some(cname) = &peer.cname {
    ///             println!("CNAME: {}", cname);
    ///         }
    ///         // Accept all connections
    ///         true
    ///     })
    ///     .build()?;
    /// # Ok::<(), librist::Error>(())
    /// ```
    pub fn on_auth_connect<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str, u16, &str, u16, &PeerInfo) -> bool + Send + Sync + 'static,
    {
        self.auth_connect_callback = Some(Box::new(callback));
        self
    }

    /// Sets a callback for peer disconnection events.
    ///
    /// Called when a peer disconnects from the receiver.
    ///
    /// # Arguments
    ///
    /// The callback receives:
    /// - `peer` - A [`PeerInfo`] struct containing the peer's ID and CNAME
    ///
    /// # Example
    ///
    /// ```no_run
    /// use librist::{RistReceiver, Profile, PeerInfo};
    ///
    /// let receiver = RistReceiver::builder()
    ///     .profile(Profile::Main)
    ///     .on_auth_disconnect(|peer| {
    ///         println!("Peer disconnected: {}", peer);
    ///     })
    ///     .build()?;
    /// # Ok::<(), librist::Error>(())
    /// ```
    pub fn on_auth_disconnect<F>(mut self, callback: F) -> Self
    where
        F: Fn(&PeerInfo) + Send + Sync + 'static,
    {
        self.auth_disconnect_callback = Some(Box::new(callback));
        self
    }

    /// Sets a callback for receiving out-of-band data.
    ///
    /// OOB data is transmitted via the RTCP channel with no buffering or
    /// retransmission. Only available with Main or Advanced profile.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use librist::{RistReceiver, Profile};
    ///
    /// let receiver = RistReceiver::builder()
    ///     .profile(Profile::Main)
    ///     .on_oob(|block| {
    ///         println!("OOB data: {} bytes", block.payload().len());
    ///     })
    ///     .build()?;
    /// # Ok::<(), librist::Error>(())
    /// ```
    pub fn on_oob<F>(mut self, callback: F) -> Self
    where
        F: Fn(OobBlock) + Send + Sync + 'static,
    {
        self.oob_callback = Some(Box::new(callback));
        self
    }

    /// Builds the receiver.
    pub fn build(self) -> Result<RistReceiver> {
        // Create logging settings
        #[cfg(feature = "tracing")]
        let logging = if let Some(callback) = self.log_callback {
            Some(Box::new(LoggingSettings::with_callback(
                self.log_level,
                callback,
            )?))
        } else if self.use_tracing && self.log_level != LogLevel::Disable {
            Some(Box::new(LoggingSettings::with_tracing(self.log_level)?))
        } else if self.log_level != LogLevel::Disable {
            Some(Box::new(LoggingSettings::with_log_crate(self.log_level)?))
        } else {
            None
        };

        #[cfg(not(feature = "tracing"))]
        let logging = if let Some(callback) = self.log_callback {
            Some(Box::new(LoggingSettings::with_callback(
                self.log_level,
                callback,
            )?))
        } else if self.log_level != LogLevel::Disable {
            Some(Box::new(LoggingSettings::with_log_crate(self.log_level)?))
        } else {
            None
        };

        let logging_ptr = logging
            .as_ref()
            .map(|l| l.as_raw())
            .unwrap_or(ptr::null_mut());

        // Create context
        let mut ctx: *mut librist_sys::rist_ctx = ptr::null_mut();
        let ret = unsafe {
            librist_sys::rist_receiver_create(&mut ctx, self.profile.into(), logging_ptr)
        };

        if ret != 0 || ctx.is_null() {
            return Err(Error::ContextCreationFailed);
        }

        let callbacks = Arc::new(Mutex::new(ReceiverCallbacks {
            stats: self.stats_callback,
            connection: self.connection_callback,
            data: self.data_callback,
            auth_connect: self.auth_connect_callback,
            auth_disconnect: self.auth_disconnect_callback,
            oob: self.oob_callback,
        }));

        let receiver = RistReceiver {
            ctx: NonNull::new(ctx).unwrap(),
            profile: self.profile,
            started: AtomicBool::new(false),
            oob_enabled: AtomicBool::new(false),
            peers: Mutex::new(Vec::new()),
            callbacks,
            callback_arc_count: AtomicU32::new(0),
            logging,
        };

        // Apply settings
        receiver.set_nack_type(self.nack_type)?;
        if let Some(size) = self.fifo_size {
            receiver.set_output_fifo_size(size)?;
        }

        // Set up callbacks
        if receiver.callbacks.lock().data.is_some() {
            receiver.setup_data_callback()?;
        }

        if receiver.callbacks.lock().stats.is_some() {
            if let Some(interval) = self.stats_interval_ms {
                receiver.setup_stats_callback(interval)?;
            }
        }

        if receiver.callbacks.lock().connection.is_some() {
            receiver.setup_connection_callback()?;
        }

        // Set up auth callbacks if either is provided
        {
            let guard = receiver.callbacks.lock();
            if guard.auth_connect.is_some() || guard.auth_disconnect.is_some() {
                drop(guard);
                receiver.setup_auth_callback()?;
            }
        }

        // Set up OOB callback if provided
        if receiver.callbacks.lock().oob.is_some() {
            receiver.setup_oob_callback()?;
        }

        Ok(receiver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn test_receiver_builder() {
        let _builder = ReceiverBuilder::default()
            .profile(Profile::Main)
            .nack_type(NackType::Range)
            .fifo_size(1024);
    }

    #[test]
    fn test_receiver_builder_with_auth_callbacks() {
        let auth_called = Arc::new(AtomicBool::new(false));
        let disconnect_called = Arc::new(AtomicBool::new(false));

        let auth_called_clone = Arc::clone(&auth_called);
        let disconnect_called_clone = Arc::clone(&disconnect_called);

        let _builder = ReceiverBuilder::default()
            .profile(Profile::Main)
            .on_auth_connect(move |_conn_ip, _conn_port, _local_ip, _local_port, _peer| {
                auth_called_clone.store(true, Ordering::SeqCst);
                true // Accept connection
            })
            .on_auth_disconnect(move |_peer| {
                disconnect_called_clone.store(true, Ordering::SeqCst);
            });

        // Callbacks haven't been called yet (just configured)
        assert!(!auth_called.load(Ordering::SeqCst));
        assert!(!disconnect_called.load(Ordering::SeqCst));
    }

    #[test]
    fn test_receiver_with_auth_accept_all() {
        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .on_auth_connect(|conn_ip, conn_port, _local_ip, _local_port, peer| {
                println!(
                    "Auth request from {}:{} (peer={})",
                    conn_ip, conn_port, peer
                );
                true // Accept all
            })
            .build();

        assert!(receiver.is_ok());
    }

    #[test]
    fn test_receiver_with_auth_reject_all() {
        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .on_auth_connect(|_conn_ip, _conn_port, _local_ip, _local_port, _peer| {
                false // Reject all
            })
            .build();

        assert!(receiver.is_ok());
    }

    #[test]
    fn test_receiver_with_auth_ip_filter() {
        let allowed_ips = vec!["127.0.0.1".to_string(), "192.168.1.100".to_string()];

        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .on_auth_connect(move |conn_ip, _conn_port, _local_ip, _local_port, _peer| {
                allowed_ips.contains(&conn_ip.to_string())
            })
            .build();

        assert!(receiver.is_ok());
    }

    #[test]
    fn test_receiver_with_disconnect_tracking() {
        let disconnect_count = Arc::new(AtomicU32::new(0));
        let count_clone = Arc::clone(&disconnect_count);

        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .on_auth_disconnect(move |peer| {
                println!("Peer {} disconnected", peer);
                count_clone.fetch_add(1, Ordering::SeqCst);
            })
            .build();

        assert!(receiver.is_ok());
        assert_eq!(disconnect_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_receiver_with_both_auth_callbacks() {
        let connect_count = Arc::new(AtomicU32::new(0));
        let disconnect_count = Arc::new(AtomicU32::new(0));
        let connect_clone = Arc::clone(&connect_count);
        let disconnect_clone = Arc::clone(&disconnect_count);

        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .on_auth_connect(move |_conn_ip, _conn_port, _local_ip, _local_port, _peer| {
                connect_clone.fetch_add(1, Ordering::SeqCst);
                true
            })
            .on_auth_disconnect(move |_peer| {
                disconnect_clone.fetch_add(1, Ordering::SeqCst);
            })
            .build();

        assert!(receiver.is_ok());
        assert_eq!(connect_count.load(Ordering::SeqCst), 0);
        assert_eq!(disconnect_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_receiver_with_peer_info_access() {
        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .on_auth_connect(|conn_ip, conn_port, _local_ip, _local_port, peer| {
                println!("Connection from {}:{}", conn_ip, conn_port);
                println!("Peer ID: {}", peer.id);
                if let Some(ref cname) = peer.cname {
                    println!("Peer CNAME: {}", cname);
                }
                true
            })
            .on_auth_disconnect(|peer| {
                println!("Disconnected: {} (cname: {:?})", peer.id, peer.cname);
            })
            .build();

        assert!(receiver.is_ok());
    }

    #[test]
    fn test_receiver_with_cname_filter() {
        let allowed_cnames = vec!["trusted-sender".to_string(), "backup-sender".to_string()];

        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .on_auth_connect(move |_conn_ip, _conn_port, _local_ip, _local_port, peer| {
                // Only accept peers with recognized CNAMEs
                peer.cname
                    .as_ref()
                    .map(|c| allowed_cnames.contains(c))
                    .unwrap_or(false)
            })
            .build();

        assert!(receiver.is_ok());
    }

    #[test]
    fn test_receiver_with_oob() {
        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .on_oob(|block| {
                println!("OOB: {} bytes", block.payload().len());
            })
            .build();

        assert!(receiver.is_ok());
    }

    #[test]
    fn test_receiver_oob_not_enabled() {
        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .build()
            .unwrap();
        receiver.add_peer("rist://@:15000").unwrap();
        receiver.start().unwrap();
        let result = receiver.send_oob(b"test");
        assert!(matches!(result, Err(Error::OobNotEnabled)));
    }
}
