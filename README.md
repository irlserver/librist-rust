# librist-rust

Safe Rust bindings for the [librist](https://code.videolan.org/rist/librist) library, implementing the RIST (Reliable Internet Stream Transport) protocol for professional video streaming.

## Features

- **Safe abstractions** over the librist C API
- **Builder pattern** for easy configuration
- **Callback support** with Rust closures
- **Async support** (optional, via `async-tokio` feature)
- **Thread-safe** sender and receiver contexts
- **Cross-platform** - Linux, macOS, Windows

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
librist = "0.1"
```

### Build Requirements

- Rust 1.85+
- Meson and Ninja (for building librist from source)
- C compiler (gcc, clang, or MSVC)

On Ubuntu/Debian:
```bash
sudo apt-get install meson ninja-build pkg-config
```

On macOS:
```bash
brew install meson ninja pkg-config
```

## Quick Start

### Sender

```rust
use librist::{RistSender, Profile};

fn main() -> librist::Result<()> {
    // Create a sender
    let sender = RistSender::builder()
        .profile(Profile::Main)
        .build()?;

    // Add destination peer
    sender.add_peer("rist://192.168.1.100:5000")?;
    
    // Start the sender
    sender.start()?;

    // Send data
    let data = vec![0u8; 1316]; // MPEG-TS packet
    sender.send(&data)?;
    
    Ok(())
}
```

### Receiver

```rust
use librist::{RistReceiver, Profile};

fn main() -> librist::Result<()> {
    // Create a receiver
    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .build()?;

    // Listen on port 5000 (note the @ prefix)
    receiver.add_peer("rist://@:5000")?;
    
    // Start the receiver
    receiver.start()?;

    // Receive data
    loop {
        match receiver.recv(1000) {
            Ok(block) => {
                println!("Received {} bytes", block.payload().len());
            }
            Err(librist::Error::Timeout) => continue,
            Err(e) => return Err(e),
        }
    }
}
```

## URL Format

RIST URLs follow the format:

```
rist://[user:pass@]host:port[?options]
```

For listener mode (receiver), prefix with `@`:

```
rist://@:5000
```

### Common URL Options

| Option | Description |
|--------|-------------|
| `bandwidth=<kbps>` | Maximum recovery bandwidth |
| `buffer=<ms>` | Buffer size in milliseconds |
| `secret=<key>` | Encryption secret |
| `aes-type=<128\|256>` | AES key size |
| `weight=<n>` | Bonding weight (0 = duplicate) |
| `cname=<name>` | RTCP canonical name |

## Features

### Optional Features

- `async-tokio` - Async support via Tokio runtime
- `serde` - Serialization support for config/stats
- `tracing` - Integration with the tracing ecosystem

### librist-sys Features

- `bundled` (default) - Build librist from source
- `system` - Use system-installed librist
- `static` - Static linking
- `mbedtls` - Enable mbedTLS encryption

## Examples

Run the example sender:
```bash
cargo run --example sender -- rist://127.0.0.1:5000
```

Run the example receiver:
```bash
cargo run --example receiver -- rist://@:5000
```

## Architecture

The library is split into two crates:

- **librist-sys** - Raw FFI bindings generated with bindgen
- **librist** - Safe, idiomatic Rust wrapper

See the [plans/](plans/) directory for detailed design documents.

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please see the design documents in `plans/` for architecture details.

## Acknowledgments

- [librist](https://code.videolan.org/rist/librist) - The underlying C library
- [Video Services Forum](https://www.videoservicesforum.org/) - RIST protocol specification
