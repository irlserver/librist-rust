//! Callback types and FFI trampolines for librist events.
//!
//! This module contains the callback type definitions and unsafe FFI trampoline
//! functions that bridge between librist's C callbacks and Rust closures.

use crate::data::DataBlock;
use crate::oob::OobBlock;
use crate::stats::{ReceiverStats, SenderStats, StatsWrapper};
use crate::types::ConnectionStatus;
use parking_lot::Mutex;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::Arc;

// ============================================================================
// Callback Type Aliases
// ============================================================================

/// Log callback: (level, message).
pub(crate) type LogCallback = Box<dyn Fn(crate::LogLevel, &str) + Send + Sync>;

/// Stats callback for sender statistics.
pub(crate) type StatsCallback<T> = Box<dyn Fn(&T) + Send + Sync>;

/// Connection status change callback.
pub(crate) type ConnectionCallback = Box<dyn Fn(u32, ConnectionStatus) + Send + Sync>;

/// Data received callback.
pub(crate) type DataCallback = Box<dyn Fn(DataBlock) + Send + Sync>;

/// Auth connect callback: (conn_ip, conn_port, local_ip, local_port, peer_id) -> accept.
pub(crate) type AuthConnectCallback = Box<dyn Fn(&str, u16, &str, u16, u32) -> bool + Send + Sync>;

/// Auth disconnect callback: (peer_id).
pub(crate) type AuthDisconnectCallback = Box<dyn Fn(u32) + Send + Sync>;

/// Out-of-band data received callback.
pub(crate) type OobCallback = Box<dyn Fn(OobBlock) + Send + Sync>;

// ============================================================================
// Callback Storage Structs
// ============================================================================

/// Callbacks for RistSender.
#[derive(Default)]
pub(crate) struct SenderCallbacks {
    pub stats: Option<StatsCallback<SenderStats>>,
    pub connection: Option<ConnectionCallback>,
    pub oob: Option<OobCallback>,
}

/// Callbacks for RistReceiver.
#[derive(Default)]
pub(crate) struct ReceiverCallbacks {
    pub stats: Option<StatsCallback<ReceiverStats>>,
    pub connection: Option<ConnectionCallback>,
    pub data: Option<DataCallback>,
    pub auth_connect: Option<AuthConnectCallback>,
    pub auth_disconnect: Option<AuthDisconnectCallback>,
    pub oob: Option<OobCallback>,
}

// ============================================================================
// Sender Callback Trampolines
// ============================================================================

/// Trampoline for sender stats callback.
pub(crate) unsafe extern "C" fn sender_stats_trampoline(
    arg: *mut c_void,
    stats: *const librist_sys::rist_stats,
) -> c_int {
    if arg.is_null() || stats.is_null() {
        return 0;
    }

    let _ = std::panic::catch_unwind(|| {
        // SAFETY: arg is a pointer we passed via Arc::into_raw
        let callbacks = unsafe { Arc::from_raw(arg as *const Mutex<SenderCallbacks>) };
        let callbacks_ref = Arc::clone(&callbacks);
        let _ = Arc::into_raw(callbacks); // Don't drop, just release our reference

        let guard = callbacks_ref.lock();
        if let Some(ref callback) = guard.stats {
            // SAFETY: stats is valid for the duration of this callback
            let wrapper = unsafe { StatsWrapper::from_raw(stats) };
            if let Some(sender_stats) = wrapper.as_sender_stats() {
                callback(&sender_stats);
            }
            std::mem::forget(wrapper); // Don't free, librist owns it
        }
    });

    0
}

/// Trampoline for sender connection status callback.
pub(crate) unsafe extern "C" fn sender_connection_trampoline(
    arg: *mut c_void,
    peer: *mut librist_sys::rist_peer,
    status: librist_sys::rist_connection_status,
) {
    if arg.is_null() {
        return;
    }

    let _ = std::panic::catch_unwind(|| {
        // SAFETY: arg points to valid data we passed in
        if let Some(callbacks) = unsafe { (arg as *const Mutex<SenderCallbacks>).as_ref() } {
            if let Some(guard) = callbacks.try_lock() {
                if let Some(ref callback) = guard.connection {
                    let peer_id = if peer.is_null() {
                        0
                    } else {
                        unsafe { librist_sys::rist_peer_get_id(peer) }
                    };
                    callback(peer_id, status.into());
                }
            }
        }
    });
}

/// Trampoline for sender OOB callback.
pub(crate) unsafe extern "C" fn sender_oob_trampoline(
    arg: *mut c_void,
    oob_block: *const librist_sys::rist_oob_block,
) -> c_int {
    if arg.is_null() || oob_block.is_null() {
        return 0;
    }

    let _ = std::panic::catch_unwind(|| {
        let callbacks = unsafe { Arc::from_raw(arg as *const Mutex<SenderCallbacks>) };
        let callbacks_ref = Arc::clone(&callbacks);
        let _ = Arc::into_raw(callbacks);

        let guard = callbacks_ref.lock();
        if let Some(ref callback) = guard.oob {
            let block = unsafe { OobBlock::from_raw(oob_block) };
            callback(block);
        }
    });

    0
}

// ============================================================================
// Receiver Callback Trampolines
// ============================================================================

