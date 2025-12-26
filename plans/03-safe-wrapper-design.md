# librist Safe Wrapper Design

## Overview

The `librist` crate provides a safe, idiomatic Rust API over the raw FFI bindings. This document details the safe abstraction layer design.

## Core Types

### RistSender

The primary type for sending RIST streams.

```rust
/// A RIST sender context for transmitting data over the RIST protocol.
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
/// sender.add_peer("rist://192.168.1.100:5000")?;
/// sender.start()?;
///
/// // Send data
/// let data = vec![0u8; 1316]; // MPEG-TS packet
/// sender.send(&data)?;
/// # Ok::<(), librist::Error>(())
/// ```
pub struct RistSender {
    /// Raw context pointer (never null after construction)
    ctx: NonNull<rist_ctx>,
    
    /// Tracks whether start() has been called
    started: AtomicBool,
    
    /// Active peers (for lifetime management)
    peers: Mutex<Vec<PeerHandle>>,
    
    /// Callback storage (prevents deallocation while callbacks active)
    callbacks: Mutex<SenderCallbacks>,
    
    /// Logging settings (owned)
    logging: Option<Box<LoggingSettings>>,
}

impl RistSender {
    /// Creates a new builder for configuring a sender.
    pub fn builder() -> SenderBuilder {
        SenderBuilder::default()
    }
    
    /// Creates a random flow ID.
    pub fn random_flow_id() -> u32 {
        unsafe { rist_flow_id_create() }
    }
    
    /// Gets the current flow ID.
    pub fn flow_id(&self) -> Result<u32> {
        let mut flow_id = 0u32;
        let ret = unsafe { rist_sender_flow_id_get(self.ctx.as_ptr(), &mut flow_id) };
        check_result(ret)?;
        Ok(flow_id)
    }
    
    /// Sets the flow ID (must be called before start).
    pub fn set_flow_id(&self, flow_id: u32) -> Result<()> {
        if self.started.load(Ordering::Acquire) {
            return Err(Error::AlreadyStarted);
        }
        let ret = unsafe { rist_sender_flow_id_set(self.ctx.as_ptr(), flow_id) };
        check_result(ret)
    }
    
    /// Adds a peer using a RIST URL.
    ///
    /// # URL Format
    ///
    /// `rist://[host]:[port]?[options]`
    ///
    /// Options include:
    /// - `bandwidth=<kbps>` - Maximum bandwidth
    /// - `buffer=<ms>` - Buffer size
    /// - `secret=<key>` - Encryption secret
    /// - `aes-type=<128|256>` - AES key size
    /// - `weight=<n>` - Bonding weight
    pub fn add_peer(&self, url: &str) -> Result<PeerHandle> {
        let url_cstr = CString::new(url).map_err(|_| Error::InvalidUrl(url.to_string()))?;
        
        let mut config: *mut rist_peer_config = ptr::null_mut();
        let ret = unsafe { rist_parse_address2(url_cstr.as_ptr(), &mut config) };
        check_result(ret)?;
        
        // Ensure config is freed even on error
        let _config_guard = scopeguard::guard(config, |c| {
            if !c.is_null() {
                unsafe { rist_peer_config_free2(&mut c as *mut _) };
            }
        });
        
        let mut peer: *mut rist_peer = ptr::null_mut();
        let ret = unsafe { rist_peer_create(self.ctx.as_ptr(), &mut peer, config) };
        check_result(ret)?;
        
        let handle = PeerHandle::new(peer, self.ctx);
        self.peers.lock().push(handle.clone());
        Ok(handle)
    }
    
    /// Adds a peer with explicit configuration.
    pub fn add_peer_with_config(&self, config: &PeerConfig) -> Result<PeerHandle> {
        let raw_config = config.to_raw();
        
        let mut peer: *mut rist_peer = ptr::null_mut();
        let ret = unsafe { rist_peer_create(self.ctx.as_ptr(), &mut peer, &raw_config) };
        check_result(ret)?;
        
        let handle = PeerHandle::new(peer, self.ctx);
        self.peers.lock().push(handle.clone());
        Ok(handle)
    }
    
    /// Starts the sender (must be called after adding peers).
    pub fn start(&self) -> Result<()> {
        if self.started.swap(true, Ordering::AcqRel) {
            return Err(Error::AlreadyStarted);
        }
        let ret = unsafe { rist_start(self.ctx.as_ptr()) };
        if ret != 0 {
            self.started.store(false, Ordering::Release);
            check_result(ret)
        } else {
            Ok(())
        }
    }
    
