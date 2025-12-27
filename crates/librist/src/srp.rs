//! SRP (Secure Remote Password) authentication support for RIST.
//!
//! SRP provides secure authentication for RIST Main profile connections without
//! transmitting passwords over the network.
//!
//! # Client Mode
//!
//! For clients connecting to an SRP-enabled server, use [`SrpCredentials`]:
//!
//! ```no_run
//! use librist::{RistSender, Profile, SrpCredentials};
//!
//! let sender = RistSender::builder()
//!     .profile(Profile::Main)
//!     .build()?;
//!
//! let peer = sender.add_peer("rist://server.example.com:5000")?;
//! peer.enable_srp_auth(SrpCredentials::new("username", "password"))?;
//!
//! sender.start()?;
//! # Ok::<(), librist::Error>(())
//! ```
//!
//! # Server Mode
//!
//! For servers that need to verify client credentials, implement a verifier lookup:
//!
//! ```no_run
//! use librist::{RistReceiver, Profile, SrpVerifier};
//! use std::collections::HashMap;
//!
//! // Pre-computed verifier database (use ristsrppasswd tool to generate)
//! let mut users: HashMap<String, SrpVerifier> = HashMap::new();
//! users.insert("alice".to_string(), SrpVerifier {
//!     salt: vec![/* salt bytes */],
//!     verifier: vec![/* verifier bytes */],
//!     ..Default::default()
//! });
//!
//! let receiver = RistReceiver::builder()
//!     .profile(Profile::Main)
//!     .build()?;
//!
//! let peer = receiver.add_peer("rist://@:5000")?;
//! peer.enable_srp_verifier(move |username| {
//!     users.get(username).cloned()
//! })?;
//!
//! receiver.start()?;
//! # Ok::<(), librist::Error>(())
//! ```

use crate::error::{check_result, Error, Result};
use crate::peer::PeerHandle;
use librist_sys::libc;
use std::ffi::{c_void, CString};
use std::sync::Arc;

/// SRP client credentials (username and password).
///
/// Used by clients to authenticate with an SRP-enabled server.
#[derive(Debug, Clone)]
pub struct SrpCredentials {
    username: String,
    password: String,
}

impl SrpCredentials {
    /// Creates new SRP credentials.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }

    /// Returns the username.
    pub fn username(&self) -> &str {
        &self.username
    }
}

/// SRP verifier data for server-side authentication.
///
/// Contains the pre-computed verifier and salt for a user.
/// Generate these using the `ristsrppasswd` tool from librist.
#[derive(Debug, Clone, Default)]
pub struct SrpVerifier {
    /// The verifier bytes (pre-computed from password).
    pub verifier: Vec<u8>,
    /// The salt bytes.
    pub salt: Vec<u8>,
    /// Use default 2048-bit N modulus and generator.
    /// If false, custom values must be provided.
    pub use_default_ng: bool,
    /// Custom N modulus in hex (optional, only if use_default_ng is false).
    pub n_modulus_hex: Option<String>,
    /// Custom generator in hex (optional, only if use_default_ng is false).
    pub generator_hex: Option<String>,
    /// Generation number for cache invalidation (0 = always re-auth).
    pub generation: u64,
}

impl SrpVerifier {
    /// Creates a new verifier with default N/g parameters.
    pub fn new(verifier: Vec<u8>, salt: Vec<u8>) -> Self {
        Self {
            verifier,
            salt,
            use_default_ng: true,
            n_modulus_hex: None,
            generator_hex: None,
            generation: 0,
        }
    }

    /// Sets a non-zero generation number for cache control.
    ///
    /// When set, librist will periodically check if the verifier has changed
    /// and re-authenticate clients if needed.
    pub fn with_generation(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }
}

/// Type alias for the verifier lookup callback.
///
/// The callback receives a username and should return the corresponding
/// [`SrpVerifier`] if the user exists, or `None` if not found.
pub type SrpVerifierLookup = dyn Fn(&str) -> Option<SrpVerifier> + Send + Sync + 'static;

// Storage for the verifier callback - must be kept alive for the duration of the connection
struct SrpCallbackData {
    lookup: Box<SrpVerifierLookup>,
}

