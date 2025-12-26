# librist-sys FFI Design

## Overview

The `librist-sys` crate provides raw, unsafe Rust bindings to the librist C library. This document details the FFI layer design, including binding generation, build system integration, and platform-specific considerations.

## Binding Generation Strategy

### Approach: Hybrid (Bindgen + Manual)

Use bindgen for the bulk of bindings, with manual overrides for:
- Callback function types (for better ergonomics)
- Complex macros
- Platform-specific types

### Bindgen Configuration

```rust
// build.rs
fn generate_bindings(header_path: &Path, out_path: &Path) {
    let bindings = bindgen::Builder::default()
        .header(header_path.join("librist/librist.h").to_str().unwrap())
        
        // Include paths
        .clang_arg(format!("-I{}", header_path.display()))
        .clang_arg(format!("-I{}", header_path.join("common").display()))
        
        // Type handling
        .default_enum_style(bindgen::EnumVariation::Rust { non_exhaustive: true })
        .bitfield_enum("rist_data_block_.*_flags")
        
        // Derive traits
        .derive_debug(true)
        .derive_default(true)
        .derive_copy(true)
        .derive_eq(true)
        .derive_hash(true)
        .derive_partialeq(true)
        
        // Function allowlist
        .allowlist_function("rist_.*")
        .allowlist_function("librist_.*")
        .allowlist_function("udpsocket_.*")
        .allowlist_function("evsocket_.*")
        
        // Type allowlist
        .allowlist_type("rist_.*")
        .allowlist_type("librist_.*")
        .allowlist_type("udpsocket_.*")
        .allowlist_type("evsocket_.*")
        
        // Constant allowlist
        .allowlist_var("RIST_.*")
        .allowlist_var("LIBRIST_.*")
        
        // Blocklist deprecated functions
        .blocklist_function("rist_receiver_data_callback_set$")  // Use _set2
        .blocklist_function("rist_receiver_data_read$")          // Use _read2
        .blocklist_function("rist_parse_address$")               // Use _address2
        
        // Generate comments
        .generate_comments(true)
        .clang_arg("-fparse-all-comments")
        
        // Layout tests
        .layout_tests(true)
        
        .generate()
        .expect("Failed to generate bindings");
    
    bindings.write_to_file(out_path).expect("Failed to write bindings");
}
```

## Build System Integration

### Meson Build Process

```rust
// build.rs
fn build_librist(source_dir: &Path, build_dir: &Path) -> PathBuf {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    
    // Determine build options
    let static_lib = target_os == "macos" || cfg!(feature = "static");
    
    // Configure meson
    let mut meson_args = vec![
        "setup".to_string(),
        build_dir.to_string_lossy().to_string(),
        "--buildtype=release".to_string(),
        "-Dbuilt_tools=false".to_string(),
        "-Dtest=false".to_string(),
    ];
    
    if static_lib {
        meson_args.push("--default-library=static".to_string());
    }
    
    // Cross-compilation support
    if target_arch == "aarch64" && target_os == "linux" {
        meson_args.push("--cross-file=cross/aarch64-linux.txt".to_string());
    }
    
    // Run meson setup
    run_command("meson", &meson_args, source_dir);
    
    // Run meson compile
    run_command("meson", &["compile", "-C", build_dir.to_str().unwrap()], source_dir);
    
    build_dir.to_path_buf()
}
```

### Linking Configuration

```rust
// build.rs
fn configure_linking(build_dir: &Path) {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    
    // Add library search path
    println!("cargo:rustc-link-search=native={}", build_dir.display());
    
    match target_os.as_str() {
        "windows" => {
            println!("cargo:rustc-link-lib=dylib=librist");
            // Copy DLL for runtime
            copy_dll(build_dir);
        }
        "macos" => {
            println!("cargo:rustc-link-lib=static=rist");
            // Link system frameworks
            println!("cargo:rustc-link-lib=framework=Security");
        }
        _ => {
            // Linux/Unix
            if cfg!(feature = "static") {
                println!("cargo:rustc-link-lib=static=rist");
            } else {
                println!("cargo:rustc-link-lib=dylib=rist");
            }
        }
    }
    
    // mbedTLS dependencies
    if cfg!(feature = "mbedtls") {
        println!("cargo:rustc-link-lib=mbedtls");
        println!("cargo:rustc-link-lib=mbedcrypto");
        println!("cargo:rustc-link-lib=mbedx509");
    }
    
    // Rerun triggers
    println!("cargo:rerun-if-changed=librist/");
    println!("cargo:rerun-if-env-changed=LIBRIST_DIR");
}
```

## C Type Mappings

### Opaque Pointer Types

