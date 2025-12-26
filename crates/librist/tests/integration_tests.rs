//! Integration tests for librist-rust.
//!
//! These tests verify end-to-end functionality with actual sender/receiver
//! communication over localhost.

mod common;

use common::*;
use librist::{Error, Profile, RistReceiver, RistSender};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

// ============================================================================
// Basic Sender/Receiver Tests
// ============================================================================

#[test]
fn test_sender_receiver_basic() {
    let port = get_test_port();

    // Create receiver
    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .build()
        .expect("Failed to create receiver");

    receiver
        .add_peer(&format!("rist://@:{}", port))
        .expect("Failed to add receiver peer");

    receiver.start().expect("Failed to start receiver");

    // Create sender
    let sender = RistSender::builder()
        .profile(Profile::Main)
        .build()
        .expect("Failed to create sender");

    sender
        .add_peer(&format!("rist://127.0.0.1:{}", port))
        .expect("Failed to add sender peer");

    sender.start().expect("Failed to start sender");

    // Wait for connection to establish
    std::thread::sleep(Duration::from_millis(200));

    // Send test data
    let test_data = b"Hello, RIST!";
    let bytes_sent = sender.send(test_data).expect("Failed to send data");
    assert_eq!(bytes_sent, test_data.len());

    // Receive data
    let block = receiver.recv(DEFAULT_TIMEOUT_MS).expect("Failed to receive data");
    assert_eq!(block.payload(), test_data);
}

