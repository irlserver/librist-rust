//! Simple RIST receiver example
//!
//! This example demonstrates how to receive data using the RIST protocol.
//!
//! Usage:
//!   cargo run --example receiver -- rist://@:5000
//!
//! The receiver will listen on the specified port and print received data stats.

use librist::{LogLevel, PeerInfo, Profile, ReceiverStats, RistReceiver};
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

fn main() -> librist::Result<()> {
    // Initialize logging
    env_logger::init();

    // Get listen URL from command line
    let args: Vec<String> = env::args().collect();
    let url = args.get(1).map(|s| s.as_str()).unwrap_or("rist://@:5000");

    println!("librist version: {}", librist::version());
    println!("Listening on: {}", url);

    // Track received bytes
    let total_bytes = Arc::new(AtomicU64::new(0));
    let total_packets = Arc::new(AtomicU64::new(0));

    // Create a receiver with stats and auth callbacks
    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .log_level(LogLevel::Info)
        .on_stats(1000, |stats: &ReceiverStats| {
            println!(
                "Stats: flow={}, bandwidth={:.2} Mbps, quality={:.1}%, lost={}, recovered={}",
                stats.flow_id,
                stats.bandwidth as f64 / 1_000_000.0 * 8.0,
                stats.quality,
                stats.lost_packets,
                stats.recovered_packets
            );
        })
        .on_connection(|peer_id, status| {
            println!("Connection status: peer={}, status={:?}", peer_id, status);
        })
        .on_auth_connect(|conn_ip, conn_port, local_ip, local_port, peer: &PeerInfo| {
            println!(
                "Auth: incoming connection from {}:{} to {}:{} ({})",
                conn_ip, conn_port, local_ip, local_port, peer
            );
            // Log CNAME if available (user-configurable identifier)
            if let Some(ref cname) = peer.cname {
                println!("  CNAME: {}", cname);
            }
            // Accept all connections - return false to reject
            true
        })
        .on_auth_disconnect(|peer: &PeerInfo| {
            println!("Auth: {} disconnected", peer);
        })
        .build()?;

    // Add listener peer
    let peer = receiver.add_peer(url)?;
    println!("Added listener with ID: {}", peer.id());

    // Start the receiver
    receiver.start()?;
    println!("Receiver started, waiting for data...");

    let start_time = Instant::now();
    let mut last_report = Instant::now();

    loop {
        // Try to receive data with 100ms timeout
        match receiver.recv(100) {
            Ok(block) => {
                let payload = block.payload();
                total_bytes.fetch_add(payload.len() as u64, Ordering::Relaxed);
                total_packets.fetch_add(1, Ordering::Relaxed);

                // Check for discontinuity
                if block.is_discontinuity() {
                    println!(
                        "Warning: Discontinuity detected at seq={}, flow={}",
                        block.sequence(),
                        block.flow_id()
                    );
                }

                // Periodic report
                if last_report.elapsed() >= Duration::from_secs(5) {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let bytes = total_bytes.load(Ordering::Relaxed);
                    let packets = total_packets.load(Ordering::Relaxed);
                    let bitrate = (bytes as f64 * 8.0) / elapsed / 1_000_000.0;

                    println!(
                        "Received: {} packets, {:.2} MB, avg bitrate: {:.2} Mbps",
                        packets,
                        bytes as f64 / 1_000_000.0,
                        bitrate
                    );
                    last_report = Instant::now();
                }
            }
            Err(librist::Error::Timeout) => {
                // No data available, continue waiting
            }
            Err(e) => {
                eprintln!("Receive error: {}", e);
            }
        }
    }
}