```rust
// src/lib.rs

/// Opaque RIST context type
#[repr(C)]
pub struct rist_ctx {
    _data: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}

/// Opaque RIST peer type
#[repr(C)]
pub struct rist_peer {
    _data: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}

/// Opaque reference type
#[repr(C)]
pub struct rist_ref {
    _data: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}
```

### Enums (Generated by Bindgen)

```rust
#[repr(u32)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
#[non_exhaustive]
pub enum rist_profile {
    RIST_PROFILE_SIMPLE = 0,
    RIST_PROFILE_MAIN = 1,
    RIST_PROFILE_ADVANCED = 2,
}

#[repr(i32)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
#[non_exhaustive]
pub enum rist_log_level {
    RIST_LOG_DISABLE = -1,
    RIST_LOG_ERROR = 3,
    RIST_LOG_WARN = 4,
    RIST_LOG_NOTICE = 5,
    RIST_LOG_INFO = 6,
    RIST_LOG_DEBUG = 7,
    RIST_LOG_SIMULATE = 100,
}
```

### Data Structures

```rust
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct rist_data_block {
    pub payload: *const c_void,
    pub payload_len: usize,
    pub ts_ntp: u64,
    pub virt_src_port: u16,
    pub virt_dst_port: u16,
    pub peer: *mut rist_peer,
    pub flow_id: u32,
    pub seq: u64,
    pub flags: u32,
    pub ref_: *mut rist_ref,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct rist_peer_config {
    pub version: c_int,
    pub address_family: c_int,
    pub initiate_conn: c_int,
    pub address: [c_char; RIST_MAX_STRING_LONG],
    pub miface: [c_char; RIST_MAX_STRING_SHORT],
    pub physical_port: u16,
    pub virt_dst_port: u16,
    pub recovery_mode: rist_recovery_mode,
    pub recovery_maxbitrate: u32,
    pub recovery_maxbitrate_return: u32,
    pub recovery_length_min: u32,
    pub recovery_length_max: u32,
    pub recovery_reorder_buffer: u32,
    pub recovery_rtt_min: u32,
    pub recovery_rtt_max: u32,
    pub weight: u32,
    pub secret: [c_char; RIST_MAX_STRING_SHORT],
    pub key_size: c_int,
    pub key_rotation: u32,
    pub compression: c_int,
    pub cname: [c_char; RIST_MAX_STRING_SHORT],
    pub congestion_control_mode: rist_congestion_control_mode,
    pub min_retries: u32,
    pub max_retries: u32,
    pub session_timeout: u32,
    pub keepalive_interval: u32,
    pub timing_mode: rist_timing_mode,
    pub srp_username: [c_char; RIST_MAX_STRING_LONG],
    pub srp_password: [c_char; RIST_MAX_STRING_LONG],
}
```

## Callback Function Types

### Manual Callback Definitions

```rust
/// Receiver data callback type
pub type receiver_data_callback2_t = Option<
    unsafe extern "C" fn(
        arg: *mut c_void,
        data_block: *mut rist_data_block,
    ) -> c_int
>;

/// Connection status callback type
pub type connection_status_callback_t = Option<
    unsafe extern "C" fn(
        arg: *mut c_void,
        peer: *mut rist_peer,
        status: rist_connection_status,
    )
>;

/// Stats callback type
pub type stats_callback_t = Option<
    unsafe extern "C" fn(
        arg: *mut c_void,
        stats: *const rist_stats,
    ) -> c_int
>;

/// Log callback type
pub type log_callback_t = Option<
    unsafe extern "C" fn(
        arg: *mut c_void,
        level: rist_log_level,
        msg: *const c_char,
    ) -> c_int
>;

/// Auth connect callback type
pub type auth_connect_callback_t = Option<
    unsafe extern "C" fn(
        arg: *mut c_void,
        conn_ip: *const c_char,
        conn_port: u16,
        local_ip: *const c_char,
        local_port: u16,
        peer: *mut rist_peer,
    ) -> c_int
>;

/// Auth disconnect callback type
pub type auth_disconnect_callback_t = Option<
    unsafe extern "C" fn(
        arg: *mut c_void,
        peer: *mut rist_peer,
    ) -> c_int
>;

/// Out-of-band callback type
pub type oob_callback_t = Option<
    unsafe extern "C" fn(
        arg: *mut c_void,
        oob_block: *const rist_oob_block,
    ) -> c_int
>;

/// Session timeout callback type
pub type session_timeout_callback_t = Option<
    unsafe extern "C" fn(
        arg: *mut c_void,
        flow_id: u32,
    ) -> c_int
>;
```

## Function Declarations