    /// Sends data over the RIST connection.
    ///
    /// # Arguments
    ///
    /// * `data` - The payload to send
    ///
    /// # Returns
    ///
    /// The number of bytes written on success.
    pub fn send(&self, data: &[u8]) -> Result<usize> {
        if !self.started.load(Ordering::Acquire) {
            return Err(Error::NotStarted);
        }
        
        let data_block = rist_data_block {
            payload: data.as_ptr() as *const c_void,
            payload_len: data.len(),
            ts_ntp: 0,
            virt_src_port: 0,
            virt_dst_port: RIST_DEFAULT_VIRT_DST_PORT,
            peer: ptr::null_mut(),
            flow_id: 0,
            seq: 0,
            flags: 0,
            ref_: ptr::null_mut(),
        };
        
        let ret = unsafe { rist_sender_data_write(self.ctx.as_ptr(), &data_block) };
        if ret < 0 {
            check_result(ret)
        } else {
            Ok(ret as usize)
        }
    }
    
    /// Sends data with a specific virtual destination port.
    pub fn send_to_port(&self, data: &[u8], virt_dst_port: u16) -> Result<usize> {
        if !self.started.load(Ordering::Acquire) {
            return Err(Error::NotStarted);
        }
        
        let data_block = rist_data_block {
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
        
        let ret = unsafe { rist_sender_data_write(self.ctx.as_ptr(), &data_block) };
        if ret < 0 {
            check_result(ret)
        } else {
            Ok(ret as usize)
        }
    }
    
    /// Sends data with full control over the data block.
    pub fn send_block(&self, block: &DataBlock) -> Result<usize> {
        if !self.started.load(Ordering::Acquire) {
            return Err(Error::NotStarted);
        }
        
        let ret = unsafe { rist_sender_data_write(self.ctx.as_ptr(), block.as_raw()) };
        if ret < 0 {
            check_result(ret)
        } else {
            Ok(ret as usize)
        }
    }
    
    /// Enables Null Packet Deletion (NPD).
    pub fn enable_npd(&self) -> Result<()> {
        let ret = unsafe { rist_sender_npd_enable(self.ctx.as_ptr()) };
        check_result(ret)
    }
    
    /// Disables Null Packet Deletion (NPD).
    pub fn disable_npd(&self) -> Result<()> {
        let ret = unsafe { rist_sender_npd_disable(self.ctx.as_ptr()) };
        check_result(ret)
    }
}

impl Drop for RistSender {
    fn drop(&mut self) {
        // Peers are destroyed automatically by rist_destroy
        self.peers.lock().clear();
        unsafe {
            rist_destroy(self.ctx.as_ptr());
        }
    }
}

// Safety: RistSender uses internal synchronization
unsafe impl Send for RistSender {}
unsafe impl Sync for RistSender {}
```

### RistReceiver

The primary type for receiving RIST streams.

```rust
/// A RIST receiver context for receiving data over the RIST protocol.
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
/// receiver.add_peer("rist://@:5000")?;  // Listen on port 5000
/// receiver.start()?;
///
/// // Receive data
/// loop {
///     let block = receiver.recv(1000)?;  // 1 second timeout
///     println!("Received {} bytes", block.payload().len());
/// }
/// # Ok::<(), librist::Error>(())
/// ```
pub struct RistReceiver {
    ctx: NonNull<rist_ctx>,
    started: AtomicBool,
    peers: Mutex<Vec<PeerHandle>>,
    callbacks: Mutex<ReceiverCallbacks>,
    logging: Option<Box<LoggingSettings>>,
}

impl RistReceiver {
    /// Creates a new builder for configuring a receiver.
    pub fn builder() -> ReceiverBuilder {
        ReceiverBuilder::default()
    }
    
