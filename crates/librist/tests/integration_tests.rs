//! Integration tests for librist-rust.
//!
//! These tests verify end-to-end functionality with actual sender/receiver
//! communication over localhost.
//!
//! # Design Decisions
//!
//! 1. Tests use `rusty_fork` to run in separate processes because librist has
//!    global state (CTX_LIST in libevsocket.c) that persists across context lifetimes
//! 2. We use `?buffer=200` to reduce RIST's default 1000ms recovery buffer
//! 3. Connection establishment is verified by actual data transmission, not just callbacks
//!    (librist connection callbacks fire before internal state is fully ready)

mod common;

use common::*;
use librist::{ConnectionStatus, Error, LogLevel, Profile, RistReceiver, RistSender};
use rusty_fork::rusty_fork_test;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

/// Magic bytes to identify handshake packets (unlikely to collide with test data)
const HANDSHAKE_MAGIC: &[u8] = b"__RIST_TEST_HANDSHAKE__";

// ============================================================================
// Test Infrastructure
// ============================================================================

/// Creates a connected sender/receiver pair for recv()-based tests.
/// Verifies the data path works before returning.
fn create_recv_test_context(profile: Profile) -> (RistSender, RistReceiver, u16) {
    let port = get_test_port();

    let receiver_ready = Arc::new(AtomicBool::new(false));
    let receiver_ready_clone = receiver_ready.clone();

    let receiver = RistReceiver::builder()
        .profile(profile)
        .log_level(LogLevel::Disable)
        .on_connection(move |_peer_id, status| {
            if status == ConnectionStatus::ClientConnected {
                receiver_ready_clone.store(true, Ordering::SeqCst);
            }
        })
        .build()
        .expect("Failed to create receiver");

    receiver
        .add_peer(&format!("rist://@:{}?buffer=200", port))
        .expect("Failed to add receiver peer");
    receiver.start().expect("Failed to start receiver");

    let sender_ready = Arc::new(AtomicBool::new(false));
    let sender_ready_clone = sender_ready.clone();

    let sender = RistSender::builder()
        .profile(profile)
        .log_level(LogLevel::Disable)
        .on_connection(move |_peer_id, status| {
            if status == ConnectionStatus::Established {
                sender_ready_clone.store(true, Ordering::SeqCst);
            }
        })
        .build()
        .expect("Failed to create sender");

    sender
        .add_peer(&format!("rist://127.0.0.1:{}?buffer=200", port))
        .expect("Failed to add sender peer");
    sender.start().expect("Failed to start sender");

    // Wait for connection callbacks
    let connected = wait_for(
        || receiver_ready.load(Ordering::SeqCst) && sender_ready.load(Ordering::SeqCst),
        Duration::from_secs(10),
    );
    assert!(
        connected,
        "Connection callbacks not received within timeout"
    );

    // Verify the full data path (send + receive) works
    let handshake_ok = wait_for(
        || {
            let _ = sender.send(HANDSHAKE_MAGIC);
            std::thread::sleep(Duration::from_millis(50));
            match receiver.try_recv() {
                Ok(Some(block)) if block.payload() == HANDSHAKE_MAGIC => true,
                _ => false,
            }
        },
        Duration::from_secs(10),
    );
    assert!(
        handshake_ok,
        "Data path handshake not completed within timeout"
    );

    // Drain any extra handshake packets
    let drain_end = std::time::Instant::now() + Duration::from_millis(200);
    while std::time::Instant::now() < drain_end {
        match receiver.try_recv() {
            Ok(Some(_)) => {}
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break,
        }
    }

    (sender, receiver, port)
}

