//! RIST context types (Sender and Receiver).

use crate::data::{DataBlock, DataBlockBuilder};
use crate::error::{check_result, Error, Result};
use crate::logging::{LogLevel, LoggingSettings};
use crate::peer::{PeerConfig, PeerHandle};
use crate::stats::{ReceiverStats, SenderStats, StatsWrapper};
use crate::types::*;
use parking_lot::Mutex;
use std::os::raw::{c_int, c_void};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// ============================================================================
// Callback Storage
// ============================================================================

type StatsCallback<T> = Box<dyn Fn(&T) + Send + Sync>;
type ConnectionCallback = Box<dyn Fn(u32, ConnectionStatus) + Send + Sync>;
type DataCallback = Box<dyn Fn(DataBlock) + Send + Sync>;
type AuthCallback = Box<dyn Fn(&str, u16, &str, u16) -> bool + Send + Sync>;

#[derive(Default)]
struct SenderCallbacks {
    stats: Option<StatsCallback<SenderStats>>,
    connection: Option<ConnectionCallback>,
}


#[derive(Default)]
struct ReceiverCallbacks {
    stats: Option<StatsCallback<ReceiverStats>>,
    connection: Option<ConnectionCallback>,
    data: Option<DataCallback>,
    #[allow(dead_code)]
    auth: Option<AuthCallback>,
}


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
    started: AtomicBool,
    peers: Mutex<Vec<PeerHandle>>,
    callbacks: Arc<Mutex<SenderCallbacks>>,
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
        let ret =
            unsafe { librist_sys::rist_peer_destroy(self.ctx.as_ptr(), peer.as_raw()) };
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

    fn setup_stats_callback(&self, interval_ms: u32) -> Result<()> {
        let callbacks = Arc::clone(&self.callbacks);
        let ctx_ptr = Arc::into_raw(callbacks) as *mut c_void;

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

    fn setup_connection_callback(&self) -> Result<()> {
        let callbacks = Arc::clone(&self.callbacks);
        let ctx_ptr = Arc::into_raw(callbacks) as *mut c_void;

        let ret = unsafe {
            librist_sys::rist_connection_status_callback_set(
                self.ctx.as_ptr(),
                Some(connection_trampoline),
                ctx_ptr,
            )
        };
        check_result(ret)
    }
}

impl Drop for RistSender {
    fn drop(&mut self) {
        self.peers.lock().clear();
        unsafe {
            librist_sys::rist_destroy(self.ctx.as_ptr());
        }
    }
}

unsafe impl Send for RistSender {}
unsafe impl Sync for RistSender {}

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
    started: AtomicBool,
    peers: Mutex<Vec<PeerHandle>>,
    callbacks: Arc<Mutex<ReceiverCallbacks>>,
    #[allow(dead_code)]
    logging: Option<Box<LoggingSettings>>,
}

impl RistReceiver {
    /// Creates a new builder for configuring a receiver.
    pub fn builder() -> ReceiverBuilder {
        ReceiverBuilder::default()
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
        let ret =
            unsafe { librist_sys::rist_receiver_data_read2(self.ctx.as_ptr(), &mut block, timeout_ms) };

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

    /// Sets the NACK type.
    pub fn set_nack_type(&self, nack_type: NackType) -> Result<()> {
        let ret =
            unsafe { librist_sys::rist_receiver_nack_type_set(self.ctx.as_ptr(), nack_type.into()) };
        check_result(ret)
    }

    /// Sets the output FIFO buffer size.
    pub fn set_output_fifo_size(&self, size: u32) -> Result<()> {
        let ret =
            unsafe { librist_sys::rist_receiver_set_output_fifo_size(self.ctx.as_ptr(), size) };
        check_result(ret)
    }

    /// Returns the number of connected peers.
    pub fn peer_count(&self) -> usize {
        self.peers.lock().len()
    }

    fn setup_data_callback(&self) -> Result<()> {
        let callbacks = Arc::clone(&self.callbacks);
        let ctx_ptr = Arc::into_raw(callbacks) as *mut c_void;

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

        let ret = unsafe {
            librist_sys::rist_connection_status_callback_set(
                self.ctx.as_ptr(),
                Some(connection_trampoline),
                ctx_ptr,
            )
        };
        check_result(ret)
    }
}

impl Drop for RistReceiver {
    fn drop(&mut self) {
        self.peers.lock().clear();
        unsafe {
            librist_sys::rist_destroy(self.ctx.as_ptr());
        }
    }
}

unsafe impl Send for RistReceiver {}
unsafe impl Sync for RistReceiver {}

// ============================================================================
// Builders
// ============================================================================

/// Builder for configuring a RIST sender.
#[derive(Default)]
pub struct SenderBuilder {
    profile: Profile,
    flow_id: Option<u32>,
    log_level: LogLevel,
    log_callback: Option<Box<dyn Fn(LogLevel, &str) + Send + Sync>>,
    stats_interval_ms: Option<u32>,
    stats_callback: Option<StatsCallback<SenderStats>>,
    connection_callback: Option<ConnectionCallback>,
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

    /// Builds the sender.
    pub fn build(self) -> Result<RistSender> {
        let flow_id = self.flow_id.unwrap_or_else(RistSender::random_flow_id);

        // Create logging settings
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
        }));

        let sender = RistSender {
            ctx: NonNull::new(ctx).unwrap(),
            started: AtomicBool::new(false),
            peers: Mutex::new(Vec::new()),
            callbacks,
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

        Ok(sender)
    }
}