    /// Adds a peer (listener) using a RIST URL.
    ///
    /// For listeners, use `@` before the port: `rist://@:5000`
    pub fn add_peer(&self, url: &str) -> Result<PeerHandle> {
        let url_cstr = CString::new(url).map_err(|_| Error::InvalidUrl(url.to_string()))?;
        
        let mut config: *mut rist_peer_config = ptr::null_mut();
        let ret = unsafe { rist_parse_address2(url_cstr.as_ptr(), &mut config) };
        check_result(ret)?;
        
        let _config_guard = scopeguard::guard(config, |c| {
            if !c.is_null() {
                unsafe { rist_peer_config_free2(&mut c as *mut _) };
            }
        });
        
        let mut peer: *mut rist_peer = ptr::null_mut();
        let ret = unsafe { rist_peer_create(self.ctx.as_ptr(), &mut peer, config) };
        check_result(ret)?;
        
        let handle = PeerHandle::new(peer, self.ctx);
        self.peers.lock().push(handle.clone());
        Ok(handle)
    }
    
    /// Starts the receiver.
    pub fn start(&self) -> Result<()> {
        if self.started.swap(true, Ordering::AcqRel) {
            return Err(Error::AlreadyStarted);
        }
        let ret = unsafe { rist_start(self.ctx.as_ptr()) };
        if ret != 0 {
            self.started.store(false, Ordering::Release);
            check_result(ret)
        } else {
            Ok(())
        }
    }
    
    /// Receives data from the RIST connection (blocking with timeout).
    ///
    /// # Arguments
    ///
    /// * `timeout_ms` - Timeout in milliseconds (-1 for infinite)
    ///
    /// # Returns
    ///
    /// A `DataBlock` containing the received data, or an error if timeout/failure.
    pub fn recv(&self, timeout_ms: i32) -> Result<DataBlock> {
        if !self.started.load(Ordering::Acquire) {
            return Err(Error::NotStarted);
        }
        
        let mut block: *mut rist_data_block = ptr::null_mut();
        let ret = unsafe { rist_receiver_data_read2(self.ctx.as_ptr(), &mut block, timeout_ms) };
        
        if ret < 0 {
            check_result(ret)
        } else if ret == 0 || block.is_null() {
            Err(Error::Timeout)
        } else {
            Ok(DataBlock::from_raw(block))
        }
    }
    
    /// Attempts to receive data without blocking.
    pub fn try_recv(&self) -> Result<Option<DataBlock>> {
        match self.recv(0) {
            Ok(block) => Ok(Some(block)),
            Err(Error::Timeout) => Ok(None),
            Err(e) => Err(e),
        }
    }
    
    /// Sets the NACK type (range or bitmask).
    pub fn set_nack_type(&self, nack_type: NackType) -> Result<()> {
        let ret = unsafe { rist_receiver_nack_type_set(self.ctx.as_ptr(), nack_type.into()) };
        check_result(ret)
    }
    
    /// Sets the output FIFO buffer size.
    pub fn set_output_fifo_size(&self, size: u32) -> Result<()> {
        let ret = unsafe { rist_receiver_set_output_fifo_size(self.ctx.as_ptr(), size) };
        check_result(ret)
    }
}

impl Drop for RistReceiver {
    fn drop(&mut self) {
        self.peers.lock().clear();
        unsafe {
            rist_destroy(self.ctx.as_ptr());
        }
    }
}

unsafe impl Send for RistReceiver {}
unsafe impl Sync for RistReceiver {}
```

### PeerHandle

A handle to a RIST peer connection.

```rust
/// A handle to a RIST peer.
///
/// Peers are automatically destroyed when the parent context is dropped.
#[derive(Clone)]
pub struct PeerHandle {
    peer: NonNull<rist_peer>,
    ctx: NonNull<rist_ctx>,
}

impl PeerHandle {
    fn new(peer: *mut rist_peer, ctx: NonNull<rist_ctx>) -> Self {
        Self {
            peer: NonNull::new(peer).expect("peer should not be null"),
            ctx,
        }
    }
    
    /// Gets the unique peer ID.
    pub fn id(&self) -> u32 {
        unsafe { rist_peer_get_id(self.peer.as_ptr()) }
    }
    
    /// Sets the peer weight for bonding.
    ///
    /// A weight of 0 means duplication (data sent to all peers).
    /// Higher weights receive proportionally more data.
    pub fn set_weight(&self, weight: u32) -> Result<()> {
        let ret = unsafe {
            rist_peer_weight_set(self.ctx.as_ptr(), self.peer.as_ptr(), weight)
        };
        check_result(ret)
    }
    
