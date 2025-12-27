//! Error types for librist operations.

use thiserror::Error;

/// Errors that can occur during RIST operations.
#[derive(Debug, Error)]
pub enum Error {
    /// Memory allocation failed in librist.
    #[error("Memory allocation failed")]
    Malloc,

    /// A null peer reference was passed.
    #[error("Null peer reference")]
    NullPeer,

    /// Invalid string length (too long or invalid encoding).
    #[error("Invalid string length")]
    InvalidStringLength,

    /// Invalid RIST profile specified.
    #[error("Invalid profile")]
    InvalidProfile,

    /// A required callback function was not set.
    #[error("Missing callback function")]
    MissingCallback,

    /// Null credentials were passed where required.
    #[error("Null credentials")]
    NullCredentials,

    /// Invalid URL format.
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// The context has not been started yet.
    #[error("Context not started")]
    NotStarted,

    /// The context has already been started.
    #[error("Context already started")]
    AlreadyStarted,

    /// Operation timed out.
    #[error("Operation timed out")]
    Timeout,

    /// Peer creation failed.
    #[error("Failed to create peer")]
    PeerCreationFailed,

    /// URL parsing failed.
    #[error("Failed to parse URL: {0}")]
    UrlParseFailed(String),

    /// Context creation failed.
    #[error("Failed to create context")]
    ContextCreationFailed,

    /// Out-of-band data is not enabled.
    #[error("OOB not enabled - call enable_oob() or on_oob() first")]
    OobNotEnabled,

    /// Operation not supported with this RIST profile.
    #[error("Operation not supported with this profile")]
    ProfileNotSupported,

    /// Peer not found (invalid peer ID).
    #[error("Peer not found")]
    PeerNotFound,

    /// An unspecified librist error occurred.
    #[error("librist error code: {0}")]
    Rist(i32),

    /// A generic error with a message.
    #[error("{0}")]
    Other(String),
}

/// Convenience Result type for RIST operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Converts a librist return code to a Result.
pub(crate) fn check_result(code: i32) -> Result<()> {
    if code >= 0 {
        return Ok(());
    }

    Err(match code {
        librist_sys::RIST_ERR_MALLOC => Error::Malloc,
        librist_sys::RIST_ERR_NULL_PEER => Error::NullPeer,
        librist_sys::RIST_ERR_INVALID_STRING_LENGTH => Error::InvalidStringLength,
        librist_sys::RIST_ERR_INVALID_PROFILE => Error::InvalidProfile,
        librist_sys::RIST_ERR_MISSING_CALLBACK_FUNCTION => Error::MissingCallback,
        librist_sys::RIST_ERR_NULL_CREDENTIALS => Error::NullCredentials,
        code => Error::Rist(code),
    })
}

/// Converts a librist return code to a Result, with timeout detection.
#[allow(dead_code)]
pub(crate) fn check_result_with_timeout(code: i32) -> Result<()> {
    if code > 0 {
        return Ok(());
    }
    if code == 0 {
        return Err(Error::Timeout);
    }
    check_result(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_result_success() {
        assert!(check_result(0).is_ok());
        assert!(check_result(1).is_ok());
        assert!(check_result(100).is_ok());
    }

    #[test]
    fn test_check_result_errors() {
        assert!(matches!(
            check_result(librist_sys::RIST_ERR_MALLOC),
            Err(Error::Malloc)
        ));
        assert!(matches!(
            check_result(librist_sys::RIST_ERR_NULL_PEER),
            Err(Error::NullPeer)
        ));
        assert!(matches!(check_result(-999), Err(Error::Rist(-999))));
    }

    #[test]
    fn test_error_display() {
        let err = Error::InvalidUrl("bad://url".to_string());
        assert_eq!(err.to_string(), "Invalid URL: bad://url");

        let err = Error::Rist(-42);
        assert_eq!(err.to_string(), "librist error code: -42");
    }
}
