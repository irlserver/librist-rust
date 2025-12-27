//! Raw FFI bindings to the librist C library.
//!
//! This crate provides low-level, unsafe bindings to the librist C library.
//! For a safe, idiomatic Rust API, use the `librist` crate instead.
//!
//! # Building
//!
//! By default, this crate will build librist from source using Meson.
//! Alternatively, you can use a system-installed librist:
//!
//! ```toml
//! [dependencies]
//! librist-sys = { version = "0.1", default-features = false, features = ["system"] }
//! ```
//!
//! # Safety
//!
//! All functions in this crate are unsafe. Consult the librist documentation
//! for correct usage.

#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::all)]

// Include the generated bindings
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

// Re-export libc for users who need it
pub use libc;

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn test_version() {
        unsafe {
            let version = librist_version();
            assert!(!version.is_null());
            let version_str = CStr::from_ptr(version).to_str().unwrap();
            assert!(!version_str.is_empty());
            println!("librist version: {}", version_str);
        }
    }

    #[test]
    fn test_api_version() {
        unsafe {
            let version = librist_api_version();
            assert!(!version.is_null());
            let version_str = CStr::from_ptr(version).to_str().unwrap();
            assert!(!version_str.is_empty());
            println!("librist API version: {}", version_str);
        }
    }

    #[test]
    fn test_flow_id_create() {
        unsafe {
            let flow_id = rist_flow_id_create();
            // Flow ID should be non-zero (random)
            println!("Generated flow ID: {:#x}", flow_id);
        }
    }

    #[test]
    fn test_peer_config_defaults() {
        unsafe {
            let mut config: rist_peer_config = std::mem::zeroed();
            let result = rist_peer_config_defaults_set(&mut config);
            assert_eq!(result, 0);

            // Check some default values
            assert_eq!(
                config.recovery_mode,
                rist_recovery_mode::RIST_RECOVERY_MODE_TIME
            );
            assert_eq!(config.recovery_maxbitrate, RIST_DEFAULT_RECOVERY_MAXBITRATE);
            assert_eq!(config.recovery_length_min, RIST_DEFAULT_RECOVERY_LENGTH_MIN);
            assert_eq!(config.recovery_length_max, RIST_DEFAULT_RECOVERY_LENGTH_MAX);
        }
    }
}