/// Trampoline for receiver stats callback.
pub(crate) unsafe extern "C" fn receiver_stats_trampoline(
    arg: *mut c_void,
    stats: *const librist_sys::rist_stats,
) -> c_int {
    if arg.is_null() || stats.is_null() {
        return 0;
    }

    let _ = std::panic::catch_unwind(|| {
        // SAFETY: arg is a pointer we passed via Arc::into_raw
        let callbacks = unsafe { Arc::from_raw(arg as *const Mutex<ReceiverCallbacks>) };
        let callbacks_ref = Arc::clone(&callbacks);
        let _ = Arc::into_raw(callbacks);

        let guard = callbacks_ref.lock();
        if let Some(ref callback) = guard.stats {
            // SAFETY: stats is valid for the duration of this callback
            let wrapper = unsafe { StatsWrapper::from_raw(stats) };
            if let Some(receiver_stats) = wrapper.as_receiver_stats() {
                callback(&receiver_stats);
            }
            std::mem::forget(wrapper);
        }
    });

    0
}

/// Trampoline for receiver data callback.
pub(crate) unsafe extern "C" fn receiver_data_trampoline(
    arg: *mut c_void,
    data_block: *mut librist_sys::rist_data_block,
) -> c_int {
    if arg.is_null() || data_block.is_null() {
        return 0;
    }

    let _ = std::panic::catch_unwind(|| {
        // SAFETY: arg is a pointer we passed via Arc::into_raw
        let callbacks = unsafe { Arc::from_raw(arg as *const Mutex<ReceiverCallbacks>) };
        let callbacks_ref = Arc::clone(&callbacks);
        let _ = Arc::into_raw(callbacks);

        let guard = callbacks_ref.lock();
        if let Some(ref callback) = guard.data {
            let block = DataBlock::from_received(data_block);
            callback(block);
        }
    });

    0
}

/// Trampoline for receiver connection status callback.
pub(crate) unsafe extern "C" fn receiver_connection_trampoline(
    arg: *mut c_void,
    peer: *mut librist_sys::rist_peer,
    status: librist_sys::rist_connection_status,
) {
    if arg.is_null() {
        return;
    }

    let _ = std::panic::catch_unwind(|| {
        // SAFETY: arg points to valid data we passed in
        if let Some(callbacks) = unsafe { (arg as *const Mutex<ReceiverCallbacks>).as_ref() } {
            if let Some(guard) = callbacks.try_lock() {
                if let Some(ref callback) = guard.connection {
                    let peer_id = if peer.is_null() {
                        0
                    } else {
                        unsafe { librist_sys::rist_peer_get_id(peer) }
                    };
                    callback(peer_id, status.into());
                }
            }
        }
    });
}

/// Trampoline for auth connect callback.
pub(crate) unsafe extern "C" fn auth_connect_trampoline(
    arg: *mut c_void,
    conn_ip: *const c_char,
    conn_port: u16,
    local_ip: *const c_char,
    local_port: u16,
    peer: *mut librist_sys::rist_peer,
) -> c_int {
    if arg.is_null() {
        return 0; // Accept by default
    }

    let result = std::panic::catch_unwind(|| {
        // SAFETY: arg is a pointer we passed via Arc::into_raw
        let callbacks = unsafe { Arc::from_raw(arg as *const Mutex<ReceiverCallbacks>) };
        let callbacks_ref = Arc::clone(&callbacks);
        let _ = Arc::into_raw(callbacks); // Don't drop, just release our reference

        let guard = callbacks_ref.lock();
        if let Some(ref callback) = guard.auth_connect {
            // SAFETY: librist guarantees these are valid C strings
            let conn_ip_str = if conn_ip.is_null() {
                ""
            } else {
                unsafe { std::ffi::CStr::from_ptr(conn_ip) }
                    .to_str()
                    .unwrap_or("")
            };
            let local_ip_str = if local_ip.is_null() {
                ""
            } else {
                unsafe { std::ffi::CStr::from_ptr(local_ip) }
                    .to_str()
                    .unwrap_or("")
            };
            let peer_id = if peer.is_null() {
                0
            } else {
                unsafe { librist_sys::rist_peer_get_id(peer) }
            };

            if callback(conn_ip_str, conn_port, local_ip_str, local_port, peer_id) {
                0 // Accept
            } else {
                -1 // Reject
            }
        } else {
            0 // Accept by default if no callback
        }
    });

    result.unwrap_or(0)
}

/// Trampoline for auth disconnect callback.
pub(crate) unsafe extern "C" fn auth_disconnect_trampoline(
    arg: *mut c_void,
    peer: *mut librist_sys::rist_peer,
) -> c_int {
    if arg.is_null() {
        return 0;
    }

    let _ = std::panic::catch_unwind(|| {
        // SAFETY: arg is a pointer we passed via Arc::into_raw
        let callbacks = unsafe { Arc::from_raw(arg as *const Mutex<ReceiverCallbacks>) };
        let callbacks_ref = Arc::clone(&callbacks);
        let _ = Arc::into_raw(callbacks);

        let guard = callbacks_ref.lock();
        if let Some(ref callback) = guard.auth_disconnect {
            let peer_id = if peer.is_null() {
                0
            } else {
                unsafe { librist_sys::rist_peer_get_id(peer) }
            };
            callback(peer_id);
        }
    });

    0
}

/// Trampoline for receiver OOB callback.
pub(crate) unsafe extern "C" fn receiver_oob_trampoline(
    arg: *mut c_void,
    oob_block: *const librist_sys::rist_oob_block,
) -> c_int {
    if arg.is_null() || oob_block.is_null() {
        return 0;
    }

    let _ = std::panic::catch_unwind(|| {
        let callbacks = unsafe { Arc::from_raw(arg as *const Mutex<ReceiverCallbacks>) };
        let callbacks_ref = Arc::clone(&callbacks);
        let _ = Arc::into_raw(callbacks);

        let guard = callbacks_ref.lock();
        if let Some(ref callback) = guard.oob {
            let block = unsafe { OobBlock::from_raw(oob_block) };
            callback(block);
        }
    });

    0
}
