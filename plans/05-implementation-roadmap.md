# Implementation Roadmap

## Current Status (December 2024)

**Overall Progress: ~98% complete**

| Phase | Status | Notes |
|-------|--------|-------|
| 1. Foundation | **Complete** | FFI bindings, CI, build system |
| 2. Core Wrapper | **Complete** | Sender/Receiver with builders |
| 3. Data Flow | **Complete** | DataBlock, send/recv, NPD, NACK |
| 4. Callbacks | **Complete** | Stats, connection, data, auth, OOB, logging |
| 5. Advanced | **Complete** | Stats, OOB, encryption, bonding, SRP |
| 6. Async | **Complete** | Tokio integration |
| 7. Polish | **Partial** | Needs Windows CI, more tests |

**Test Suite:** 53 tests passing (34 librist unit, 19 integration, 4 librist-sys)

**Examples:** sender, receiver, bonding_sender, stats_monitor, async_receiver, debug_recv

---

## Remaining Work

### High Priority
- [ ] Windows CI testing
- [ ] Integration tests with real network
- [ ] crates.io publish preparation

### Medium Priority
- [ ] Benchmark suite
- [ ] Test coverage improvements

---

## Completed Features

### Core
- RistSender / RistReceiver with builder pattern
- Profile, LogLevel, RecoveryMode, ConnectionStatus enums
- PeerHandle, PeerConfig, URL parsing
- Error handling with Result type

### Data Flow
- DataBlock with metadata (timestamps, ports, flags)
- send(), send_to_port(), send_block()
- recv(), try_recv()
- NPD (Null Packet Deletion): enable_npd(), disable_npd()
- NACK type configuration: set_nack_type()

### Callbacks
- Stats callbacks (sender + receiver)
- Connection status callbacks
- Data callback (receiver)
- Auth callbacks (connect + disconnect)
- OOB callbacks (sender + receiver)
- Log callback with `log` and `tracing` crate integration

### Advanced
- OOB send/receive: send_oob(), send_oob_to_peer(), send_oob_block(), on_oob()
- OOB peer targeting for directed messaging
- Encryption via URL parameters
- Bonding via weight configuration
- Statistics: SenderStats, ReceiverStats
- SRP authentication: SrpCredentials, SrpVerifier, enable_srp_auth(), enable_srp_verifier()

### Async (feature: async-tokio)
- AsyncRistSender: send(), send_to_port(), send_bulk()
- AsyncRistReceiver: recv(), recv_timeout(), try_recv()
- Stream trait for use with StreamExt

---

## Quality Gates

Before v1.0 release:
1. All tests pass on Linux, macOS, Windows
2. No Clippy warnings
3. Documentation for all public items
4. Security audit clean (cargo audit)
5. Real-world testing with RIST streams

## Dependencies

**Required:** Rust 1.85+, Meson 0.54+, Ninja, C compiler

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `bundled` | Yes | Build librist from source |
| `mbedtls` | Yes | AES encryption support |
| `srp` | No | SRP authentication (enables mbedtls) |
| `async-tokio` | No | Async support via Tokio |
| `tracing` | No | Tracing crate integration |
| `serde` | No | Serialization support |
