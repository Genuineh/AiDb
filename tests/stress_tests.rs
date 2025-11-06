// Stress Tests for AiDb
// These tests are marked with #[ignore] and are intended to be run manually
// Run with: cargo test --release -- --ignored --nocapture

use aidb::{Options, DB};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// High-frequency write stress test (100k ops/s target)
#[test]
#[ignore]
fn stress_high_frequency_writes() {
    let dir = TempDir::new().unwrap();

    let options = Options {
        memtable_size: 1024 * 1024 * 4, // 4MB memtable
        ..Default::default()
    };

    let db = Arc::new(DB::open(dir.path(), options).unwrap());

    let duration = Duration::from_secs(60); // 1 minute stress test
    let start = Instant::now();
    let operations = Arc::new(AtomicUsize::new(0));
    let stop_flag = Arc::new(AtomicBool::new(false));

    // Spawn multiple writer threads
    let num_threads = 8;
    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let ops_clone = Arc::clone(&operations);
        let stop_clone = Arc::clone(&stop_flag);

        let handle = thread::spawn(move || {
            let mut local_ops = 0;
            while !stop_clone.load(Ordering::Relaxed) {
                let key = format!("stress_key_{}_{}", thread_id, local_ops);
                let value = format!("stress_value_{}", local_ops);

                if db_clone.put(key.as_bytes(), value.as_bytes()).is_ok() {
                    local_ops += 1;
                    ops_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    // Run for specified duration
    thread::sleep(duration);
    stop_flag.store(true, Ordering::Relaxed);

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    let elapsed = start.elapsed();
    let total_ops = operations.load(Ordering::Relaxed);
    let ops_per_sec = total_ops as f64 / elapsed.as_secs_f64();

    println!("=== High-Frequency Write Stress Test ===");
    println!("Duration: {:.2}s", elapsed.as_secs_f64());
    println!("Total operations: {}", total_ops);
    println!("Throughput: {:.0} ops/s", ops_per_sec);
    println!("Target: 100,000 ops/s");

    if ops_per_sec >= 100_000.0 {
        println!("✅ Target achieved!");
    } else {
        println!("⚠️  Below target ({:.1}% of target)", (ops_per_sec / 100_000.0) * 100.0);
    }
}

/// High-frequency read stress test
#[test]
#[ignore]
fn stress_high_frequency_reads() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Prepare data
    println!("Preparing data...");
    for i in 0..10_000 {
        let key = format!("read_key_{}", i);
        let value = format!("read_value_{}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }
    println!("Data preparation complete");

    let duration = Duration::from_secs(60);
    let start = Instant::now();
    let operations = Arc::new(AtomicUsize::new(0));
    let stop_flag = Arc::new(AtomicBool::new(false));

    let num_threads = 16; // More readers
    let mut handles = vec![];

    for _ in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let ops_clone = Arc::clone(&operations);
        let stop_clone = Arc::clone(&stop_flag);

        let handle = thread::spawn(move || {
            let mut local_ops = 0;
            while !stop_clone.load(Ordering::Relaxed) {
                let key = format!("read_key_{}", local_ops % 10_000);

                if db_clone.get(key.as_bytes()).is_ok() {
                    local_ops += 1;
                    ops_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    thread::sleep(duration);
    stop_flag.store(true, Ordering::Relaxed);

    for handle in handles {
        handle.join().unwrap();
    }

    let elapsed = start.elapsed();
    let total_ops = operations.load(Ordering::Relaxed);
    let ops_per_sec = total_ops as f64 / elapsed.as_secs_f64();

    println!("=== High-Frequency Read Stress Test ===");
    println!("Duration: {:.2}s", elapsed.as_secs_f64());
    println!("Total operations: {}", total_ops);
    println!("Throughput: {:.0} ops/s", ops_per_sec);
}

/// Mixed read/write workload stress test
#[test]
#[ignore]
fn stress_mixed_workload() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    let duration = Duration::from_secs(120); // 2 minutes
    let start = Instant::now();
    let read_ops = Arc::new(AtomicUsize::new(0));
    let write_ops = Arc::new(AtomicUsize::new(0));
    let stop_flag = Arc::new(AtomicBool::new(false));

    let num_writers = 4;
    let num_readers = 12;
    let mut handles = vec![];

    // Writer threads
    for thread_id in 0..num_writers {
        let db_clone = Arc::clone(&db);
        let ops_clone = Arc::clone(&write_ops);
        let stop_clone = Arc::clone(&stop_flag);

        let handle = thread::spawn(move || {
            let mut local_ops = 0;
            while !stop_clone.load(Ordering::Relaxed) {
                let key = format!("mixed_key_{}_{}", thread_id, local_ops);
                let value = vec![b'x'; 100];

                if db_clone.put(key.as_bytes(), &value).is_ok() {
                    local_ops += 1;
                    ops_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    // Reader threads
    for thread_id in 0..num_readers {
        let db_clone = Arc::clone(&db);
        let ops_clone = Arc::clone(&read_ops);
        let stop_clone = Arc::clone(&stop_flag);

        let handle = thread::spawn(move || {
            let mut local_ops = 0;
            while !stop_clone.load(Ordering::Relaxed) {
                let key = format!("mixed_key_{}_{}", thread_id % 4, local_ops);

                if db_clone.get(key.as_bytes()).is_ok() {
                    local_ops += 1;
                    ops_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    thread::sleep(duration);
    stop_flag.store(true, Ordering::Relaxed);

    for handle in handles {
        handle.join().unwrap();
    }

    let elapsed = start.elapsed();
    let total_reads = read_ops.load(Ordering::Relaxed);
    let total_writes = write_ops.load(Ordering::Relaxed);

    println!("=== Mixed Workload Stress Test ===");
    println!("Duration: {:.2}s", elapsed.as_secs_f64());
    println!(
        "Read operations: {} ({:.0} ops/s)",
        total_reads,
        total_reads as f64 / elapsed.as_secs_f64()
    );
    println!(
        "Write operations: {} ({:.0} ops/s)",
        total_writes,
        total_writes as f64 / elapsed.as_secs_f64()
    );
    println!("Total operations: {}", total_reads + total_writes);
}

/// Memory pressure stress test
#[test]
#[ignore]
fn stress_memory_pressure() {
    let dir = TempDir::new().unwrap();

    let options = Options {
        memtable_size: 1024 * 1024 * 2, // 2MB memtable
        ..Default::default()
    };

    let db = DB::open(dir.path(), options).unwrap();

    println!("=== Memory Pressure Stress Test ===");
    println!("Writing large dataset to trigger multiple flushes...");

    let num_records = 100_000;
    let value_size = 1024; // 1KB values

    let start = Instant::now();

    for i in 0..num_records {
        let key = format!("mem_key_{:08}", i);
        let value = vec![b'x'; value_size];

        db.put(key.as_bytes(), &value).unwrap();

        if i % 10_000 == 0 {
            println!("Progress: {}/{} records", i, num_records);
        }
    }

    let write_duration = start.elapsed();

    println!("Write complete. Verifying data...");

    let verify_start = Instant::now();
    let sample_rate = 100;

    for i in (0..num_records).step_by(sample_rate) {
        let key = format!("mem_key_{:08}", i);
        let result = db.get(key.as_bytes()).unwrap();
        assert!(result.is_some(), "Key {} not found", key);
        assert_eq!(result.unwrap().len(), value_size);
    }

    let verify_duration = verify_start.elapsed();

    println!("Write time: {:.2}s", write_duration.as_secs_f64());
    println!("Verify time: {:.2}s", verify_duration.as_secs_f64());
    println!(
        "Write throughput: {:.0} ops/s",
        num_records as f64 / write_duration.as_secs_f64()
    );
}

/// Large value stress test (1MB+ values)
#[test]
#[ignore]
fn stress_large_values() {
    let dir = TempDir::new().unwrap();
    let db = DB::open(dir.path(), Options::default()).unwrap();

    println!("=== Large Value Stress Test ===");

    let num_records = 100;
    let value_size = 1024 * 1024; // 1MB

    let start = Instant::now();

    for i in 0..num_records {
        let key = format!("large_key_{}", i);
        let value = vec![b'x'; value_size];

        db.put(key.as_bytes(), &value).unwrap();

        if i % 10 == 0 {
            println!("Progress: {}/{} large records", i, num_records);
        }
    }

    let write_duration = start.elapsed();

    println!("Verifying large values...");

    let verify_start = Instant::now();

    for i in 0..num_records {
        let key = format!("large_key_{}", i);
        let result = db.get(key.as_bytes()).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), value_size);
    }

    let verify_duration = verify_start.elapsed();

    println!("Write time: {:.2}s", write_duration.as_secs_f64());
    println!("Verify time: {:.2}s", verify_duration.as_secs_f64());
    println!("Total data written: {} MB", (num_records * value_size) / (1024 * 1024));
}

/// Long-running stress test (1+ hour)
#[test]
#[ignore]
fn stress_long_running() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    println!("=== Long-Running Stress Test ===");
    println!("This test will run for 1+ hour...");

    let duration = Duration::from_secs(3600); // 1 hour
    let start = Instant::now();
    let operations = Arc::new(AtomicUsize::new(0));
    let stop_flag = Arc::new(AtomicBool::new(false));

    let num_threads = 8;
    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let ops_clone = Arc::clone(&operations);
        let stop_clone = Arc::clone(&stop_flag);

        let handle = thread::spawn(move || {
            let mut local_ops = 0;
            while !stop_clone.load(Ordering::Relaxed) {
                // Mix of operations
                if local_ops % 3 == 0 {
                    // Write
                    let key = format!("long_key_{}_{}", thread_id, local_ops);
                    let value = vec![b'x'; 100];
                    let _ = db_clone.put(key.as_bytes(), &value);
                } else {
                    // Read
                    let key = format!("long_key_{}_{}", thread_id, local_ops / 2);
                    let _ = db_clone.get(key.as_bytes());
                }

                local_ops += 1;
                ops_clone.fetch_add(1, Ordering::Relaxed);

                // Occasional sleep to reduce pressure
                if local_ops % 1000 == 0 {
                    thread::sleep(Duration::from_millis(10));
                }
            }
        });

        handles.push(handle);
    }

    // Progress reporting thread
    let ops_reporter = Arc::clone(&operations);
    let stop_reporter = Arc::clone(&stop_flag);
    let reporter = thread::spawn(move || {
        while !stop_reporter.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_secs(60));
            let ops = ops_reporter.load(Ordering::Relaxed);
            println!("Progress: {} operations completed", ops);
        }
    });

    thread::sleep(duration);
    stop_flag.store(true, Ordering::Relaxed);

    for handle in handles {
        handle.join().unwrap();
    }
    reporter.join().unwrap();

    let elapsed = start.elapsed();
    let total_ops = operations.load(Ordering::Relaxed);

    println!("=== Long-Running Test Complete ===");
    println!("Duration: {:.2} hours", elapsed.as_secs_f64() / 3600.0);
    println!("Total operations: {}", total_ops);
    println!("Average throughput: {:.0} ops/s", total_ops as f64 / elapsed.as_secs_f64());
}

/// Disk space stress test
#[test]
#[ignore]
fn stress_disk_space() {
    let dir = TempDir::new().unwrap();

    let options = Options {
        memtable_size: 1024 * 1024, // 1MB memtable for more flushes
        ..Default::default()
    };

    let db = DB::open(dir.path(), options).unwrap();

    println!("=== Disk Space Stress Test ===");
    println!("Writing large dataset to consume disk space...");

    let target_size_mb = 1000; // 1GB target
    let value_size = 1024; // 1KB values
    let num_records = (target_size_mb * 1024 * 1024) / value_size;

    let start = Instant::now();

    for i in 0..num_records {
        let key = format!("disk_key_{:08}", i);
        let value = vec![(i % 256) as u8; value_size];

        db.put(key.as_bytes(), &value).unwrap();

        if i % 50_000 == 0 {
            println!("Progress: {} MB written", (i * value_size) / (1024 * 1024));
        }
    }

    let duration = start.elapsed();

    println!("Write complete");
    println!("Duration: {:.2}s", duration.as_secs_f64());
    println!("Data written: {} MB", (num_records * value_size) / (1024 * 1024));
    println!(
        "Throughput: {:.0} MB/s",
        ((num_records * value_size) as f64 / (1024.0 * 1024.0)) / duration.as_secs_f64()
    );
}