### Core API Functions

```rust
extern "C" {
    // Version
    pub fn librist_version() -> *const c_char;
    pub fn librist_api_version() -> *const c_char;
    
    // Context lifecycle
    pub fn rist_sender_create(
        ctx: *mut *mut rist_ctx,
        profile: rist_profile,
        flow_id: u32,
        logging_settings: *mut rist_logging_settings,
    ) -> c_int;
    
    pub fn rist_receiver_create(
        ctx: *mut *mut rist_ctx,
        profile: rist_profile,
        logging_settings: *mut rist_logging_settings,
    ) -> c_int;
    
    pub fn rist_start(ctx: *mut rist_ctx) -> c_int;
    pub fn rist_destroy(ctx: *mut rist_ctx) -> c_int;
    
    // Peer management
    pub fn rist_peer_config_defaults_set(
        peer_config: *mut rist_peer_config,
    ) -> c_int;
    
    pub fn rist_parse_address2(
        url: *const c_char,
        peer_config: *mut *mut rist_peer_config,
    ) -> c_int;
    
    pub fn rist_peer_config_free2(
        peer_config: *mut *mut rist_peer_config,
    ) -> c_int;
    
    pub fn rist_peer_create(
        ctx: *mut rist_ctx,
        peer: *mut *mut rist_peer,
        config: *const rist_peer_config,
    ) -> c_int;
    
    pub fn rist_peer_destroy(
        ctx: *mut rist_ctx,
        peer: *mut rist_peer,
    ) -> c_int;
    
    pub fn rist_peer_weight_set(
        ctx: *mut rist_ctx,
        peer: *mut rist_peer,
        weight: u32,
    ) -> c_int;
    
    pub fn rist_peer_get_id(peer: *const rist_peer) -> u32;
    
    // Sender operations
    pub fn rist_flow_id_create() -> u32;
    pub fn rist_sender_flow_id_get(ctx: *mut rist_ctx, flow_id: *mut u32) -> c_int;
    pub fn rist_sender_flow_id_set(ctx: *mut rist_ctx, flow_id: u32) -> c_int;
    pub fn rist_sender_data_write(
        ctx: *mut rist_ctx,
        data_block: *const rist_data_block,
    ) -> c_int;
    pub fn rist_sender_npd_enable(ctx: *mut rist_ctx) -> c_int;
    pub fn rist_sender_npd_disable(ctx: *mut rist_ctx) -> c_int;
    
    // Receiver operations
    pub fn rist_receiver_nack_type_set(
        ctx: *mut rist_ctx,
        nack_type: rist_nack_type,
    ) -> c_int;
    
    pub fn rist_receiver_set_output_fifo_size(
        ctx: *mut rist_ctx,
        size: u32,
    ) -> c_int;
    
    pub fn rist_receiver_data_read2(
        ctx: *mut rist_ctx,
        data_block: *mut *mut rist_data_block,
        timeout: c_int,
    ) -> c_int;
    
    pub fn rist_receiver_data_block_free2(
        block: *mut *mut rist_data_block,
    );
    
    pub fn rist_receiver_data_notify_fd_set(
        ctx: *mut rist_ctx,
        fd: c_int,
    ) -> c_int;
    
    // Callbacks
    pub fn rist_receiver_data_callback_set2(
        ctx: *mut rist_ctx,
        callback: receiver_data_callback2_t,
        arg: *mut c_void,
    ) -> c_int;
    
    pub fn rist_connection_status_callback_set(
        ctx: *mut rist_ctx,
        callback: connection_status_callback_t,
        arg: *mut c_void,
    ) -> c_int;
    
    pub fn rist_stats_callback_set(
        ctx: *mut rist_ctx,
        interval: c_int,
        callback: stats_callback_t,
        arg: *mut c_void,
    ) -> c_int;
    
    pub fn rist_auth_handler_set(
        ctx: *mut rist_ctx,
        connect_cb: auth_connect_callback_t,
        disconnect_cb: auth_disconnect_callback_t,
        arg: *mut c_void,
    ) -> c_int;
    
    pub fn rist_oob_callback_set(
        ctx: *mut rist_ctx,
        callback: oob_callback_t,
        arg: *mut c_void,
    ) -> c_int;
    
    // Logging
    pub fn rist_logging_set(
        logging_settings: *mut *mut rist_logging_settings,
        log_level: rist_log_level,
        log_cb: log_callback_t,
        cb_arg: *mut c_void,
        address: *mut c_char,
        logfp: *mut FILE,
    ) -> c_int;
    
    pub fn rist_logging_settings_free2(
        logging_settings: *mut *mut rist_logging_settings,
    ) -> c_int;
    
    pub fn rist_log(
        logging_settings: *mut rist_logging_settings,
        level: rist_log_level,
        format: *const c_char,
        ...
    );
    
    // Stats
    pub fn rist_stats_free(stats: *const rist_stats) -> c_int;
    
    // OOB
    pub fn rist_oob_write(
        ctx: *mut rist_ctx,
        oob_block: *const rist_oob_block,
    ) -> c_int;
    
    pub fn rist_oob_read(
        ctx: *mut rist_ctx,
        oob_block: *mut *const rist_oob_block,
    ) -> c_int;
    
    // Options
    pub fn rist_jitter_max_set(ctx: *mut rist_ctx, t: c_int) -> c_int;
}
```

