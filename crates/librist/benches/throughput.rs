//! Throughput benchmarks for librist-rust.
//!
//! Run with: cargo bench --package librist

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use librist::{LogLevel, Profile, RistReceiver, RistSender};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Port counter for benchmarks
static PORT_COUNTER: AtomicU64 = AtomicU64::new(40000);

fn get_bench_port() -> u16 {
    PORT_COUNTER.fetch_add(10, Ordering::SeqCst) as u16
}

/// Benchmark sending packets of various sizes
fn bench_send_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("send_throughput");
    group.measurement_time(Duration::from_secs(5));

    for size in [188, 1316, 4096, 8000].iter() {
        let port = get_bench_port();

        // Create receiver (required for sender to connect)
        let receiver = RistReceiver::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .build()
            .unwrap();
        receiver
            .add_peer(&format!("rist://@:{}?buffer=100", port))
            .unwrap();
        receiver.start().unwrap();

        // Create sender
        let sender = RistSender::builder()
            .profile(Profile::Main)
            .log_level(LogLevel::Disable)
            .build()
            .unwrap();
        sender
            .add_peer(&format!("rist://127.0.0.1:{}?buffer=100", port))
            .unwrap();
        sender.start().unwrap();

        // Wait for connection
        std::thread::sleep(Duration::from_millis(500));

        let data = vec![0xABu8; *size];

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                sender.send(black_box(&data)).unwrap();
            });
        });

        drop(sender);
        drop(receiver);
        std::thread::sleep(Duration::from_millis(100));
    }

    group.finish();
}

/// Benchmark receiving packets
fn bench_recv_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("recv_throughput");
    group.measurement_time(Duration::from_secs(5));

    let size = 1316; // Standard MPEG-TS bundle size
    let port = get_bench_port();

    // Create receiver
    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .log_level(LogLevel::Disable)
        .build()
        .unwrap();
    receiver
        .add_peer(&format!("rist://@:{}?buffer=100", port))
        .unwrap();
    receiver.start().unwrap();

    // Create sender
    let sender = RistSender::builder()
        .profile(Profile::Main)
        .log_level(LogLevel::Disable)
        .build()
        .unwrap();
    sender
        .add_peer(&format!("rist://127.0.0.1:{}?buffer=100", port))
        .unwrap();
    sender.start().unwrap();

    // Wait for connection
    std::thread::sleep(Duration::from_millis(500));

    let data = vec![0xABu8; size];

    // Pre-fill the buffer
    for _ in 0..100 {
        sender.send(&data).unwrap();
    }
    std::thread::sleep(Duration::from_millis(100));

    group.throughput(Throughput::Bytes(size as u64));
    group.bench_function("recv_1316", |b| {
        b.iter(|| {
            // Keep sending to replenish buffer
            let _ = sender.send(&data);
            if let Ok(block) = receiver.recv(100) {
                black_box(block);
            }
        });
    });

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

/// Benchmark with data callback (callback overhead)
fn bench_callback_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("callback_overhead");
    group.measurement_time(Duration::from_secs(5));

    let port = get_bench_port();
    let received = Arc::new(AtomicU64::new(0));
    let received_clone = received.clone();

    // Receiver with callback
    let receiver = RistReceiver::builder()
        .profile(Profile::Main)
        .log_level(LogLevel::Disable)
        .on_data(move |block| {
            received_clone.fetch_add(block.payload().len() as u64, Ordering::Relaxed);
        })
        .build()
        .unwrap();
    receiver
        .add_peer(&format!("rist://@:{}?buffer=100", port))
        .unwrap();
    receiver.start().unwrap();

    // Sender
    let sender = RistSender::builder()
        .profile(Profile::Main)
        .log_level(LogLevel::Disable)
        .build()
        .unwrap();
    sender
        .add_peer(&format!("rist://127.0.0.1:{}?buffer=100", port))
        .unwrap();
    sender.start().unwrap();

    std::thread::sleep(Duration::from_millis(500));

    let data = vec![0xABu8; 1316];

    group.throughput(Throughput::Bytes(1316));
    group.bench_function("send_with_callback", |b| {
        b.iter(|| {
            sender.send(black_box(&data)).unwrap();
        });
    });

    group.finish();
}

/// Benchmark DataBlock creation
fn bench_datablock(c: &mut Criterion) {
    use librist::DataBlock;

    let mut group = c.benchmark_group("datablock");

    let sizes = [188, 1316, 4096];

    for size in sizes {
        let data = vec![0xABu8; size];

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("create", size), &data, |b, data| {
            b.iter(|| {
                let block = DataBlock::new(black_box(data.clone()));
                black_box(block);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_send_throughput,
    bench_recv_throughput,
    bench_context_lifecycle,
    bench_callback_overhead,
    bench_datablock,
);
criterion_main!(benches);