/// Builder for configuring a RIST receiver.
#[derive(Default)]
pub struct ReceiverBuilder {
    profile: Profile,
    log_level: LogLevel,
    log_callback: Option<Box<dyn Fn(LogLevel, &str) + Send + Sync>>,
    nack_type: NackType,
    fifo_size: Option<u32>,
    stats_interval_ms: Option<u32>,
    stats_callback: Option<StatsCallback<ReceiverStats>>,
    connection_callback: Option<ConnectionCallback>,
    data_callback: Option<DataCallback>,
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

    /// Builds the receiver.
    pub fn build(self) -> Result<RistReceiver> {
        // Create logging settings
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
        let ret =
            unsafe { librist_sys::rist_receiver_create(&mut ctx, self.profile.into(), logging_ptr) };

        if ret != 0 || ctx.is_null() {
            return Err(Error::ContextCreationFailed);
        }

        let callbacks = Arc::new(Mutex::new(ReceiverCallbacks {
            stats: self.stats_callback,
            connection: self.connection_callback,
            data: self.data_callback,
            auth: None,
        }));

        let receiver = RistReceiver {
            ctx: NonNull::new(ctx).unwrap(),
            started: AtomicBool::new(false),
            peers: Mutex::new(Vec::new()),
            callbacks,
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

        Ok(receiver)
    }
}

// ============================================================================
// Callback Trampolines
// ============================================================================

unsafe extern "C" fn sender_stats_trampoline(
    arg: *mut c_void,
    stats: *const librist_sys::rist_stats,
) -> c_int {
    if arg.is_null() || stats.is_null() {
        return 0;
    }

    let _ = std::panic::catch_unwind(|| {
        // SAFETY: arg is a pointer we passed via Arc::into_raw
        let callbacks = unsafe { Arc::from_raw(arg as *const Mutex<SenderCallbacks>) };
        let callbacks_ref = Arc::clone(&callbacks);
        let _ = Arc::into_raw(callbacks); // Don't drop, just release our reference

        let guard = callbacks_ref.lock();
        if let Some(ref callback) = guard.stats {
            // SAFETY: stats is valid for the duration of this callback
            let wrapper = unsafe { StatsWrapper::from_raw(stats) };
            if let Some(sender_stats) = wrapper.as_sender_stats() {
                callback(&sender_stats);
            }
            std::mem::forget(wrapper); // Don't free, librist owns it
        }
    });

    0
}

unsafe extern "C" fn receiver_stats_trampoline(
    arg: *mut c_void,
    stats: *const librist_sys::rist_stats,
) -> c_int {
    if arg.is_null() || stats.is_null() {
        return 0;
    }

    let _ = std::panic::catch_unwind(|| {
        // SAFETY: arg is a pointer we passed via Arc::into_raw
        let callbacks = unsafe { Arc::from_raw(arg as *const Mutex<ReceiverCallbacks>) };
        let callbacks_ref = Arc::clone(&callbacks);
        let _ = Arc::into_raw(callbacks);

        let guard = callbacks_ref.lock();
        if let Some(ref callback) = guard.stats {
            // SAFETY: stats is valid for the duration of this callback
            let wrapper = unsafe { StatsWrapper::from_raw(stats) };
            if let Some(receiver_stats) = wrapper.as_receiver_stats() {
                callback(&receiver_stats);
            }
            std::mem::forget(wrapper);
        }
    });

    0
}

unsafe extern "C" fn receiver_data_trampoline(
    arg: *mut c_void,
    data_block: *mut librist_sys::rist_data_block,
) -> c_int {
    if arg.is_null() || data_block.is_null() {
        return 0;
    }

    let _ = std::panic::catch_unwind(|| {
        // SAFETY: arg is a pointer we passed via Arc::into_raw
        let callbacks = unsafe { Arc::from_raw(arg as *const Mutex<ReceiverCallbacks>) };
        let callbacks_ref = Arc::clone(&callbacks);
        let _ = Arc::into_raw(callbacks);

        let guard = callbacks_ref.lock();
        if let Some(ref callback) = guard.data {
            let block = DataBlock::from_received(data_block);
            callback(block);
        }
    });

    0
}

unsafe extern "C" fn connection_trampoline(
    arg: *mut c_void,
    peer: *mut librist_sys::rist_peer,
    status: librist_sys::rist_connection_status,
) {
    if arg.is_null() {
        return;
    }

    let _ = std::panic::catch_unwind(|| {
        // Try sender callbacks first
        // SAFETY: arg points to valid data we passed in
        if let Some(callbacks) = unsafe { (arg as *const Mutex<SenderCallbacks>).as_ref() } {
            if let Some(guard) = callbacks.try_lock() {
                if let Some(ref callback) = guard.connection {
                    let peer_id = if peer.is_null() {
                        0
                    } else {
                        // SAFETY: peer is valid if not null
                        unsafe { librist_sys::rist_peer_get_id(peer) }
                    };
                    callback(peer_id, status.into());
                    return;
                }
            }
        }

        // Try receiver callbacks
        // SAFETY: arg points to valid data we passed in
        if let Some(callbacks) = unsafe { (arg as *const Mutex<ReceiverCallbacks>).as_ref() } {
            if let Some(guard) = callbacks.try_lock() {
                if let Some(ref callback) = guard.connection {
                    let peer_id = if peer.is_null() {
                        0
                    } else {
                        // SAFETY: peer is valid if not null
                        unsafe { librist_sys::rist_peer_get_id(peer) }
                    };
                    callback(peer_id, status.into());
                }
            }
        }
    });
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
    fn test_receiver_builder() {
        let _builder = ReceiverBuilder::default()
            .profile(Profile::Main)
            .nack_type(NackType::Range)
            .fifo_size(1024);
    }
}
