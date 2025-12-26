//! Logging configuration for librist.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

/// Type alias for log callback closures.
type LogCallbackFn = Box<dyn Fn(LogLevel, &str) + Send + Sync>;

/// Log level for RIST operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum LogLevel {
    /// Logging disabled.
    Disable = -1,
    /// Error level - critical issues.
    Error = 3,
    /// Warning level - potential issues.
    Warn = 4,
    /// Notice level - important information.
    Notice = 5,
    /// Info level - general information (default).
    #[default]
    Info = 6,
    /// Debug level - detailed debugging information.
    Debug = 7,
    /// Simulate level - simulation/testing information.
    Simulate = 100,
}

impl From<LogLevel> for librist_sys::rist_log_level {
    fn from(l: LogLevel) -> Self {
        match l {
            LogLevel::Disable => librist_sys::rist_log_level::RIST_LOG_DISABLE,
            LogLevel::Error => librist_sys::rist_log_level::RIST_LOG_ERROR,
            LogLevel::Warn => librist_sys::rist_log_level::RIST_LOG_WARN,
            LogLevel::Notice => librist_sys::rist_log_level::RIST_LOG_NOTICE,
            LogLevel::Info => librist_sys::rist_log_level::RIST_LOG_INFO,
            LogLevel::Debug => librist_sys::rist_log_level::RIST_LOG_DEBUG,
            LogLevel::Simulate => librist_sys::rist_log_level::RIST_LOG_SIMULATE,
        }
    }
}

impl From<librist_sys::rist_log_level> for LogLevel {
    fn from(l: librist_sys::rist_log_level) -> Self {
        match l {
            librist_sys::rist_log_level::RIST_LOG_DISABLE => LogLevel::Disable,
            librist_sys::rist_log_level::RIST_LOG_ERROR => LogLevel::Error,
            librist_sys::rist_log_level::RIST_LOG_WARN => LogLevel::Warn,
            librist_sys::rist_log_level::RIST_LOG_NOTICE => LogLevel::Notice,
            librist_sys::rist_log_level::RIST_LOG_INFO => LogLevel::Info,
            librist_sys::rist_log_level::RIST_LOG_DEBUG => LogLevel::Debug,
            librist_sys::rist_log_level::RIST_LOG_SIMULATE => LogLevel::Simulate,
            _ => LogLevel::Info,
        }
    }
}

impl LogLevel {
    /// Converts to the corresponding `log` crate level.
    pub fn to_log_level(self) -> Option<log::Level> {
        match self {
            LogLevel::Disable => None,
            LogLevel::Error => Some(log::Level::Error),
            LogLevel::Warn => Some(log::Level::Warn),
            LogLevel::Notice | LogLevel::Info => Some(log::Level::Info),
            LogLevel::Debug | LogLevel::Simulate => Some(log::Level::Debug),
        }
    }
}

/// Logging settings for a RIST context.
///
/// This struct manages the lifetime of librist logging settings
/// and optional callback closures.
/// Type alias for the boxed callback to reduce complexity warnings
type LogCallback = Box<dyn Fn(LogLevel, &str) + Send + Sync>;

pub struct LoggingSettings {
    /// Raw pointer to librist logging settings.
    settings: *mut librist_sys::rist_logging_settings,
    /// Callback closure (kept alive while settings exist).
    /// Double-boxed to get a thin pointer for FFI.
    #[allow(dead_code)]
    callback: Option<Box<LogCallback>>,
}

impl LoggingSettings {
    /// Creates new logging settings with the specified level.
    pub fn new(level: LogLevel) -> crate::Result<Self> {
        let mut settings: *mut librist_sys::rist_logging_settings = ptr::null_mut();

        let ret = unsafe {
            librist_sys::rist_logging_set(
                &mut settings,
                level.into(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };

        if ret != 0 || settings.is_null() {
            return Err(crate::Error::Other("Failed to create logging settings".into()));
        }

        Ok(Self {
            settings,
            callback: None,
        })
    }

    /// Creates logging settings with a custom callback.
    pub fn with_callback<F>(level: LogLevel, callback: F) -> crate::Result<Self>
    where
        F: Fn(LogLevel, &str) + Send + Sync + 'static,
    {
        // Box the callback and then box the trait object to get a thin pointer
        let callback: LogCallbackFn = Box::new(callback);
        let callback = Box::new(callback);
        // Get a raw pointer to the Box (thin pointer, properly aligned)
        let callback_ptr = Box::into_raw(callback);

        let mut settings: *mut librist_sys::rist_logging_settings = ptr::null_mut();

        let ret = unsafe {
            librist_sys::rist_logging_set(
                &mut settings,
                level.into(),
                Some(log_callback_trampoline),
                callback_ptr as *mut c_void,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };

        if ret != 0 || settings.is_null() {
            // Clean up the callback if we failed
            unsafe {
                let _ = Box::from_raw(callback_ptr);
            }
            return Err(crate::Error::Other("Failed to create logging settings".into()));
        }

        // Convert back to Box for storage (we need to keep it alive)
        let callback = unsafe { Box::from_raw(callback_ptr) };

        Ok(Self {
            settings,
            callback: Some(callback),
        })
    }

    /// Creates logging settings that forward to the `log` crate.
    pub fn with_log_crate(level: LogLevel) -> crate::Result<Self> {
        Self::with_callback(level, |log_level, msg| {
            match log_level {
                LogLevel::Disable => {}
                LogLevel::Error => log::error!(target: "librist", "{}", msg),
                LogLevel::Warn => log::warn!(target: "librist", "{}", msg),
                LogLevel::Notice => log::info!(target: "librist", "{}", msg),
                LogLevel::Info => log::info!(target: "librist", "{}", msg),
                LogLevel::Debug => log::debug!(target: "librist", "{}", msg),
                LogLevel::Simulate => log::trace!(target: "librist", "{}", msg),
            }
        })
    }

    /// Returns the raw pointer to the logging settings.
    ///
    /// # Safety
    ///
    /// The returned pointer is valid as long as this `LoggingSettings` exists.
    pub(crate) fn as_raw(&self) -> *mut librist_sys::rist_logging_settings {
        self.settings
    }
}

impl Drop for LoggingSettings {
    fn drop(&mut self) {
        if !self.settings.is_null() {
            // Unset global logging first to avoid dangling pointer
            // (rist_logging_set may have set our settings as global)
            unsafe {
                librist_sys::rist_logging_unset_global();
                librist_sys::rist_logging_settings_free2(&mut self.settings);
            }
        }
    }
}

// Safety: LoggingSettings uses internal synchronization for callbacks
unsafe impl Send for LoggingSettings {}
unsafe impl Sync for LoggingSettings {}

/// C-compatible trampoline for log callbacks.
unsafe extern "C" fn log_callback_trampoline(
    arg: *mut c_void,
    level: librist_sys::rist_log_level,
    msg: *const c_char,
) -> c_int {
    if arg.is_null() || msg.is_null() {
        return 0;
    }

    // Catch panics to prevent unwinding across FFI boundary
    let result = std::panic::catch_unwind(|| {
        // SAFETY: arg is a pointer to Box<LogCallback> that we passed via callback registration
        // It's a thin pointer (pointer to a Box which contains the fat pointer)
        let callback_box = unsafe { &*(arg as *const LogCallback) };
        // SAFETY: msg is a valid C string from librist
        let msg_str = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
        let log_level = LogLevel::from(level);
        callback_box(log_level, &msg_str);
    });

    match result {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_default() {
        assert_eq!(LogLevel::default(), LogLevel::Info);
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Error < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Debug);
    }
}
