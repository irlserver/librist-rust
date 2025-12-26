//! RIST sender context and builder.

use crate::callbacks::{
    sender_connection_trampoline, sender_oob_trampoline, sender_stats_trampoline,
    ConnectionCallback, LogCallback, OobCallback, SenderCallbacks, StatsCallback,
};
use crate::data::DataBlockBuilder;
use crate::error::{check_result, Error, Result};
use crate::logging::{LogLevel, LoggingSettings};
use crate::oob::OobBlock;
use crate::peer::{PeerConfig, PeerHandle};
use crate::stats::SenderStats;
use crate::types::*;
use parking_lot::Mutex;
use std::os::raw::{c_int, c_void};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

// ============================================================================
// RistSender
// ============================================================================

/// A RIST sender context for transmitting data.
///
/// The sender manages connections to one or more receiver peers and
/// handles all aspects of reliable delivery including retransmissions.
///
/// # Thread Safety
///
/// `RistSender` is `Send + Sync` and can be shared between threads
/// using `Arc<RistSender>`.
///
/// # Example
///
/// ```no_run
/// use librist::{RistSender, Profile};
///
/// let sender = RistSender::builder()
///     .profile(Profile::Main)
///     .build()?;
///
/// // Add destination peer
/// sender.add_peer("rist://192.168.1.100:5000")?;
///
/// // Start the sender
/// sender.start()?;
///
/// // Send data
/// let data = vec![0u8; 1316];
/// sender.send(&data)?;
/// # Ok::<(), librist::Error>(())
/// ```
pub struct RistSender {
    ctx: NonNull<librist_sys::rist_ctx>,
    profile: Profile,
    started: AtomicBool,
    oob_enabled: AtomicBool,
    peers: Mutex<Vec<PeerHandle>>,
    pub(crate) callbacks: Arc<Mutex<SenderCallbacks>>,
    /// Number of times we've called Arc::into_raw for callbacks (need to reclaim in Drop)
    callback_arc_count: AtomicU32,
    #[allow(dead_code)]
    logging: Option<Box<LoggingSettings>>,
}

impl RistSender {
    /// Creates a new builder for configuring a sender.
    pub fn builder() -> SenderBuilder {
        SenderBuilder::default()
    }

    /// Generates a random flow ID.
    ///
    /// Flow IDs are used to identify data streams and should be
    /// unique per sender.
    pub fn random_flow_id() -> u32 {
        unsafe { librist_sys::rist_flow_id_create() }
    }

    /// Gets the current flow ID.
    pub fn flow_id(&self) -> Result<u32> {
        let mut flow_id = 0u32;
        let ret = unsafe { librist_sys::rist_sender_flow_id_get(self.ctx.as_ptr(), &mut flow_id) };
        check_result(ret)?;
        Ok(flow_id)
    }

    /// Sets the flow ID (must be called before start).
    pub fn set_flow_id(&self, flow_id: u32) -> Result<()> {
        if self.started.load(Ordering::Acquire) {
            return Err(Error::AlreadyStarted);
        }
        let ret = unsafe { librist_sys::rist_sender_flow_id_set(self.ctx.as_ptr(), flow_id) };
        check_result(ret)
    }

    /// Returns the RIST profile being used.
    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Adds a peer using a RIST URL.
    ///
    /// # URL Format
    ///
    /// `rist://host:port[?options]`
    ///
    /// See [`PeerConfig::from_url`] for URL option details.
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

    /// Removes a peer from the sender.
    pub fn remove_peer(&self, peer: &PeerHandle) -> Result<()> {
        let ret = unsafe { librist_sys::rist_peer_destroy(self.ctx.as_ptr(), peer.as_raw()) };
        check_result(ret)?;

        // Remove from our tracking
        let mut peers = self.peers.lock();
        peers.retain(|p| p.id() != peer.id());
        Ok(())
    }

    /// Starts the sender.
    ///
    /// Must be called after adding at least one peer.
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

