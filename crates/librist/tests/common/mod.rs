//! Common test utilities and helpers.

use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

/// Port counter within a process.
static PORT_OFFSET: AtomicU16 = AtomicU16::new(0);

/// Gets a unique port for testing.
///
/// Uses the process ID to generate a base port, ensuring different forked
/// processes use different port ranges. Within a process, allocates sequentially.
/// 
/// Port range: 30000-60000, with each process getting a unique starting offset
/// based on its PID.
pub fn get_test_port() -> u16 {
    let pid = std::process::id() as u16;
    // Use PID to create a base offset, wrapping to stay in valid port range
    // Each test within a process increments by 10 (enough for RIST's port pairs)
    let base = 30000 + (pid % 3000) * 10;
    let offset = PORT_OFFSET.fetch_add(10, Ordering::SeqCst);
    base + offset
}

/// Default timeout for test operations.
pub const DEFAULT_TIMEOUT_MS: i32 = 5000;

/// Short timeout for quick checks.
pub const SHORT_TIMEOUT_MS: i32 = 100;

/// Waits for a condition with timeout.
pub fn wait_for<F>(condition: F, timeout: Duration) -> bool
where
    F: Fn() -> bool,
{
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Generates test data patterns.
pub fn generate_test_data(size: usize, pattern: u8) -> Vec<u8> {
    vec![pattern; size]
}

/// Generates MPEG-TS-like test packets (188 bytes with sync byte).
pub fn generate_mpeg_ts_packet(seq: u64) -> Vec<u8> {
    let mut packet = vec![0u8; 188];
    packet[0] = 0x47; // Sync byte
    packet[1] = 0x1F;
    packet[2] = 0xFF;
    packet[3] = 0x10;
    // Embed sequence number
    packet[4..12].copy_from_slice(&seq.to_be_bytes());
    packet
}

/// Generates multiple MPEG-TS packets bundled together (7 packets = 1316 bytes).
pub fn generate_mpeg_ts_bundle(seq: u64) -> Vec<u8> {
    let mut bundle = Vec::with_capacity(1316);
    for i in 0..7 {
        let mut packet = vec![0u8; 188];
        packet[0] = 0x47;
        packet[1] = 0x1F;
        packet[2] = 0xFF;
        packet[3] = 0x10;
        let sub_seq = seq * 7 + i;
        packet[4..12].copy_from_slice(&sub_seq.to_be_bytes());
        bundle.extend_from_slice(&packet);
    }
    bundle
}