## Constants

```rust
pub const RIST_MAX_STRING_SHORT: usize = 128;
pub const RIST_MAX_STRING_LONG: usize = 256;

pub const RIST_PEER_CONFIG_VERSION: c_int = 0;
pub const RIST_UDP_CONFIG_VERSION: c_int = 1;
pub const RIST_STATS_VERSION: u16 = 0;

// Default values
pub const RIST_DEFAULT_VIRT_SRC_PORT: u16 = 1971;
pub const RIST_DEFAULT_VIRT_DST_PORT: u16 = 1968;
pub const RIST_DEFAULT_RECOVERY_MAXBITRATE: u32 = 100000;
pub const RIST_DEFAULT_RECOVERY_MAXBITRATE_RETURN: u32 = 0;
pub const RIST_DEFAULT_RECOVERY_LENGTH_MIN: u32 = 1000;
pub const RIST_DEFAULT_RECOVERY_LENGTH_MAX: u32 = 1000;
pub const RIST_DEFAULT_RECOVERY_REORDER_BUFFER: u32 = 15;
pub const RIST_DEFAULT_RECOVERY_RTT_MIN: u32 = 5;
pub const RIST_DEFAULT_RECOVERY_RTT_MAX: u32 = 500;
pub const RIST_DEFAULT_MIN_RETRIES: u32 = 6;
pub const RIST_DEFAULT_MAX_RETRIES: u32 = 20;
pub const RIST_DEFAULT_SESSION_TIMEOUT: u32 = 2000;
pub const RIST_DEFAULT_KEEPALIVE_INTERVAL: u32 = 1000;

// Error codes
pub const RIST_ERR_MALLOC: c_int = -1;
pub const RIST_ERR_NULL_PEER: c_int = -2;
pub const RIST_ERR_INVALID_STRING_LENGTH: c_int = -3;
pub const RIST_ERR_INVALID_PROFILE: c_int = -4;
pub const RIST_ERR_MISSING_CALLBACK_FUNCTION: c_int = -5;
pub const RIST_ERR_NULL_CREDENTIALS: c_int = -6;
```

## Platform-Specific Handling

### Windows

```rust
#[cfg(target_os = "windows")]
pub type FILE = c_void;  // Opaque on Windows

#[cfg(target_os = "windows")]
extern "C" {
    pub fn _fileno(stream: *mut FILE) -> c_int;
}
```

### Unix/POSIX

```rust
#[cfg(not(target_os = "windows"))]
use libc::FILE;

#[cfg(not(target_os = "windows"))]
extern "C" {
    pub fn fileno(stream: *mut FILE) -> c_int;
}
```

## Safety Documentation

Each function should be documented with safety requirements:

```rust
/// Creates a RIST sender context.
///
/// # Safety
///
/// - `ctx` must be a valid pointer to a pointer that will receive the context
/// - `logging_settings` may be null for default logging, or must point to valid settings
/// - The returned context must be destroyed with `rist_destroy`
/// - Only one context should be created per flow_id
pub unsafe fn rist_sender_create(
    ctx: *mut *mut rist_ctx,
    profile: rist_profile,
    flow_id: u32,
    logging_settings: *mut rist_logging_settings,
) -> c_int;
```

## Testing

### Link Tests

```rust
#[test]
fn test_version() {
    unsafe {
        let version = librist_version();
        assert!(!version.is_null());
        let version_str = CStr::from_ptr(version).to_str().unwrap();
        assert!(!version_str.is_empty());
    }
}
```

### Layout Tests

Bindgen generates layout tests automatically. Keep them enabled to catch ABI mismatches.

```rust
#[test]
fn bindgen_test_layout_rist_peer_config() {
    assert_eq!(
        ::std::mem::size_of::<rist_peer_config>(),
        // Expected size
    );
    assert_eq!(
        ::std::mem::align_of::<rist_peer_config>(),
        // Expected alignment
    );
}
```
