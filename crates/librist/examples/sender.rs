//! Simple RIST sender example
//!
//! This example demonstrates how to send data using the RIST protocol.
//!
//! Usage:
//!   cargo run --example sender -- rist://192.168.1.100:5000
//!
//! The sender will generate dummy data and send it to the specified receiver.

use librist::{LogLevel, Profile, RistSender, SenderStats};
use std::env;
use std::time::Duration;

fn main() -> librist::Result<()> {
    // Initialize logging
    env_logger::init();

    // Get destination URL from command line
    let args: Vec<String> = env::args().collect();
    let url = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("rist://127.0.0.1:5000");

    println!("librist version: {}", librist::version());
    println!("Sending to: {}", url);

    // Create a sender with stats callback
    let sender = RistSender::builder()
        .profile(Profile::Main)
        .log_level(LogLevel::Info)
        .on_stats(1000, |stats: &SenderStats| {
            println!(
                "Stats: peer={}, bandwidth={:.2} Mbps, rtt={}ms, quality={:.1}%",
                stats.peer_id,
                stats.bandwidth as f64 / 1_000_000.0 * 8.0,
                stats.rtt_ms,
                stats.quality
            );
        })
        .on_connection(|peer_id, status| {
            println!("Connection status: peer={}, status={:?}", peer_id, status);
        })
        .build()?;

    // Add destination peer
    let peer = sender.add_peer(url)?;
    println!("Added peer with ID: {}", peer.id());

    // Start the sender
    sender.start()?;
    println!("Sender started");

    // Generate and send dummy MPEG-TS packets
    let packet_size = 1316; // 7 MPEG-TS packets (188 * 7)
    let mut packet = vec![0u8; packet_size];
    let mut seq: u64 = 0;

    // Fill with MPEG-TS null packets pattern
    for i in 0..7 {
        let offset = i * 188;
        packet[offset] = 0x47; // Sync byte
        packet[offset + 1] = 0x1F; // PID high (null packet)
        packet[offset + 2] = 0xFF; // PID low (null packet)
        packet[offset + 3] = 0x10; // Adaptation field control
    }

    println!("Sending packets (Ctrl+C to stop)...");

    loop {
        // Update sequence in first packet
        packet[4..12].copy_from_slice(&seq.to_be_bytes());
        seq += 1;

        // Send the packet
        match sender.send(&packet) {
            Ok(bytes) => {
                if seq % 1000 == 0 {
                    println!("Sent {} packets ({} bytes each)", seq, bytes);
                }
            }
            Err(e) => {
                eprintln!("Send error: {}", e);
            }
        }

        // Rate limit to ~10 Mbps
        std::thread::sleep(Duration::from_micros(1000));
    }
}
