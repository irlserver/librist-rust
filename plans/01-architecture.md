# librist-rust Architecture

## Overview

This document outlines the architecture for a production-ready Rust wrapper around the librist C library, providing safe, idiomatic Rust APIs for the RIST (Reliable Internet Stream Transport) protocol.

## Workspace Structure

```
librist-rust/
├── Cargo.toml                    # Workspace root
├── .cargo/
│   └── config.toml               # Build configuration, rpath settings
├── .github/
│   └── workflows/
│       └── ci.yml                # CI/CD pipeline
├── crates/
│   ├── librist-sys/              # Raw FFI bindings (-sys crate)
│   │   ├── Cargo.toml
│   │   ├── build.rs              # Meson build integration
│   │   ├── src/
│   │   │   └── lib.rs            # Bindgen-generated bindings
│   │   └── librist/              # Git submodule of librist
│   │
│   └── librist/                  # Safe Rust wrapper
│       ├── Cargo.toml
│       ├── src/
│       │   ├── lib.rs            # Public API exports
│       │   ├── error.rs          # Error types
│       │   ├── context.rs        # Sender/Receiver contexts
│       │   ├── peer.rs           # Peer management
│       │   ├── config.rs         # Configuration types
│       │   ├── data.rs           # Data block handling
│       │   ├── stats.rs          # Statistics types
│       │   ├── logging.rs        # Logging integration
│       │   ├── oob.rs            # Out-of-band data
│       │   └── callbacks.rs      # Callback handling
│       ├── examples/
│       │   ├── sender.rs
│       │   └── receiver.rs
│       └── tests/
│           └── integration.rs
│
├── plans/                        # Design documents
├── scripts/                      # Build/utility scripts
└── README.md
```

## Crate Responsibilities

### librist-sys (FFI Layer)

**Purpose:** Provide raw, unsafe Rust bindings to librist C API.

**Responsibilities:**
- Generate FFI bindings using bindgen
- Compile librist from source using Meson (optional, can use system library)
- Handle platform-specific linking (static/dynamic)
- Export raw C types and functions

**Features:**
- `bundled` - Build librist from included source (default)
- `system` - Use system-installed librist
- `static` - Static linking (macOS default)
- `mbedtls` - Enable mbedTLS for encryption
- `srp` - Enable SRP authentication support

### librist (Safe Wrapper)

**Purpose:** Provide safe, idiomatic Rust API for RIST protocol.

**Responsibilities:**
- Safe abstractions over FFI
- Memory management with RAII
- Thread-safe callback handling
- Error handling with Result types
- Async support (optional feature)

**Features:**
- `async-tokio` - Async support via Tokio runtime
- `async-std` - Async support via async-std runtime
- `serde` - Serialization support for config/stats

## Platform Support Matrix

| Platform | Architecture | Build Method | Linking |
|----------|-------------|--------------|---------|
| Linux | x86_64 | Meson | Dynamic |
| Linux | aarch64 | Meson | Dynamic |
| macOS | x86_64 | Meson | Static |
| macOS | aarch64 | Meson | Static |
| Windows | x86_64 | Meson | Dynamic (.dll) |
| FreeBSD | x86_64 | Meson | Dynamic |

## Design Principles

### 1. Safety First
- All public APIs are safe Rust
- Unsafe code isolated to FFI layer
- Proper lifetime management for callbacks
- No undefined behavior

### 2. Zero-Cost Abstractions
- Minimal runtime overhead
- No unnecessary allocations
- Inline where appropriate

### 3. Ergonomic API
- Builder pattern for configuration
- Type-safe enums for options
- Meaningful error types
- Comprehensive documentation

### 4. Flexibility
- Support both sync and async usage
- Callback-based and polling APIs
- Configurable at compile-time via features

## Memory Model

### Ownership Rules

1. **Context Ownership:**
   - `RistSender` and `RistReceiver` own their `rist_ctx`
   - Destroyed on Drop

2. **Peer Ownership:**
   - Peers are owned by their parent Context
   - `PeerHandle` provides safe reference to peer
   - Peer destroyed when Context is dropped or explicitly removed

3. **Data Block Ownership:**
   - Received data blocks are reference-counted in librist
   - Wrapped in `DataBlock` with proper Drop implementation
   - Copy-on-write semantics for efficient data handling

4. **Callback Lifetime:**
   - Callbacks stored in Context with appropriate lifetime
   - Use `Arc<Mutex<>>` or channels for safe callback data
   - Trampoline functions for C callback compatibility

## Thread Safety

### Thread Model

librist internally uses multiple threads:
- Main thread for context management
- Worker threads for packet processing
- Callback threads for event notifications

### Rust Wrapper Guarantees

1. **Send + Sync:**
   - `RistSender` and `RistReceiver` are `Send + Sync`
   - Safe to share between threads with `Arc`

2. **Callback Thread Safety:**
   - Callbacks execute on librist threads
   - User callback closures must be `Send + Sync`
   - Internal synchronization provided

3. **Data Access:**
   - Stats accessed via atomic operations
   - Configuration changes synchronized

## Error Handling Strategy

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
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
    
    #[error("librist error: {0}")]
    Rist(i32),
    
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    
    #[error("Peer connection failed")]
    PeerConnectionFailed,
    
    #[error("Context not started")]
    NotStarted,
    
    #[error("Already started")]
    AlreadyStarted,
}
```

### Error Mapping

librist error codes are mapped to specific enum variants where possible, with a fallback to `Error::Rist(code)` for unknown codes.

## API Design

### Builder Pattern for Configuration

```rust
// Sender configuration
let sender = RistSender::builder()
    .profile(RistProfile::Main)
    .flow_id(0x12345678)
    .log_level(LogLevel::Info)
    .on_stats(|stats| { /* handle stats */ })
    .on_peer_connected(|peer_id| { /* handle connection */ })
    .build()?;

// Add peers via URL
let peer = sender.add_peer("rist://server:5000?bandwidth=10000")?;

// Start streaming
sender.start()?;

// Send data
sender.send(&data)?;
```

### Type-Safe Enums

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Profile {
    Simple = 0,
    Main = 1,
    Advanced = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RecoveryMode {
    Unconfigured = 0,
    Disabled = 1,
    Time = 2,
}
```

### Async Support

```rust
#[cfg(feature = "async-tokio")]
impl RistReceiver {
    pub async fn recv(&self) -> Result<DataBlock, Error> {
        // Async receive with proper wakeup
    }
}
```

## Integration Points

### Logging

Integrate with Rust logging ecosystem:
- `log` crate for facade
- `tracing` support via feature flag
- Custom callback for advanced use

### Metrics

Export metrics in standard formats:
- JSON (built-in from librist)
- Prometheus (optional feature)
- Custom callback for integration

## Testing Strategy

### Unit Tests
- Test configuration builders
- Test error handling
- Test type conversions

### Integration Tests
- Sender/receiver communication
- Multiple peer scenarios
- Reconnection handling
- Stats collection

### CI/CD Tests
- Cross-platform builds
- Clippy linting
- Security audits
- Documentation tests

## Future Considerations

1. **WebAssembly Support:**
   - Potential for browser-based RIST clients
   - Requires WebRTC data channel transport

2. **No-std Support:**
   - Embedded systems use case
   - Would require significant refactoring

3. **FFI for Other Languages:**
   - C header generation via cbindgen
   - Python bindings via PyO3

4. **Metrics Exporter:**
   - Prometheus metrics endpoint
   - OpenTelemetry integration
