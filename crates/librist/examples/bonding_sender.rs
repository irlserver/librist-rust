//! RIST bonding sender example
//!
//! This example demonstrates how to use RIST bonding to send data over
//! multiple network paths for redundancy and increased reliability.
//!
//! Usage:
//!   cargo run --example bonding_sender -- rist://192.168.1.100:5000 rist://10.0.0.100:5000
//!
//! Bonding modes:
//! - weight=0: Duplicate mode - packets sent on all paths (maximum redundancy)
//! - weight>0: Load balancing - packets distributed based on weight ratios

use librist::{LogLevel, Profile, RistSender, SenderStats};
use std::env;
use std::time::Duration;

fn main() -> librist::Result<()> {
    // Initialize logging
    env_logger::init();

    // Get destination URLs from command line
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: {} <url1> <url2> [url3...]", args[0]);
        eprintln!(
            "Example: {} rist://192.168.1.100:5000 rist://10.0.0.100:5000",
            args[0]
        );
        eprintln!();
        eprintln!("URL options for bonding:");
        eprintln!("  ?weight=0    Duplicate mode (default) - send on all paths");
        eprintln!("  ?weight=5    Load balance with weight 5");
        std::process::exit(1);
    }

    let urls: Vec<&str> = args[1..].iter().map(|s| s.as_str()).collect();

    println!("librist version: {}", librist::version());
    println!("Bonding sender with {} paths:", urls.len());
    for (i, url) in urls.iter().enumerate() {
        println!("  Path {}: {}", i + 1, url);
    }

    // Create a sender with stats callback
    let sender = RistSender::builder()
        .profile(Profile::Main)
        .log_level(LogLevel::Info)
        .on_stats(1000, |stats: &SenderStats| {
            println!(
                "Stats: peer={}, sent={}, retrans={}, bandwidth={:.2} Mbps, rtt={}ms, quality={:.1}%",
                stats.peer_id,
                stats.sent_packets,
                stats.retransmitted_packets,
                stats.bandwidth as f64 / 1_000_000.0 * 8.0,
                stats.rtt_ms,
                stats.quality
            );
        })
        .on_connection(|peer_id, status| {
            println!("Connection status: peer={}, status={:?}", peer_id, status);
        })
        .build()?;

    // Add all destination peers
    for (i, url) in urls.iter().enumerate() {
        let peer = sender.add_peer(url)?;
        println!("Added peer {} with ID: {} ({})", i + 1, peer.id(), url);
    }

    // Start the sender
    sender.start()?;
    println!("Bonding sender started with {} paths", urls.len());

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

    println!("Sending packets via bonded paths (Ctrl+C to stop)...");

    loop {
        // Update sequence in first packet
        packet[4..12].copy_from_slice(&seq.to_be_bytes());
        seq += 1;

        // Send the packet - RIST will handle distribution across bonded paths
        match sender.send(&packet) {
            Ok(bytes) => {
                if seq % 1000 == 0 {
                    println!(
                        "Sent {} packets ({} bytes each) via {} bonded paths",
                        seq,
                        bytes,
                        urls.len()
                    );
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