/// Creates a connected sender/receiver pair with a data callback.
/// Verifies the data path works before returning.
fn create_callback_test_context<F>(
    profile: Profile,
    on_data: F,
) -> (RistSender, RistReceiver, u16, Arc<AtomicBool>)
where
    F: Fn(&[u8]) + Send + Sync + 'static,
{
    let port = get_test_port();

    let receiver_ready = Arc::new(AtomicBool::new(false));
    let receiver_ready_clone = receiver_ready.clone();

    let handshake_received = Arc::new(AtomicBool::new(false));
    let handshake_clone = handshake_received.clone();

    let receiver = RistReceiver::builder()
        .profile(profile)
        .log_level(LogLevel::Disable)
        .on_connection(move |_peer_id, status| {
            if status == ConnectionStatus::ClientConnected {
                receiver_ready_clone.store(true, Ordering::SeqCst);
            }
        })
        .on_data(move |block| {
            let payload = block.payload();
            if payload == HANDSHAKE_MAGIC {
                handshake_clone.store(true, Ordering::SeqCst);
            } else {
                on_data(payload);
            }
        })
        .build()
        .expect("Failed to create receiver");

    receiver
        .add_peer(&format!("rist://@:{}?buffer=200", port))
        .expect("Failed to add receiver peer");
    receiver.start().expect("Failed to start receiver");

    let sender_ready = Arc::new(AtomicBool::new(false));
    let sender_ready_clone = sender_ready.clone();

    let sender = RistSender::builder()
        .profile(profile)
        .log_level(LogLevel::Disable)
        .on_connection(move |_peer_id, status| {
            if status == ConnectionStatus::Established {
                sender_ready_clone.store(true, Ordering::SeqCst);
            }
        })
        .build()
        .expect("Failed to create sender");

    sender
        .add_peer(&format!("rist://127.0.0.1:{}?buffer=200", port))
        .expect("Failed to add sender peer");
    sender.start().expect("Failed to start sender");

    // Wait for connection callbacks
    let connected = wait_for(
        || receiver_ready.load(Ordering::SeqCst) && sender_ready.load(Ordering::SeqCst),
        Duration::from_secs(10),
    );
    assert!(
        connected,
        "Connection callbacks not received within timeout"
    );

    // Perform handshake to verify data path is ready
    let handshake_ok = wait_for(
        || {
            let _ = sender.send(HANDSHAKE_MAGIC);
            std::thread::sleep(Duration::from_millis(50));
            handshake_received.load(Ordering::SeqCst)
        },
        Duration::from_secs(10),
    );
    assert!(handshake_ok, "Handshake not completed within timeout");

    // Allow time for any extra handshake packets to be processed
    std::thread::sleep(Duration::from_millis(100));

    (sender, receiver, port, handshake_received)
}

// ============================================================================
// Tests using rusty_fork - each test runs in its own process
// ============================================================================