    /// Gets the CNAME for this peer.
    pub fn cname(&self) -> Option<String> {
        let mut cname_ptr: *const c_char = ptr::null();
        let ret = unsafe { rist_peer_get_cname(self.peer.as_ptr(), &mut cname_ptr) };
        if ret == 0 || cname_ptr.is_null() {
            None
        } else {
            unsafe { CStr::from_ptr(cname_ptr) }
                .to_str()
                .ok()
                .map(String::from)
        }
    }
}

// Safety: PeerHandle only contains raw pointers that are managed by librist
unsafe impl Send for PeerHandle {}
unsafe impl Sync for PeerHandle {}
```

### DataBlock

A wrapper for received data blocks.

```rust
/// A block of data received from RIST.
pub struct DataBlock {
    block: NonNull<rist_data_block>,
}

impl DataBlock {
    fn from_raw(block: *mut rist_data_block) -> Self {
        Self {
            block: NonNull::new(block).expect("block should not be null"),
        }
    }
    
    /// Gets the payload data.
    pub fn payload(&self) -> &[u8] {
        unsafe {
            let block = self.block.as_ref();
            std::slice::from_raw_parts(block.payload as *const u8, block.payload_len)
        }
    }
    
    /// Gets the NTP timestamp.
    pub fn timestamp_ntp(&self) -> u64 {
        unsafe { self.block.as_ref().ts_ntp }
    }
    
    /// Gets the virtual source port.
    pub fn virtual_src_port(&self) -> u16 {
        unsafe { self.block.as_ref().virt_src_port }
    }
    
    /// Gets the virtual destination port.
    pub fn virtual_dst_port(&self) -> u16 {
        unsafe { self.block.as_ref().virt_dst_port }
    }
    
    /// Gets the flow ID.
    pub fn flow_id(&self) -> u32 {
        unsafe { self.block.as_ref().flow_id }
    }
    
    /// Gets the sequence number.
    pub fn sequence(&self) -> u64 {
        unsafe { self.block.as_ref().seq }
    }
    
    /// Checks if there was a discontinuity before this block.
    pub fn is_discontinuity(&self) -> bool {
        unsafe { self.block.as_ref().flags & 1 != 0 }
    }
    
    /// Checks if this is the start of a flow buffer.
    pub fn is_flow_buffer_start(&self) -> bool {
        unsafe { self.block.as_ref().flags & 2 != 0 }
    }
    
    /// Checks if there was a buffer overflow.
    pub fn is_overflow(&self) -> bool {
        unsafe { self.block.as_ref().flags & 4 != 0 }
    }
    
    fn as_raw(&self) -> *const rist_data_block {
        self.block.as_ptr()
    }
}

impl Drop for DataBlock {
    fn drop(&mut self) {
        unsafe {
            let mut block = self.block.as_ptr();
            rist_receiver_data_block_free2(&mut block);
        }
    }
}

// Safety: DataBlock owns its data and doesn't share mutable state
unsafe impl Send for DataBlock {}
unsafe impl Sync for DataBlock {}
```

## Builder Pattern

### SenderBuilder

```rust
/// Builder for configuring a RIST sender.
#[derive(Default)]
pub struct SenderBuilder {
    profile: Profile,
    flow_id: Option<u32>,
    log_level: LogLevel,
    log_callback: Option<Box<dyn Fn(LogLevel, &str) + Send + Sync>>,
    stats_interval_ms: Option<u32>,
    stats_callback: Option<Box<dyn Fn(&SenderStats) + Send + Sync>>,
    connection_callback: Option<Box<dyn Fn(u32, ConnectionStatus) + Send + Sync>>,
}

impl SenderBuilder {
    /// Sets the RIST profile (Simple, Main, or Advanced).
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
    
    /// Sets the stats reporting interval and callback.
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
        let logging = self.create_logging_settings()?;
        let logging_ptr = logging.as_ref()
            .map(|l| l.as_raw())
            .unwrap_or(ptr::null_mut());
        
        // Create context
        let mut ctx: *mut rist_ctx = ptr::null_mut();
        let ret = unsafe {
            rist_sender_create(
                &mut ctx,
                self.profile.into(),
                flow_id,
                logging_ptr,
            )
        };
        check_result(ret)?;
        
        let sender = RistSender {
            ctx: NonNull::new(ctx).expect("context should not be null"),
            started: AtomicBool::new(false),
            peers: Mutex::new(Vec::new()),
            callbacks: Mutex::new(SenderCallbacks::default()),
            logging,
        };
        