    /// Returns whether the sender has been started.
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }

    /// Sends data over the RIST connection.
    ///
    /// # Returns
    ///
    /// The number of bytes queued for sending.
    pub fn send(&self, data: &[u8]) -> Result<usize> {
        if !self.started.load(Ordering::Acquire) {
            return Err(Error::NotStarted);
        }

        let data_block = librist_sys::rist_data_block {
            payload: data.as_ptr() as *const c_void,
            payload_len: data.len(),
            ts_ntp: 0,
            virt_src_port: 0,
            virt_dst_port: DEFAULT_VIRT_DST_PORT,
            peer: ptr::null_mut(),
            flow_id: 0,
            seq: 0,
            flags: 0,
            ref_: ptr::null_mut(),
        };

        let ret = unsafe { librist_sys::rist_sender_data_write(self.ctx.as_ptr(), &data_block) };
        if ret < 0 {
            check_result(ret)?;
        }
        Ok(ret as usize)
    }

    /// Sends data to a specific virtual destination port.
    pub fn send_to_port(&self, data: &[u8], virt_dst_port: u16) -> Result<usize> {
        if !self.started.load(Ordering::Acquire) {
            return Err(Error::NotStarted);
        }

        let data_block = librist_sys::rist_data_block {
            payload: data.as_ptr() as *const c_void,
            payload_len: data.len(),
            ts_ntp: 0,
            virt_src_port: 0,
            virt_dst_port,
            peer: ptr::null_mut(),
            flow_id: 0,
            seq: 0,
            flags: 0,
            ref_: ptr::null_mut(),
        };

        let ret = unsafe { librist_sys::rist_sender_data_write(self.ctx.as_ptr(), &data_block) };
        if ret < 0 {
            check_result(ret)?;
        }
        Ok(ret as usize)
    }

    /// Sends data with full control over block parameters.
    pub fn send_block(&self, data: &[u8], builder: &DataBlockBuilder) -> Result<usize> {
        if !self.started.load(Ordering::Acquire) {
            return Err(Error::NotStarted);
        }

        let (block, _lifetime) = builder.build_raw(data);
        let ret = unsafe { librist_sys::rist_sender_data_write(self.ctx.as_ptr(), &block) };
        if ret < 0 {
            check_result(ret)?;
        }
        Ok(ret as usize)
    }

    /// Sends out-of-band data.
    ///
    /// OOB data is transmitted via the RTCP channel with no buffering or
    /// retransmission. This is suitable for low-latency signaling.
    ///
    /// # Requirements
    ///
    /// - OOB must be enabled via `enable_oob()` in the builder
    /// - Only works with Main or Advanced profile
    /// - Must be called after `start()`
    ///
    /// # Returns
    ///
    /// The number of bytes written.
    pub fn send_oob(&self, data: &[u8]) -> Result<usize> {
        self.send_oob_block(&OobBlock::new(data.to_vec()))
    }

    /// Sends an OOB block with optional peer targeting.
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

        let oob_block = librist_sys::rist_oob_block {
            peer: ptr::null_mut(), // TODO: support targeting specific peer
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

    /// Enables Null Packet Deletion (NPD).
    ///
    /// NPD removes null MPEG-TS packets to save bandwidth.
    pub fn enable_npd(&self) -> Result<()> {
        let ret = unsafe { librist_sys::rist_sender_npd_enable(self.ctx.as_ptr()) };
        check_result(ret)
    }

    /// Disables Null Packet Deletion (NPD).
    pub fn disable_npd(&self) -> Result<()> {
        let ret = unsafe { librist_sys::rist_sender_npd_disable(self.ctx.as_ptr()) };
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

    pub(crate) fn setup_stats_callback(&self, interval_ms: u32) -> Result<()> {
        let callbacks = Arc::clone(&self.callbacks);
        let ctx_ptr = Arc::into_raw(callbacks) as *mut c_void;
        self.callback_arc_count.fetch_add(1, Ordering::SeqCst);

        let ret = unsafe {
            librist_sys::rist_stats_callback_set(
                self.ctx.as_ptr(),
                interval_ms as c_int,
                Some(sender_stats_trampoline),
                ctx_ptr,
            )
        };
        check_result(ret)
    }

    pub(crate) fn setup_connection_callback(&self) -> Result<()> {
        let callbacks = Arc::clone(&self.callbacks);
        let ctx_ptr = Arc::into_raw(callbacks) as *mut c_void;
        self.callback_arc_count.fetch_add(1, Ordering::SeqCst);

        let ret = unsafe {
            librist_sys::rist_connection_status_callback_set(
                self.ctx.as_ptr(),
                Some(sender_connection_trampoline),
                ctx_ptr,
            )
        };
        check_result(ret)
    }

    pub(crate) fn setup_oob_callback(&self) -> Result<()> {
        if self.profile == Profile::Simple {
            return Err(Error::ProfileNotSupported);
        }

        let callbacks = Arc::clone(&self.callbacks);
        let ctx_ptr = Arc::into_raw(callbacks) as *mut c_void;
        self.callback_arc_count.fetch_add(1, Ordering::SeqCst);

        let ret = unsafe {
            librist_sys::rist_oob_callback_set(
                self.ctx.as_ptr(),
                Some(sender_oob_trampoline),
                ctx_ptr,
            )
        };
        if ret == 0 {
            self.oob_enabled.store(true, Ordering::Release);
        }
        check_result(ret)
    }
}

impl Drop for RistSender {
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

unsafe impl Send for RistSender {}
unsafe impl Sync for RistSender {}

// ============================================================================
// SenderBuilder
// ============================================================================

/// Builder for configuring a RIST sender.
#[derive(Default)]
pub struct SenderBuilder {
    profile: Profile,
    flow_id: Option<u32>,
    log_level: LogLevel,
    log_callback: Option<LogCallback>,
    #[cfg(feature = "tracing")]
    use_tracing: bool,
    stats_interval_ms: Option<u32>,
    stats_callback: Option<StatsCallback<SenderStats>>,
    connection_callback: Option<ConnectionCallback>,
    oob_callback: Option<OobCallback>,
    enable_oob: bool,
}

impl SenderBuilder {
    /// Sets the RIST profile.
    pub fn profile(mut self, profile: Profile) -> Self {
        self.profile = profile;
        self
    }

    /// Sets the flow ID (random if not specified).
    pub fn flow_id(mut self, flow_id: u32) -> Self {
        self.flow_id = Some(flow_id);
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
    /// use librist::{RistSender, Profile, LogLevel};
    ///
    /// let sender = RistSender::builder()
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

    /// Sets the stats callback with reporting interval.
    pub fn on_stats<F>(mut self, interval_ms: u32, callback: F) -> Self
    where
        F: Fn(&SenderStats) + Send + Sync + 'static,
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

    /// Enables out-of-band data channel.
    ///
    /// This must be called to enable `send_oob()`. Only works with
    /// Main or Advanced profile.
    pub fn enable_oob(mut self) -> Self {
        self.enable_oob = true;
        self
    }

    /// Sets a callback for receiving out-of-band data.
    ///
    /// Automatically enables OOB if not already enabled.
    pub fn on_oob<F>(mut self, callback: F) -> Self
    where
        F: Fn(OobBlock) + Send + Sync + 'static,
    {
        self.oob_callback = Some(Box::new(callback));
        self.enable_oob = true;
        self
    }

    /// Builds the sender.
    pub fn build(self) -> Result<RistSender> {
        let flow_id = self.flow_id.unwrap_or_else(RistSender::random_flow_id);

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
            librist_sys::rist_sender_create(&mut ctx, self.profile.into(), flow_id, logging_ptr)
        };

        if ret != 0 || ctx.is_null() {
            return Err(Error::ContextCreationFailed);
        }

        let callbacks = Arc::new(Mutex::new(SenderCallbacks {
            stats: self.stats_callback,
            connection: self.connection_callback,
            oob: self.oob_callback,
        }));

        let sender = RistSender {
            ctx: NonNull::new(ctx).unwrap(),
            profile: self.profile,
            started: AtomicBool::new(false),
            oob_enabled: AtomicBool::new(false),
            peers: Mutex::new(Vec::new()),
            callbacks,
            callback_arc_count: AtomicU32::new(0),
            logging,
        };

        // Set up callbacks
        if sender.callbacks.lock().stats.is_some() {
            if let Some(interval) = self.stats_interval_ms {
                sender.setup_stats_callback(interval)?;
            }
        }

        if sender.callbacks.lock().connection.is_some() {
            sender.setup_connection_callback()?;
        }

        // Set up OOB if enabled
        if self.enable_oob {
            sender.setup_oob_callback()?;
        }

        Ok(sender)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sender_builder() {
        // Just test that the builder compiles and has the right methods
        let _builder = SenderBuilder::default()
            .profile(Profile::Main)
            .flow_id(12345)
            .log_level(LogLevel::Info);
    }

    #[test]
    fn test_sender_builder_with_oob() {
        let _builder = SenderBuilder::default()
            .profile(Profile::Main)
            .enable_oob()
            .on_oob(|block| {
                println!("OOB: {} bytes", block.payload().len());
            });
    }

    #[test]
    fn test_sender_create() {
        let sender = RistSender::builder().profile(Profile::Main).build();
        assert!(sender.is_ok());
        let sender = sender.unwrap();
        assert!(!sender.is_started());
        assert_eq!(sender.profile(), Profile::Main);
    }

    #[test]
    fn test_sender_send_before_start() {
        let sender = RistSender::builder().profile(Profile::Main).build().unwrap();
        sender.add_peer("rist://127.0.0.1:5000").unwrap();
        let result = sender.send(&[1, 2, 3]);
        assert!(matches!(result, Err(Error::NotStarted)));
    }

    #[test]
    fn test_sender_oob_not_enabled() {
        let sender = RistSender::builder().profile(Profile::Main).build().unwrap();
        sender.add_peer("rist://127.0.0.1:5000").unwrap();
        sender.start().unwrap();
        let result = sender.send_oob(b"test");
        assert!(matches!(result, Err(Error::OobNotEnabled)));
    }
}
