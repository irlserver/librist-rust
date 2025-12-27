//! RIST statistics monitoring example
//!
//! This example demonstrates how to monitor RIST statistics in real-time.
//! It creates a receiver that logs detailed statistics at regular intervals.
//!
//! Usage:
//!   cargo run --example stats_monitor -- rist://@:5000
//!
//! The monitor will display:
//! - Bandwidth usage
//! - Packet loss and recovery rates
//! - Quality percentage
//! - RTT (Round Trip Time)

use librist::{LogLevel, Profile, ReceiverStats, RistReceiver};
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

fn main() -> librist::Result<()> {
    // Initialize logging
    env_logger::init();

    // Get listen URL from command line
    let args: Vec<String> = env::args().collect();
    let url = args.get(1).map(|s| s.as_str()).unwrap_or("rist://@:5000");

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║             RIST Statistics Monitor                          ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("librist version: {}", librist::version());
    println!("Listening on: {}", url);
    println!();

    // Track cumulative statistics
    let total_packets = Arc::new(AtomicU64::new(0));
    let total_lost = Arc::new(AtomicU64::new(0));
    let total_recovered = Arc::new(AtomicU64::new(0));
    let has_connection = Arc::new(AtomicBool::new(false));

    let total_packets_stats = Arc::clone(&total_packets);
    let total_lost_stats = Arc::clone(&total_lost);
    let total_recovered_stats = Arc::clone(&total_recovered);
    let has_connection_stats = Arc::clone(&has_connection);

    // Create a receiver with detailed stats callback
    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .log_level(LogLevel::Warn) // Only log warnings and errors from librist
        .on_stats(1000, move |stats: &ReceiverStats| {
            has_connection_stats.store(true, Ordering::SeqCst);

            // Update cumulative counters
            total_packets_stats.fetch_add(stats.received_packets, Ordering::Relaxed);
            total_lost_stats.fetch_add(stats.lost_packets as u64, Ordering::Relaxed);
            total_recovered_stats.fetch_add(stats.recovered_packets as u64, Ordering::Relaxed);

            // Calculate recovery efficiency
            let recovery_rate = if stats.lost_packets > 0 {
                (stats.recovered_packets as f64 / stats.lost_packets as f64 * 100.0).min(100.0)
            } else {
                100.0
            };

            // Convert nanoseconds to milliseconds for display
            let min_spacing_ms = stats.min_inter_packet_spacing_ns as f64 / 1_000_000.0;
            let max_spacing_ms = stats.max_inter_packet_spacing_ns as f64 / 1_000_000.0;

            // Print formatted stats
            println!("┌─────────────────────────────────────────────────────────────┐");
            println!(
                "│ Flow ID: {:10} │ Quality: {:6.2}%                      │",
                stats.flow_id, stats.quality
            );
            println!("├─────────────────────────────────────────────────────────────┤");
            println!(
                "│ Bandwidth:    {:8.2} Mbps                                 │",
                stats.bandwidth as f64 / 1_000_000.0 * 8.0
            );
            println!(
                "│ Received:     {:8} packets                              │",
                stats.received_packets
            );
            println!(
                "│ Lost:         {:8} packets                              │",
                stats.lost_packets
            );
            println!(
                "│ Recovered:    {:8} packets ({:5.1}% recovery rate)       │",
                stats.recovered_packets, recovery_rate
            );
            println!(
                "│ RTT:          {:8} ms                                    │",
                stats.rtt_ms
            );
            println!(
                "│ Spacing:      {:8.2} ms (min), {:8.2} ms (max)           │",
                min_spacing_ms, max_spacing_ms
            );
            println!("└─────────────────────────────────────────────────────────────┘");
            println!();
        })
        .on_connection(|peer_id, status| {
            println!(
                ">>> Connection event: peer={}, status={:?}",
                peer_id, status
            );
        })
        .on_auth_connect(|conn_ip, conn_port, _local_ip, _local_port, peer_id| {
            println!(
                ">>> New connection from {}:{} (peer_id={})",
                conn_ip, conn_port, peer_id
            );
            true // Accept all connections
        })
        .on_auth_disconnect(|peer_id| {
            println!(">>> Peer {} disconnected", peer_id);
        })
        .build()?;

    // Add listener peer
    let peer = receiver.add_peer(url)?;
    println!("Added listener with ID: {}", peer.id());
    println!();
    println!("Waiting for incoming RIST stream...");
    println!("(Stats will appear when data is received)");
    println!();

    // Start the receiver
    receiver.start()?;

    let start_time = Instant::now();

    // Main loop - just receive data and let callbacks handle stats
    loop {
        match receiver.recv(1000) {
            Ok(block) => {
                total_packets.fetch_add(1, Ordering::Relaxed);

                // Check for issues
                if block.is_discontinuity() {
                    println!("!!! DISCONTINUITY detected at seq={}", block.sequence());
                }
                if block.is_overflow() {
                    println!("!!! BUFFER OVERFLOW detected");
                }
            }
            Err(librist::Error::Timeout) => {
                // Print waiting message if no connection yet
                if !has_connection.load(Ordering::SeqCst) {
                    let elapsed = start_time.elapsed().as_secs();
                    if elapsed > 0 && elapsed % 5 == 0 {
                        println!("Still waiting for data... ({} seconds)", elapsed);
                    }
                }
            }
            Err(e) => {
                eprintln!("Receive error: {}", e);
            }
        }

        // Print cumulative stats every 30 seconds
        let elapsed = start_time.elapsed();
        if elapsed.as_secs() > 0 && elapsed.as_secs() % 30 == 0 {
            let packets = total_packets.load(Ordering::Relaxed);
            let lost = total_lost.load(Ordering::Relaxed);
            let recovered = total_recovered.load(Ordering::Relaxed);

            if packets > 0 {
                let loss_rate = lost as f64 / packets as f64 * 100.0;
                println!("═══════════════════════════════════════════════════════════════");
                println!("CUMULATIVE STATS ({}s elapsed)", elapsed.as_secs());
                println!("  Total packets: {}", packets);
                println!("  Total lost: {} ({:.4}%)", lost, loss_rate);
                println!("  Total recovered: {}", recovered);
                println!("═══════════════════════════════════════════════════════════════");
                println!();
            }
        }

        std::thread::sleep(Duration::from_millis(1));
    }
}