        // Set up callbacks
        sender.setup_callbacks(self.stats_callback, self.connection_callback)?;
        
        Ok(sender)
    }
    
    fn create_logging_settings(&self) -> Result<Option<Box<LoggingSettings>>> {
        // Implementation for creating logging settings
        // ...
        Ok(None)
    }
}
```

### ReceiverBuilder

```rust
/// Builder for configuring a RIST receiver.
#[derive(Default)]
pub struct ReceiverBuilder {
    profile: Profile,
    log_level: LogLevel,
    log_callback: Option<Box<dyn Fn(LogLevel, &str) + Send + Sync>>,
    nack_type: NackType,
    fifo_size: Option<u32>,
    stats_interval_ms: Option<u32>,
    stats_callback: Option<Box<dyn Fn(&ReceiverStats) + Send + Sync>>,
    connection_callback: Option<Box<dyn Fn(u32, ConnectionStatus) + Send + Sync>>,
    data_callback: Option<Box<dyn Fn(DataBlock) + Send + Sync>>,
    auth_callback: Option<Box<dyn Fn(&str, u16, &str, u16) -> bool + Send + Sync>>,
}

impl ReceiverBuilder {
    /// Sets the RIST profile.
    pub fn profile(mut self, profile: Profile) -> Self {
        self.profile = profile;
        self
    }
    
    /// Sets the NACK type.
    pub fn nack_type(mut self, nack_type: NackType) -> Self {
        self.nack_type = nack_type;
        self
    }
    
    /// Sets the output FIFO size.
    pub fn fifo_size(mut self, size: u32) -> Self {
        self.fifo_size = Some(size);
        self
    }
    
    /// Sets a callback for received data (alternative to polling).
    pub fn on_data<F>(mut self, callback: F) -> Self
    where
        F: Fn(DataBlock) + Send + Sync + 'static,
    {
        self.data_callback = Some(Box::new(callback));
        self
    }
    
    /// Sets an authentication callback for validating connections.
    pub fn on_auth<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str, u16, &str, u16) -> bool + Send + Sync + 'static,
    {
        self.auth_callback = Some(Box::new(callback));
        self
    }
    
    /// Builds the receiver.
    pub fn build(self) -> Result<RistReceiver> {
        let logging = self.create_logging_settings()?;
        let logging_ptr = logging.as_ref()
            .map(|l| l.as_raw())
            .unwrap_or(ptr::null_mut());
        
        let mut ctx: *mut rist_ctx = ptr::null_mut();
        let ret = unsafe {
            rist_receiver_create(&mut ctx, self.profile.into(), logging_ptr)
        };
        check_result(ret)?;
        
        let receiver = RistReceiver {
            ctx: NonNull::new(ctx).expect("context should not be null"),
            started: AtomicBool::new(false),
            peers: Mutex::new(Vec::new()),
            callbacks: Mutex::new(ReceiverCallbacks::default()),
            logging,
        };
        
        // Apply settings
        if let Some(size) = self.fifo_size {
            receiver.set_output_fifo_size(size)?;
        }
        receiver.set_nack_type(self.nack_type)?;
        
        // Set up callbacks
        receiver.setup_callbacks(
            self.stats_callback,
            self.connection_callback,
            self.data_callback,
            self.auth_callback,
        )?;
        
        Ok(receiver)
    }
}
```

## Callback Handling

### Trampoline Pattern

```rust
/// Storage for callback closures
struct ReceiverCallbacks {
    data_callback: Option<Box<dyn Fn(DataBlock) + Send + Sync>>,
    stats_callback: Option<Box<dyn Fn(&ReceiverStats) + Send + Sync>>,
    connection_callback: Option<Box<dyn Fn(u32, ConnectionStatus) + Send + Sync>>,
    auth_callback: Option<Box<dyn Fn(&str, u16, &str, u16) -> bool + Send + Sync>>,
}