#[test]
fn test_send_multiple_packets() {
    let port = get_test_port();

    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
    receiver.start().unwrap();

    let sender = RistSender::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    sender.add_peer(&format!("rist://127.0.0.1:{}", port)).unwrap();
    sender.start().unwrap();

    std::thread::sleep(Duration::from_millis(200));

    // Send multiple packets
    let packet_count = 10;
    for i in 0..packet_count {
        let data = generate_mpeg_ts_bundle(i);
        sender.send(&data).expect("Failed to send packet");
    }

    // Receive all packets
    let mut received_count = 0;
    for _ in 0..packet_count {
        match receiver.recv(DEFAULT_TIMEOUT_MS) {
            Ok(_block) => received_count += 1,
            Err(Error::Timeout) => break,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    assert_eq!(received_count, packet_count, "Should receive all packets");
}

#[test]
fn test_recv_timeout() {
    let port = get_test_port();

    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
    receiver.start().unwrap();

    // No sender, should timeout
    let result = receiver.recv(SHORT_TIMEOUT_MS);
    assert!(
        matches!(result, Err(Error::Timeout)),
        "Expected timeout error"
    );
}

#[test]
fn test_try_recv_non_blocking() {
    let port = get_test_port();

    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
    receiver.start().unwrap();

    // No sender, should return None immediately
    let result = receiver.try_recv().unwrap();
    assert!(result.is_none(), "Expected None for non-blocking recv");
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_send_before_start() {
    let sender = RistSender::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    sender.add_peer("rist://127.0.0.1:5000").unwrap();

    let result = sender.send(&[1, 2, 3]);
    assert!(
        matches!(result, Err(Error::NotStarted)),
        "Expected NotStarted error"
    );
}

#[test]
fn test_recv_before_start() {
    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    receiver.add_peer("rist://@:5000").unwrap();

    let result = receiver.recv(100);
    assert!(
        matches!(result, Err(Error::NotStarted)),
        "Expected NotStarted error"
    );
}

#[test]
fn test_double_start_sender() {
    let sender = RistSender::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    sender.add_peer("rist://127.0.0.1:5000").unwrap();

    sender.start().unwrap();
    let result = sender.start();
    assert!(
        matches!(result, Err(Error::AlreadyStarted)),
        "Expected AlreadyStarted error"
    );
}

#[test]
fn test_double_start_receiver() {
    let port = get_test_port();
    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    receiver.add_peer(&format!("rist://@:{}", port)).unwrap();

    receiver.start().unwrap();
    let result = receiver.start();
    assert!(
        matches!(result, Err(Error::AlreadyStarted)),
        "Expected AlreadyStarted error"
    );
}

// ============================================================================
// Callback Tests
// ============================================================================

#[test]
fn test_data_callback() {
    let port = get_test_port();
    let received = Arc::new(AtomicBool::new(false));
    let received_clone = received.clone();

    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .on_data(move |_block| {
            received_clone.store(true, Ordering::SeqCst);
        })
        .build()
        .unwrap();
    receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
    receiver.start().unwrap();

    let sender = RistSender::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    sender.add_peer(&format!("rist://127.0.0.1:{}", port)).unwrap();
    sender.start().unwrap();

    std::thread::sleep(Duration::from_millis(200));

    sender.send(b"test data").unwrap();

    // Wait for callback
    let callback_invoked = wait_for(|| received.load(Ordering::SeqCst), Duration::from_secs(5));
    assert!(callback_invoked, "Data callback should have been invoked");
}

#[test]
fn test_auth_connect_accept() {
    let port = get_test_port();
    let auth_called = Arc::new(AtomicBool::new(false));
    let auth_called_clone = auth_called.clone();

    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .on_auth_connect(move |_conn_ip, _conn_port, _local_ip, _local_port, _peer_id| {
            auth_called_clone.store(true, Ordering::SeqCst);
            true // Accept
        })
        .build()
        .unwrap();
    receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
    receiver.start().unwrap();

    let sender = RistSender::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    sender.add_peer(&format!("rist://127.0.0.1:{}", port)).unwrap();
    sender.start().unwrap();

    // Wait for connection and auth callback
    std::thread::sleep(Duration::from_millis(500));

    // Send data to trigger connection
    sender.send(b"hello").unwrap();

    let auth_invoked = wait_for(|| auth_called.load(Ordering::SeqCst), Duration::from_secs(5));
    assert!(auth_invoked, "Auth connect callback should have been invoked");
}

#[test]
fn test_connection_callback() {
    let port = get_test_port();
    let connected = Arc::new(AtomicBool::new(false));
    let connected_clone = connected.clone();

    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .on_connection(move |_peer_id, status| {
            if status.is_connected() {
                connected_clone.store(true, Ordering::SeqCst);
            }
        })
        .build()
        .unwrap();
    receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
    receiver.start().unwrap();

    let sender = RistSender::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    sender.add_peer(&format!("rist://127.0.0.1:{}", port)).unwrap();
    sender.start().unwrap();

    // Send data to establish connection
    std::thread::sleep(Duration::from_millis(200));
    sender.send(b"hello").unwrap();

    let connection_established =
        wait_for(|| connected.load(Ordering::SeqCst), Duration::from_secs(5));
    assert!(
        connection_established,
        "Connection callback should indicate connected"
    );
}

// ============================================================================
// Profile Tests
// ============================================================================

#[test]
fn test_simple_profile() {
    let port = get_test_port();

    let receiver = RistReceiver::builder()
        .profile(Profile::Simple)
        .build()
        .unwrap();
    receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
    receiver.start().unwrap();

    let sender = RistSender::builder()
        .profile(Profile::Simple)
        .build()
        .unwrap();
    sender.add_peer(&format!("rist://127.0.0.1:{}", port)).unwrap();
    sender.start().unwrap();

    std::thread::sleep(Duration::from_millis(200));

    let test_data = b"Simple profile test";
    sender.send(test_data).unwrap();

    let block = receiver.recv(DEFAULT_TIMEOUT_MS).unwrap();
    assert_eq!(block.payload(), test_data);
}

#[test]
fn test_advanced_profile() {
    let port = get_test_port();

    let receiver = RistReceiver::builder()
        .profile(Profile::Advanced)
        .build()
        .unwrap();
    receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
    receiver.start().unwrap();

    let sender = RistSender::builder()
        .profile(Profile::Advanced)
        .build()
        .unwrap();
    sender.add_peer(&format!("rist://127.0.0.1:{}", port)).unwrap();
    sender.start().unwrap();

    std::thread::sleep(Duration::from_millis(200));

    let test_data = b"Advanced profile test";
    sender.send(test_data).unwrap();

    let block = receiver.recv(DEFAULT_TIMEOUT_MS).unwrap();
    assert_eq!(block.payload(), test_data);
}

// ============================================================================
// Stats Tests
// ============================================================================

#[test]
fn test_stats_callback_invoked() {
    let port = get_test_port();
    let stats_received = Arc::new(AtomicU32::new(0));
    let stats_clone = stats_received.clone();

    let sender = RistSender::builder()
        .profile(Profile::Main)
        .on_stats(100, move |_stats| {
            stats_clone.fetch_add(1, Ordering::SeqCst);
        })
        .build()
        .unwrap();
    sender.add_peer(&format!("rist://127.0.0.1:{}", port)).unwrap();
    sender.start().unwrap();

    // Create receiver to complete connection
    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
    receiver.start().unwrap();

    // Wait for connection and stats
    std::thread::sleep(Duration::from_millis(200));

    // Send some data to generate stats
    for _ in 0..10 {
        sender.send(b"stats test").ok();
        std::thread::sleep(Duration::from_millis(50));
    }

    let stats_invoked = wait_for(
        || stats_received.load(Ordering::SeqCst) > 0,
        Duration::from_secs(5),
    );
    assert!(stats_invoked, "Stats callback should have been invoked");
}

// ============================================================================
// Virtual Port Tests
// ============================================================================

#[test]
fn test_send_to_virtual_port() {
    let port = get_test_port();

    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
    receiver.start().unwrap();

    let sender = RistSender::builder()
        .profile(Profile::Main)
        .build()
        .unwrap();
    sender.add_peer(&format!("rist://127.0.0.1:{}", port)).unwrap();
    sender.start().unwrap();

    std::thread::sleep(Duration::from_millis(200));

    // Send to virtual port 1234
    let test_data = b"Virtual port test";
    sender.send_to_port(test_data, 1234).unwrap();

    let block = receiver.recv(DEFAULT_TIMEOUT_MS).unwrap();
    assert_eq!(block.payload(), test_data);
    assert_eq!(block.virt_dst_port(), 1234);
}