rusty_fork_test! {
    #[test]
    fn test_sender_receiver_basic() {
        let (sender, receiver, _port) = create_recv_test_context(Profile::Main);

        let test_data = b"Hello, RIST!";
        sender.send(test_data).expect("Failed to send");

        let block = receiver.recv(DEFAULT_TIMEOUT_MS).expect("Failed to receive");
        assert_eq!(block.payload(), test_data);
    }

    #[test]
    fn test_send_multiple_packets() {
        let (sender, receiver, _port) = create_recv_test_context(Profile::Main);

        let packet_count = 10;
        for i in 0..packet_count {
            let data = generate_mpeg_ts_bundle(i);
            sender.send(&data).expect("Failed to send");
        }

        let mut received = 0;
        for _ in 0..packet_count {
            match receiver.recv(DEFAULT_TIMEOUT_MS) {
                Ok(_) => received += 1,
                Err(Error::Timeout) => break,
                Err(e) => panic!("Unexpected error: {:?}", e),
            }
        }
        assert_eq!(received, packet_count);
    }

    #[test]
    fn test_recv_timeout() {
        let port = get_test_port();
        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .build()
            .unwrap();
        receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
        receiver.start().unwrap();

        // No sender connected - should timeout
        assert!(matches!(receiver.recv(SHORT_TIMEOUT_MS), Err(Error::Timeout)));
    }

    #[test]
    fn test_try_recv_non_blocking() {
        let port = get_test_port();
        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .build()
            .unwrap();
        receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
        receiver.start().unwrap();

        assert!(receiver.try_recv().unwrap().is_none());
    }

    #[test]
    fn test_send_before_start() {
        let sender = RistSender::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .build()
            .unwrap();
        sender.add_peer("rist://127.0.0.1:5000").unwrap();
        assert!(matches!(sender.send(&[1, 2, 3]), Err(Error::NotStarted)));
    }

    #[test]
    fn test_recv_before_start() {
        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .build()
            .unwrap();
        receiver.add_peer("rist://@:5000").unwrap();
        assert!(matches!(receiver.recv(100), Err(Error::NotStarted)));
    }

    #[test]
    fn test_double_start_sender() {
        let sender = RistSender::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .build()
            .unwrap();
        sender.add_peer("rist://127.0.0.1:5000").unwrap();
        sender.start().unwrap();
        assert!(matches!(sender.start(), Err(Error::AlreadyStarted)));
    }

    #[test]
    fn test_double_start_receiver() {
        let port = get_test_port();
        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .build()
            .unwrap();
        receiver.add_peer(&format!("rist://@:{}", port)).unwrap();
        receiver.start().unwrap();
        assert!(matches!(receiver.start(), Err(Error::AlreadyStarted)));
    }

    #[test]
    fn test_data_callback() {
        let received = Arc::new(AtomicBool::new(false));
        let received_clone = received.clone();

        let (sender, _receiver, _port, _) = create_callback_test_context(Profile::Main, move |_payload| {
            received_clone.store(true, Ordering::SeqCst);
        });

        sender.send(b"test data").expect("Failed to send");

        assert!(
            wait_for(|| received.load(Ordering::SeqCst), Duration::from_secs(5)),
            "Data callback not invoked"
        );
    }

    #[test]
    fn test_auth_connect_accept() {
        let port = get_test_port();
        let auth_called = Arc::new(AtomicBool::new(false));
        let auth_called_clone = auth_called.clone();

        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .on_auth_connect(move |_conn_ip, _conn_port, _local_ip, _local_port, _peer_id| {
                auth_called_clone.store(true, Ordering::SeqCst);
                true
            })
            .build()
            .unwrap();
        receiver.add_peer(&format!("rist://@:{}?buffer=200", port)).unwrap();
        receiver.start().unwrap();

        let sender_connected = Arc::new(AtomicBool::new(false));
        let sender_connected_clone = sender_connected.clone();

        let sender = RistSender::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .on_connection(move |_peer_id, status| {
                if status == ConnectionStatus::Established {
                    sender_connected_clone.store(true, Ordering::SeqCst);
                }
            })
            .build()
            .unwrap();
        sender.add_peer(&format!("rist://127.0.0.1:{}?buffer=200", port)).unwrap();
        sender.start().unwrap();

        assert!(
            wait_for(|| sender_connected.load(Ordering::SeqCst), Duration::from_secs(10)),
            "Sender not connected"
        );
        assert!(
            wait_for(|| auth_called.load(Ordering::SeqCst), Duration::from_secs(5)),
            "Auth callback not invoked"
        );
    }

    #[test]
    fn test_connection_callback() {
        let port = get_test_port();
        let connected = Arc::new(AtomicBool::new(false));
        let connected_clone = connected.clone();

        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .on_connection(move |_peer_id, status| {
                if status.is_connected() {
                    connected_clone.store(true, Ordering::SeqCst);
                }
            })
            .build()
            .unwrap();
        receiver.add_peer(&format!("rist://@:{}?buffer=200", port)).unwrap();
        receiver.start().unwrap();

        let sender = RistSender::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .build()
            .unwrap();
        sender.add_peer(&format!("rist://127.0.0.1:{}?buffer=200", port)).unwrap();
        sender.start().unwrap();

        assert!(
            wait_for(|| connected.load(Ordering::SeqCst), Duration::from_secs(10)),
            "Connection callback not invoked"
        );
    }

    #[test]
    fn test_simple_profile() {
        let (sender, receiver, _port) = create_recv_test_context(Profile::Simple);

        let test_data = b"Simple profile test";
        sender.send(test_data).unwrap();

        let block = receiver.recv(DEFAULT_TIMEOUT_MS).unwrap();
        assert_eq!(block.payload(), test_data);
    }

    #[test]
    fn test_advanced_profile() {
        let (sender, receiver, _port) = create_recv_test_context(Profile::Advanced);

        let test_data = b"Advanced profile test";
        sender.send(test_data).unwrap();

        let block = receiver.recv(DEFAULT_TIMEOUT_MS).unwrap();
        assert_eq!(block.payload(), test_data);
    }

    #[test]
    fn test_stats_callback_invoked() {
        let port = get_test_port();
        let stats_count = Arc::new(AtomicU32::new(0));
        let stats_clone = stats_count.clone();

        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .build()
            .unwrap();
        receiver.add_peer(&format!("rist://@:{}?buffer=200", port)).unwrap();
        receiver.start().unwrap();

        let sender = RistSender::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .on_stats(100, move |_stats| {
                stats_clone.fetch_add(1, Ordering::SeqCst);
            })
            .build()
            .unwrap();
        sender.add_peer(&format!("rist://127.0.0.1:{}?buffer=200", port)).unwrap();
        sender.start().unwrap();

        // Wait for connection and send some data
        std::thread::sleep(Duration::from_millis(500));
        for _ in 0..5 {
            let _ = sender.send(b"stats test");
            std::thread::sleep(Duration::from_millis(100));
        }

        assert!(
            wait_for(|| stats_count.load(Ordering::SeqCst) > 0, Duration::from_secs(5)),
            "Stats callback not invoked"
        );
    }

    #[test]
    fn test_send_to_virtual_port() {
        let (sender, receiver, _port) = create_recv_test_context(Profile::Main);

        let test_data = b"Virtual port test";
        sender.send_to_port(test_data, 1234).expect("Failed to send");

        let block = receiver.recv(DEFAULT_TIMEOUT_MS).unwrap();
        assert_eq!(block.payload(), test_data);
        assert_eq!(block.virtual_dst_port(), 1234);
    }

    #[test]
    fn stress_test_high_throughput() {
        let received = Arc::new(AtomicU32::new(0));
        let received_clone = received.clone();

        let (sender, _receiver, _port, _) = create_callback_test_context(Profile::Main, move |payload| {
            if payload.starts_with(b"pkt:") {
                received_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        let packet_count = 500u32;
        for i in 0..packet_count {
            let data = format!("pkt:{:06}", i);
            sender.send(data.as_bytes()).expect("Failed to send");
        }

        // Wait for at least 90% delivery
        let min_expected = packet_count * 9 / 10;
        assert!(
            wait_for(
                || received.load(Ordering::SeqCst) >= min_expected,
                Duration::from_secs(10)
            ),
            "Expected at least {} packets, got {}",
            min_expected,
            received.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn stress_test_context_churn() {
        // Rapid create/destroy - tests memory safety, no actual I/O
        for i in 0..50 {
            let port = 45000 + (i as u16);

            let sender = RistSender::builder()
                .profile(Profile::Main)
                .log_level(LogLevel::Disable)
                .build()
                .expect("Failed to create sender");
            sender.add_peer(&format!("rist://127.0.0.1:{}", port)).ok();
            drop(sender);

            let receiver = RistReceiver::builder()
                .profile(Profile::Main)
                .log_level(LogLevel::Disable)
                .build()
                .expect("Failed to create receiver");
            receiver.add_peer(&format!("rist://@:{}", port)).ok();
            drop(receiver);
        }
    }

    #[test]
    fn stress_test_payload_sizes() {
        let received_sizes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received_clone = received_sizes.clone();

        let (sender, _receiver, _port, _) = create_callback_test_context(Profile::Main, move |payload| {
            if payload.len() >= 2 {
                let marked = ((payload[0] as usize) << 8) | (payload[1] as usize);
                if marked == payload.len() {
                    received_clone.lock().unwrap().push(payload.len());
                }
            }
        });

        let sizes = [100, 188, 500, 1000, 1316];
        for &size in &sizes {
            let mut data = vec![0u8; size];
            data[0] = (size >> 8) as u8;
            data[1] = (size & 0xFF) as u8;
            for i in 2..size {
                data[i] = (i % 256) as u8;
            }
            sender.send(&data).expect("Failed to send");
        }

        assert!(
            wait_for(
                || received_sizes.lock().unwrap().len() >= sizes.len(),
                Duration::from_secs(10)
            ),
            "Not all payload sizes received: {:?}",
            received_sizes.lock().unwrap()
        );
    }

    #[test]
    fn stress_test_callback_safety() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let (sender, _receiver, _port, _) = create_callback_test_context(Profile::Main, move |payload| {
            if payload.starts_with(b"DATA:") {
                counter_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        let packet_count = 100u32;
        for i in 0..packet_count {
            let data = format!("DATA:{:06}", i);
            sender.send(data.as_bytes()).expect("Failed to send");
        }

        // Expect at least 80% delivery
        let min_expected = packet_count * 8 / 10;
        assert!(
            wait_for(
                || counter.load(Ordering::SeqCst) >= min_expected,
                Duration::from_secs(10)
            ),
            "Expected at least {} callbacks, got {}",
            min_expected,
            counter.load(Ordering::SeqCst)
        );
    }
}