impl RistReceiver {
    fn setup_callbacks(
        &self,
        stats_callback: Option<Box<dyn Fn(&ReceiverStats) + Send + Sync>>,
        connection_callback: Option<Box<dyn Fn(u32, ConnectionStatus) + Send + Sync>>,
        data_callback: Option<Box<dyn Fn(DataBlock) + Send + Sync>>,
        auth_callback: Option<Box<dyn Fn(&str, u16, &str, u16) -> bool + Send + Sync>>,
    ) -> Result<()> {
        let mut callbacks = self.callbacks.lock();
        callbacks.data_callback = data_callback;
        callbacks.stats_callback = stats_callback;
        callbacks.connection_callback = connection_callback;
        callbacks.auth_callback = auth_callback;
        
        // Set up data callback if provided
        if callbacks.data_callback.is_some() {
            let ctx_ptr = self as *const Self as *mut c_void;
            let ret = unsafe {
                rist_receiver_data_callback_set2(
                    self.ctx.as_ptr(),
                    Some(data_callback_trampoline),
                    ctx_ptr,
                )
            };
            check_result(ret)?;
        }
        
        // Set up connection callback
        if callbacks.connection_callback.is_some() {
            let ctx_ptr = self as *const Self as *mut c_void;
            let ret = unsafe {
                rist_connection_status_callback_set(
                    self.ctx.as_ptr(),
                    Some(connection_callback_trampoline),
                    ctx_ptr,
                )
            };
            check_result(ret)?;
        }
        
        Ok(())
    }
}

/// C-compatible trampoline for data callback
unsafe extern "C" fn data_callback_trampoline(
    arg: *mut c_void,
    data_block: *mut rist_data_block,
) -> c_int {
    if arg.is_null() || data_block.is_null() {
        return 0;
    }
    
    // Catch panics to prevent unwinding across FFI boundary
    let result = std::panic::catch_unwind(|| {
        let receiver = &*(arg as *const RistReceiver);
        let callbacks = receiver.callbacks.lock();
        
        if let Some(ref callback) = callbacks.data_callback {
            // Clone the data block for the callback
            // The original will be freed by librist after we return
            let block = DataBlock::from_raw(data_block);
            callback(block);
            // Note: block is NOT dropped here to avoid double-free
            std::mem::forget(block);
        }
    });
    
    match result {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

/// C-compatible trampoline for connection status callback
unsafe extern "C" fn connection_callback_trampoline(
    arg: *mut c_void,
    peer: *mut rist_peer,
    status: rist_connection_status,
) {
    if arg.is_null() {
        return;
    }
    
    let _ = std::panic::catch_unwind(|| {
        let receiver = &*(arg as *const RistReceiver);
        let callbacks = receiver.callbacks.lock();
        
        if let Some(ref callback) = callbacks.connection_callback {
            let peer_id = if peer.is_null() { 0 } else { rist_peer_get_id(peer) };
            callback(peer_id, status.into());
        }
    });
}
```

## Enums and Types

```rust
/// RIST protocol profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// Simple profile - basic functionality
    Simple,
    /// Main profile - recommended for most uses
    #[default]
    Main,
    /// Advanced profile - full feature set
    Advanced,
}

impl From<Profile> for rist_profile {
    fn from(p: Profile) -> Self {
        match p {
            Profile::Simple => rist_profile::RIST_PROFILE_SIMPLE,
            Profile::Main => rist_profile::RIST_PROFILE_MAIN,
            Profile::Advanced => rist_profile::RIST_PROFILE_ADVANCED,
        }
    }
}

/// Log level for RIST operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogLevel {
    Disable,
    Error,
    Warn,
    Notice,
    #[default]
    Info,
    Debug,
    Simulate,
}

/// NACK type for packet recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NackType {
    /// Range-based NACKs
    #[default]
    Range,
    /// Bitmask-based NACKs
    Bitmask,
}

/// Connection status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Established,
    TimedOut,
    ClientConnected,
    ClientTimedOut,
}

/// Recovery mode for packet recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecoveryMode {
    Unconfigured,
    Disabled,
    #[default]
    Time,
}

/// Congestion control mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CongestionControl {
    Off,
    #[default]
    Normal,
    Aggressive,
}

/// Timing mode for data delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimingMode {
    #[default]
    Source,
    Arrival,
    Rtc,
}
```

## Statistics Types

```rust
/// Statistics for a sender peer.
#[derive(Debug, Clone)]
pub struct SenderStats {
    pub peer_id: u32,
    pub cname: String,
    pub bandwidth: u64,
    pub retry_bandwidth: u64,
    pub sent_packets: u64,
    pub received_packets: u64,
    pub retransmitted_packets: u64,
    pub quality: f64,
    pub rtt_ms: u32,
}