/// Trampoline function for the C callback.
unsafe extern "C" fn srp_verifier_trampoline(
    username: *mut std::ffi::c_char,
    lookup_data: *mut librist_sys::librist_verifier_lookup_data_t,
    hashversion: *mut std::ffi::c_int,
    generation: *mut u64,
    user_data: *mut c_void,
) {
    if username.is_null() || user_data.is_null() {
        return;
    }

    let callback_data = unsafe { &*(user_data as *const SrpCallbackData) };

    // Convert username to Rust string
    let username_str = match unsafe { std::ffi::CStr::from_ptr(username) }.to_str() {
        Ok(s) => s,
        Err(_) => return,
    };

    // If lookup_data is null, librist is just checking the generation
    if lookup_data.is_null() {
        // Just a generation check - we could update generation here
        return;
    }

    // Call the Rust lookup function
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (callback_data.lookup)(username_str)
    }));

    let verifier = match result {
        Ok(Some(v)) => v,
        Ok(None) => return, // User not found
        Err(_) => return,   // Panic in callback
    };

    // Fill in the lookup data
    // Note: librist takes ownership of heap-allocated data
    let lookup = unsafe { &mut *lookup_data };

    // Allocate and copy verifier
    if !verifier.verifier.is_empty() {
        let verifier_ptr = unsafe { libc::malloc(verifier.verifier.len()) } as *mut u8;
        if !verifier_ptr.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    verifier.verifier.as_ptr(),
                    verifier_ptr,
                    verifier.verifier.len(),
                );
            }
            lookup.verifier = verifier_ptr;
            lookup.verifier_len = verifier.verifier.len();
        }
    }

    // Allocate and copy salt
    if !verifier.salt.is_empty() {
        let salt_ptr = unsafe { libc::malloc(verifier.salt.len()) } as *mut u8;
        if !salt_ptr.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    verifier.salt.as_ptr(),
                    salt_ptr,
                    verifier.salt.len(),
                );
            }
            lookup.salt = salt_ptr;
            lookup.salt_len = verifier.salt.len();
        }
    }

    lookup.default_ng = verifier.use_default_ng;

    // Set custom N/g if provided
    if let Some(ref n_hex) = verifier.n_modulus_hex {
        if let Ok(cstr) = CString::new(n_hex.as_str()) {
            let len = cstr.as_bytes_with_nul().len();
            let ptr = unsafe { libc::malloc(len) } as *mut std::ffi::c_char;
            if !ptr.is_null() {
                unsafe {
                    std::ptr::copy_nonoverlapping(cstr.as_ptr(), ptr, len);
                }
                lookup.n_modulus_ascii = ptr;
            }
        }
    }

    if let Some(ref g_hex) = verifier.generator_hex {
        if let Ok(cstr) = CString::new(g_hex.as_str()) {
            let len = cstr.as_bytes_with_nul().len();
            let ptr = unsafe { libc::malloc(len) } as *mut std::ffi::c_char;
            if !ptr.is_null() {
                unsafe {
                    std::ptr::copy_nonoverlapping(cstr.as_ptr(), ptr, len);
                }
                lookup.generator_ascii = ptr;
            }
        }
    }

    // Set hashversion (use latest)
    if !hashversion.is_null() {
        // Keep the incoming hashversion as the max supported
    }

    // Set generation
    if !generation.is_null() {
        unsafe { *generation = verifier.generation };
    }
}

impl PeerHandle {
    /// Enables SRP authentication for this peer (client mode).
    ///
    /// Call this after adding the peer but before starting the context.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use librist::{RistSender, Profile, SrpCredentials};
    ///
    /// let sender = RistSender::builder()
    ///     .profile(Profile::Main)
    ///     .build()?;
    ///
    /// let peer = sender.add_peer("rist://server:5000")?;
    /// peer.enable_srp_auth(SrpCredentials::new("user", "pass"))?;
    ///
    /// sender.start()?;
    /// # Ok::<(), librist::Error>(())
    /// ```
    pub fn enable_srp_auth(&self, credentials: SrpCredentials) -> Result<()> {
        let username =
            CString::new(credentials.username).map_err(|_| Error::InvalidStringLength)?;
        let password =
            CString::new(credentials.password).map_err(|_| Error::InvalidStringLength)?;

        let ret = unsafe {
            librist_sys::rist_enable_eap_srp_2(
                self.as_raw(),
                username.as_ptr(),
                password.as_ptr(),
                None,
                std::ptr::null_mut(),
            )
        };
        check_result(ret)
    }

    /// Enables SRP authentication for this peer (server mode) with a verifier lookup.
    ///
    /// The lookup function is called when a client attempts to authenticate.
    /// It should return the [`SrpVerifier`] for the username if valid.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use librist::{RistReceiver, Profile, SrpVerifier};
    ///
    /// let receiver = RistReceiver::builder()
    ///     .profile(Profile::Main)
    ///     .build()?;
    ///
    /// let peer = receiver.add_peer("rist://@:5000")?;
    /// peer.enable_srp_verifier(|username| {
    ///     if username == "admin" {
    ///         Some(SrpVerifier::new(
    ///             vec![/* verifier bytes */],
    ///             vec![/* salt bytes */],
    ///         ))
    ///     } else {
    ///         None
    ///     }
    /// })?;
    ///
    /// receiver.start()?;
    /// # Ok::<(), librist::Error>(())
    /// ```
    pub fn enable_srp_verifier<F>(&self, lookup: F) -> Result<Arc<()>>
    where
        F: Fn(&str) -> Option<SrpVerifier> + Send + Sync + 'static,
    {
        let callback_data = Box::new(SrpCallbackData {
            lookup: Box::new(lookup),
        });

        // Leak the callback data - it needs to live for the duration of the connection
        // We return an Arc that the caller should keep alive
        let callback_ptr = Box::into_raw(callback_data) as *mut c_void;

        let ret = unsafe {
            librist_sys::rist_enable_eap_srp_2(
                self.as_raw(),
                std::ptr::null(),
                std::ptr::null(),
                Some(srp_verifier_trampoline),
                callback_ptr,
            )
        };

        if ret != 0 {
            // Clean up on failure
            unsafe {
                let _ = Box::from_raw(callback_ptr as *mut SrpCallbackData);
            }
            return check_result(ret).map(|_| Arc::new(()));
        }

        // Return an Arc that keeps track of the callback
        // Note: In practice, librist manages the lifetime, but we provide this
        // for the caller to hold onto if they want
        Ok(Arc::new(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_srp_credentials() {
        let creds = SrpCredentials::new("alice", "secret123");
        assert_eq!(creds.username(), "alice");
    }

    #[test]
    fn test_srp_verifier() {
        let verifier = SrpVerifier::new(vec![1, 2, 3], vec![4, 5, 6]);
        assert_eq!(verifier.verifier, vec![1, 2, 3]);
        assert_eq!(verifier.salt, vec![4, 5, 6]);
        assert!(verifier.use_default_ng);
        assert_eq!(verifier.generation, 0);

        let verifier = verifier.with_generation(42);
        assert_eq!(verifier.generation, 42);
    }
}
