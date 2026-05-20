//! Throughput benchmarks for librist-rust.
//!
//! Run with: cargo bench --package librist
//!
//! Note: These benchmarks must run in sequence because librist has global state
//! (CTX_LIST in libevsocket.c) that persists across context lifetimes. We use
//! connection handshakes to ensure proper connection establishment, and lazy
//! initialization to avoid creating contexts until they're actually needed.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use librist::{ConnectionStatus, LogLevel, Profile, RistReceiver, RistSender};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::time::{Duration, Instant};

/// Port offset within this benchmark run
static PORT_OFFSET: AtomicU16 = AtomicU16::new(0);

/// Gets a unique port for benchmarking.
///
/// Uses PID and timestamp to generate a base port, ensuring different benchmark
/// runs use different port ranges. Within a run, allocates sequentially with
/// spacing of 10 (enough for RIST's port pairs).
fn get_bench_port() -> u16 {
    let pid = std::process::id() as u16;
    let time_component = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_nanos() as u16) & 0xFF)
        .unwrap_or(0);
    // Base port in range 35000-60000, offset by PID and time
    let base = 35000 + ((pid.wrapping_add(time_component)) % 2500) * 10;
    let offset = PORT_OFFSET.fetch_add(10, Ordering::SeqCst);
    base + offset
}

/// Wait for a condition with timeout
fn wait_for<F: Fn() -> bool>(condition: F, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Creates a connected sender/receiver pair, verifying the connection is established
fn create_bench_pair() -> (RistSender, RistReceiver, u16) {
    let port = get_bench_port();

    let receiver_ready = Arc::new(AtomicBool::new(false));
    let receiver_ready_clone = receiver_ready.clone();

    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
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
        .profile(Profile::Main)
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
    assert!(connected, "Connection not established within timeout");

    // Verify data path with handshake
    let handshake_magic = b"__BENCH_HANDSHAKE__";
    let handshake_ok = wait_for(
        || {
            let _ = sender.send(handshake_magic);
            std::thread::sleep(Duration::from_millis(50));
            matches!(receiver.try_recv(), Ok(Some(block)) if block.payload() == handshake_magic)
        },
        Duration::from_secs(10),
    );
    assert!(handshake_ok, "Data path handshake failed");

    // Drain any extra handshake packets
    while receiver.try_recv().is_ok_and(|b| b.is_some()) {}

    (sender, receiver, port)
}

/// Benchmark sending packets of various sizes
fn bench_send_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("send_throughput");
    group.measurement_time(Duration::from_secs(5));

    // Use a single sender/receiver pair for all sizes to avoid port reuse issues
    // Created lazily only when this benchmark actually runs
    let pair: std::cell::OnceCell<(RistSender, RistReceiver, u16)> = std::cell::OnceCell::new();

    for size in [188, 1316, 4096, 8000].iter() {
        let data = vec![0xABu8; *size];

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            let (sender, _, _) = pair.get_or_init(create_bench_pair);
            b.iter(|| {
                sender.send(black_box(&data)).unwrap();
            });
        });
    }

    // Cleanup if pair was initialized
    if let Some((sender, receiver, _)) = pair.into_inner() {
        drop(sender);
        drop(receiver);
        std::thread::sleep(Duration::from_millis(500));
    }

    group.finish();
}

/// Benchmark receiving packets
fn bench_recv_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("recv_throughput");
    group.measurement_time(Duration::from_secs(5));

    let size = 1316; // Standard MPEG-TS bundle size
    let data = vec![0xABu8; size];

    // Created lazily only when this benchmark actually runs
    let pair: std::cell::OnceCell<(RistSender, RistReceiver, u16)> = std::cell::OnceCell::new();
    let prefilled: std::cell::OnceCell<()> = std::cell::OnceCell::new();

    group.throughput(Throughput::Bytes(size as u64));
    group.bench_function("recv_1316", |b| {
        let (sender, receiver, _) = pair.get_or_init(create_bench_pair);

        // Pre-fill the buffer once
        prefilled.get_or_init(|| {
            for _ in 0..100 {
                let _ = sender.send(&data);
            }
            std::thread::sleep(Duration::from_millis(100));
        });

        b.iter(|| {
            // Keep sending to replenish buffer
            let _ = sender.send(&data);
            if let Ok(block) = receiver.recv(100) {
                black_box(block);
            }
        });
    });

    // Cleanup if pair was initialized
    if let Some((sender, receiver, _)) = pair.into_inner() {
        drop(sender);
        drop(receiver);
        // Longer wait for librist global state to fully clean up
        std::thread::sleep(Duration::from_secs(2));
    }

    group.finish();
}

/// Benchmark context creation/destruction
fn bench_context_lifecycle(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_lifecycle");

    group.bench_function("sender_create_destroy", |b| {
        b.iter(|| {
            let sender = RistSender::builder()
                .profile(Profile::Main)
                .log_level(LogLevel::Disable)
                .build()
                .unwrap();
            black_box(sender);
        });
    });

    group.bench_function("receiver_create_destroy", |b| {
        b.iter(|| {
            let receiver = RistReceiver::builder()
                .profile(Profile::Main)
                .log_level(LogLevel::Disable)
                .build()
                .unwrap();
            black_box(receiver);
        });
    });

    group.finish();
}

/// Benchmark DataBlockBuilder
fn bench_datablock(c: &mut Criterion) {
    use librist::DataBlockBuilder;

    let mut group = c.benchmark_group("datablock");

    let sizes = [188, 1316, 4096];

    for size in sizes {
        let data = vec![0xABu8; size];

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("builder", size), &data, |b, data| {
            b.iter(|| {
                let builder = DataBlockBuilder::new()
                    .virtual_dst_port(1234)
                    .timestamp_ntp(12345678);
                black_box(builder);
                black_box(data);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_send_throughput,
    bench_recv_throughput,
    // context_lifecycle should run last since it churns global state
    bench_context_lifecycle,
    bench_datablock,
);
criterion_main!(benches);