/// Statistics for a receiver flow.
#[derive(Debug, Clone)]
pub struct ReceiverStats {
    pub flow_id: u32,
    pub cname: String,
    pub bandwidth: u64,
    pub retry_bandwidth: u64,
    pub sent_packets: u64,
    pub received_packets: u64,
    pub missing_packets: u32,
    pub reordered_packets: u32,
    pub recovered_packets: u32,
    pub lost_packets: u32,
    pub quality: f64,
    pub rtt_ms: u32,
    pub peers: Vec<ReceiverPeerStats>,
}

/// Statistics for a receiver peer.
#[derive(Debug, Clone)]
pub struct ReceiverPeerStats {
    pub peer_id: u32,
    pub received_data: u64,
    pub received_rtcp: u32,
    pub sent_rtcp: u32,
    pub rtt_us: u64,
    pub avg_rtt_us: f64,
    pub bandwidth: u64,
    pub avg_bandwidth: u64,
}

impl From<&rist_stats_sender_peer> for SenderStats {
    fn from(raw: &rist_stats_sender_peer) -> Self {
        Self {
            peer_id: raw.peer_id,
            cname: unsafe { CStr::from_ptr(raw.cname.as_ptr()) }
                .to_string_lossy()
                .into_owned(),
            bandwidth: raw.bandwidth as u64,
            retry_bandwidth: raw.retry_bandwidth as u64,
            sent_packets: raw.sent,
            received_packets: raw.received,
            retransmitted_packets: raw.retransmitted,
            quality: raw.quality,
            rtt_ms: raw.rtt,
        }
    }
}
```

## Async Support (Optional Feature)

```rust
#[cfg(feature = "async-tokio")]
mod async_impl {
    use super::*;
    use tokio::sync::mpsc;
    
    impl RistReceiver {
        /// Asynchronously receives data.
        pub async fn recv_async(&self) -> Result<DataBlock> {
            let (tx, mut rx) = mpsc::channel(1);
            
            // Use a separate thread for the blocking call
            let ctx = self.ctx;
            tokio::task::spawn_blocking(move || {
                let mut block: *mut rist_data_block = ptr::null_mut();
                let ret = unsafe {
                    rist_receiver_data_read2(ctx.as_ptr(), &mut block, 1000)
                };
                
                if ret > 0 && !block.is_null() {
                    let _ = tx.blocking_send(DataBlock::from_raw(block));
                }
            });
            
            rx.recv().await.ok_or(Error::Timeout)
        }
    }
    
    impl RistSender {
        /// Asynchronously sends data.
        pub async fn send_async(&self, data: Vec<u8>) -> Result<usize> {
            let ctx = self.ctx;
            tokio::task::spawn_blocking(move || {
                // Implementation using blocking send
            }).await.map_err(|_| Error::Other("task failed".into()))?
        }
    }
}
```

## Error Type

```rust
use thiserror::Error;

/// Errors that can occur during RIST operations.
#[derive(Debug, Error)]
pub enum Error {
    #[error("Memory allocation failed")]
    Malloc,
    
    #[error("Null peer reference")]
    NullPeer,
    
    #[error("Invalid string length")]
    InvalidStringLength,
    
    #[error("Invalid profile")]
    InvalidProfile,
    
    #[error("Missing callback function")]
    MissingCallback,
    
    #[error("Null credentials")]
    NullCredentials,
    
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    
    #[error("Context not started")]
    NotStarted,
    
    #[error("Context already started")]
    AlreadyStarted,
    
    #[error("Operation timed out")]
    Timeout,
    
    #[error("librist error code: {0}")]
    Rist(i32),
    
    #[error("{0}")]
    Other(String),
}

/// Convenience Result type for RIST operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Converts a librist return code to a Result.
fn check_result(code: i32) -> Result<()> {
    match code {
        0 => Ok(()),
        RIST_ERR_MALLOC => Err(Error::Malloc),
        RIST_ERR_NULL_PEER => Err(Error::NullPeer),
        RIST_ERR_INVALID_STRING_LENGTH => Err(Error::InvalidStringLength),
        RIST_ERR_INVALID_PROFILE => Err(Error::InvalidProfile),
        RIST_ERR_MISSING_CALLBACK_FUNCTION => Err(Error::MissingCallback),
        RIST_ERR_NULL_CREDENTIALS => Err(Error::NullCredentials),
        code => Err(Error::Rist(code)),
    }
}
```
