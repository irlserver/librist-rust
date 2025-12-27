//! Async receiver example using Tokio.
//!
//! This example demonstrates how to use the async wrapper for non-blocking
//! receive operations with Tokio.
//!
//! Run with:
//! ```sh
//! cargo run --example async_receiver --features async-tokio -- rist://@:5000
//! ```

#[cfg(not(feature = "async-tokio"))]
fn main() {
    eprintln!("This example requires the 'async-tokio' feature.");
    eprintln!("Run with: cargo run --example async_receiver --features async-tokio");
    std::process::exit(1);
}

#[cfg(feature = "async-tokio")]
mod async_example {
    use librist::{AsyncRistReceiver, Profile, RistReceiver};
    use std::env;
    use std::time::Duration;

    pub async fn run() -> librist::Result<()> {
        // Initialize logging
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

        // Parse command line arguments
        let args: Vec<String> = env::args().collect();
        let url = args.get(1).map(|s| s.as_str()).unwrap_or("rist://@:5000");

        println!("Async RIST Receiver Example");
        println!("============================");
        println!("Listening on: {}", url);
        println!();

        // Create the synchronous receiver
        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .on_connection(|peer_id, status| {
                println!("[Connection] Peer {}: {:?}", peer_id, status);
            })
            .build()?;

        receiver.add_peer(url)?;
        receiver.start()?;

        // Wrap in async receiver with a buffer of 1024 blocks
        let mut async_receiver = AsyncRistReceiver::new(receiver, 1024);

        println!("Receiver started, waiting for data...");
        println!("Press Ctrl+C to stop");
        println!();

        let mut total_bytes = 0u64;
        let mut total_packets = 0u64;

        // Receive loop using async/await
        loop {
            match async_receiver.recv_timeout(Duration::from_secs(5)).await {
                Ok(block) => {
                    total_bytes += block.payload().len() as u64;
                    total_packets += 1;

                    if total_packets % 100 == 0 {
                        println!(
                            "Received {} packets ({:.2} MB)",
                            total_packets,
                            total_bytes as f64 / 1_000_000.0
                        );
                    }
                }
                Err(librist::Error::Timeout) => {
                    println!("No data received in 5 seconds, still waiting...");
                }
                Err(e) => {
                    eprintln!("Error receiving data: {}", e);
                    break;
                }
            }
        }

        println!();
        println!(
            "Final stats: {} packets, {:.2} MB",
            total_packets,
            total_bytes as f64 / 1_000_000.0
        );

        Ok(())
    }
}

#[cfg(feature = "async-tokio")]
#[tokio::main]
async fn main() -> librist::Result<()> {
    async_example::run().await
}
