//! Debug test for recv() behavior

use librist::{ConnectionStatus, Profile, RistReceiver, RistSender};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

fn main() {
    let port = 19999;

    let receiver_connected = Arc::new(AtomicBool::new(false));
    let receiver_connected_clone = receiver_connected.clone();
    let data_received = Arc::new(AtomicU32::new(0));
    let data_received_clone = data_received.clone();

    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .on_connection(move |peer_id, status| {
            println!("Receiver: peer={} status={:?}", peer_id, status);
            if status == ConnectionStatus::ClientConnected {
                receiver_connected_clone.store(true, Ordering::SeqCst);
            }
        })
        // NO on_data callback - use recv() instead
        .build()
        .expect("Failed to create receiver");

    // Mark as used to avoid warning
    let _ = data_received_clone;

    // Use buffer=50 for low latency testing
    receiver
        .add_peer(&format!("rist://@:{}?buffer=50", port))
        .expect("Failed to add receiver peer");
    receiver.start().expect("Failed to start receiver");
    println!("Receiver started on port {}", port);

    let sender_connected = Arc::new(AtomicBool::new(false));
    let sender_connected_clone = sender_connected.clone();

    let sender = RistSender::builder()
        .profile(Profile::Main)
        .on_connection(move |peer_id, status| {
            println!("Sender: peer={} status={:?}", peer_id, status);
            if status == ConnectionStatus::Established {
                sender_connected_clone.store(true, Ordering::SeqCst);
            }
        })
        .build()
        .expect("Failed to create sender");

    // Use buffer=50 for low latency testing
    sender
        .add_peer(&format!("rist://127.0.0.1:{}?buffer=50", port))
        .expect("Failed to add sender peer");
    sender.start().expect("Failed to start sender");
    println!("Sender started");

    // Wait for connection
    println!("Waiting for connection...");
    for i in 0..100 {
        if receiver_connected.load(Ordering::SeqCst) && sender_connected.load(Ordering::SeqCst) {
            println!("Both connected after {}ms", i * 100);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Send data
    println!("Sending data...");
    let test_data = b"Hello, RIST!";
    match sender.send(test_data) {
        Ok(n) => println!("Sent {} bytes", n),
        Err(e) => println!("Send error: {:?}", e),
    }

    // Try recv with increasing timeouts
    println!("Trying recv()...");
    for timeout in [100, 500, 1000, 2000, 5000] {
        match receiver.recv(timeout) {
            Ok(block) => {
                println!(
                    "recv() got {} bytes: {:?}",
                    block.payload().len(),
                    String::from_utf8_lossy(block.payload())
                );
                break;
            }
            Err(e) => println!("recv({}) error: {:?}", timeout, e),
        }
    }

    // Check callback count
    println!(
        "Data callback count: {}",
        data_received.load(Ordering::SeqCst)
    );

    // Keep alive briefly to see if callback fires
    std::thread::sleep(Duration::from_secs(2));
    println!(
        "Final callback count: {}",
        data_received.load(Ordering::SeqCst)
    );
}
