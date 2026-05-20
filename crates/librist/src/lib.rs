//! Safe Rust bindings to the librist C library for RIST protocol.
//!
//! RIST (Reliable Internet Stream Transport) is a protocol for reliable
//! transport of video over lossy networks, designed for professional
//! broadcasting applications.
//!
//! # Features
//!
//! - **Safe abstractions** over the librist C API
//! - **Builder pattern** for easy configuration
//! - **Callback support** with Rust closures
//! - **Async support** (optional, via `async-tokio` feature)
//! - **Thread-safe** sender and receiver contexts
//! - **Out-of-band data** support for low-latency signaling
//!
//! # Quick Start
//!
//! ## Sender
//!
//! ```no_run
//! use librist::{RistSender, Profile, Result};
//!
//! fn main() -> Result<()> {
//!     // Create a sender
//!     let sender = RistSender::builder()
//!         .profile(Profile::Main)
//!         .build()?;
//!
//!     // Add a peer (destination)
//!     sender.add_peer("rist://192.168.1.100:5000")?;
//!
//!     // Start the sender
//!     sender.start()?;
//!
//!     // Send data
//!     let data = vec![0u8; 1316]; // MPEG-TS packet
//!     sender.send(&data)?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Receiver
//!
//! ```no_run
//! use librist::{RistReceiver, Profile, Result};
//!
//! fn main() -> Result<()> {
//!     // Create a receiver
//!     let receiver = RistReceiver::builder()
//!         .profile(Profile::Main)
//!         .build()?;
//!
//!     // Listen on a port (note the @ prefix for listener mode)
//!     receiver.add_peer("rist://@:5000")?;
//!
//!     // Start the receiver
//!     receiver.start()?;
//!
//!     // Receive data
//!     loop {
//!         match receiver.recv(1000) {
//!             Ok(block) => {
//!                 println!("Received {} bytes", block.payload().len());
//!             }
//!             Err(librist::Error::Timeout) => continue,
//!             Err(e) => return Err(e),
//!         }
//!     }
//! }
//! ```
//!
//! # URL Format
//!
//! RIST URLs follow the format:
//!
//! ```text
//! rist://[user:pass@]host:port[?options]
//! ```
//!
//! For listener mode (receiver), prefix with `@`:
//!
//! ```text
//! rist://@:5000
//! ```
//!
//! Common URL options:
//!
//! - `bandwidth=<kbps>` - Maximum recovery bandwidth
//! - `buffer=<ms>` - Buffer size in milliseconds
//! - `secret=<key>` - Encryption secret
//! - `aes-type=<128|256>` - AES key size
//! - `weight=<n>` - Bonding weight (0 = duplicate to all)
//! - `cname=<name>` - RTCP canonical name

mod callbacks;
mod data;
mod error;
mod logging;
mod oob;
mod peer;
mod receiver;
mod sender;
mod stats;
mod tunnel;
mod types;

#[cfg(feature = "async-tokio")]
mod async_context;

#[cfg(feature = "srp")]
mod srp;

// Re-export public API
pub use data::{DataBlock, DataBlockBuilder};
pub use error::{Error, Result};
pub use logging::{LogLevel, LoggingSettings};
pub use oob::{MAX_OOB_PAYLOAD_SIZE, OobBlock, OobBlockBuilder};
pub use peer::{PeerConfig, PeerHandle};
pub use receiver::{ReceiverBuilder, RistReceiver};
pub use sender::{RistSender, SenderBuilder};
pub use stats::{ReceiverPeerStats, ReceiverStats, SenderStats};
pub use tunnel::{DataFdFlags, DataFdStats, Tun};
pub use types::*;

#[cfg(feature = "srp")]
pub use srp::{SrpCredentials, SrpVerifier};

// Async exports
#[cfg(feature = "async-tokio")]
pub use async_context::{AsyncRistReceiver, AsyncRistSender};

/// Returns the librist library version string.
pub fn version() -> &'static str {
    unsafe {
        let ptr = librist_sys::librist_version();
        std::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("unknown")
    }
}

/// Returns the librist API version string.
pub fn api_version() -> &'static str {
    unsafe {
        let ptr = librist_sys::librist_api_version();
        std::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("unknown")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        let v = version();
        assert!(!v.is_empty());
        println!("librist version: {}", v);
    }

    #[test]
    fn test_api_version() {
        let v = api_version();
        assert!(!v.is_empty());
        println!("librist API version: {}", v);
    }
}
