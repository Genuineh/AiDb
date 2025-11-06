//! # MemTable Example
//!
//! This example demonstrates how to use the MemTable API.

use aidb::memtable::MemTable;
use std::sync::Arc;
use std::thread;

fn main() {
    println!("=== MemTable Example ===\n");

    // Example 1: Basic Operations
    basic_operations();

    // Example 2: MVCC Semantics
    mvcc_example();

    // Example 3: Iterator
    iterator_example();

    // Example 4: Concurrent Access
    concurrent_example();
}

fn basic_operations() {
    println!("1. Basic Operations");
    println!("-------------------");

    let memtable = MemTable::new(1);

    // Put operations
    memtable.put(b"name", b"Alice", 1);
    memtable.put(b"age", b"30", 2);
    memtable.put(b"city", b"New York", 3);

    // Get operations
    if let Some(value) = memtable.get(b"name", 100) {
        println!("name = {}", String::from_utf8_lossy(&value));
    }

    if let Some(value) = memtable.get(b"age", 100) {
        println!("age = {}", String::from_utf8_lossy(&value));
    }

    // Delete operation
    memtable.delete(b"city", 4);
    println!("city deleted: {:?}", memtable.get(b"city", 100));

    // Size statistics
    println!("MemTable size: {} bytes", memtable.approximate_size());
    println!("Entry count: {}", memtable.len());
    println!();
}

fn mvcc_example() {
    println!("2. MVCC Semantics");
    println!("------------------");

    let memtable = MemTable::new(1);

    // Write multiple versions of the same key
    memtable.put(b"counter", b"1", 1);
    memtable.put(b"counter", b"2", 2);
    memtable.put(b"counter", b"3", 3);

    // Read different versions using sequence numbers
    println!(
        "counter @ seq 1 = {}",
        String::from_utf8_lossy(&memtable.get(b"counter", 1).unwrap())
    );
    println!(
        "counter @ seq 2 = {}",
        String::from_utf8_lossy(&memtable.get(b"counter", 2).unwrap())
    );
    println!(
        "counter @ seq 3 = {}",
        String::from_utf8_lossy(&memtable.get(b"counter", 3).unwrap())
    );

    // Delete at sequence 4
    memtable.delete(b"counter", 4);

    // Earlier snapshots still see the value
    println!(
        "counter @ seq 3 (after delete) = {}",
        String::from_utf8_lossy(&memtable.get(b"counter", 3).unwrap())
    );

    // But seq 4+ sees deletion
    println!("counter @ seq 4 (after delete) = {:?}", memtable.get(b"counter", 4));
    println!();
}

fn iterator_example() {
    println!("3. Iterator Example");
    println!("-------------------");

    let memtable = MemTable::new(1);

    // Insert some data
    memtable.put(b"apple", b"red", 1);
    memtable.put(b"banana", b"yellow", 2);
    memtable.put(b"cherry", b"red", 3);
    memtable.put(b"date", b"brown", 4);

    println!("All entries:");
    for entry in memtable.iter() {
        println!(
            "  {} = {} (seq: {})",
            String::from_utf8_lossy(entry.user_key()),
            String::from_utf8_lossy(entry.value()),
            entry.sequence()
        );
    }
    println!();
}

fn concurrent_example() {
    println!("4. Concurrent Access");
    println!("--------------------");

    let memtable = Arc::new(MemTable::new(1));
    let num_threads = 4;
    let ops_per_thread = 100;

    println!("Spawning {} writer threads...", num_threads);

    // Spawn multiple writer threads
    let mut handles = vec![];
    for thread_id in 0..num_threads {
        let mt = memtable.clone();
        let handle = thread::spawn(move || {
            for i in 0..ops_per_thread {
                let seq = (thread_id * ops_per_thread + i) as u64;
                let key = format!("key_{}", seq);
                let value = format!("value_{}", seq);
                mt.put(key.as_bytes(), value.as_bytes(), seq);
            }
        });
        handles.push(handle);
    }

    // Wait for all writers to complete
    for handle in handles {
        handle.join().unwrap();
    }

    println!("All threads completed!");
    println!("Final entry count: {}", memtable.len());
    println!("Final size: {} bytes", memtable.approximate_size());

    // Verify some entries
    println!("\nVerifying some entries:");
    for i in [0, 50, 100, 200, 399] {
        let key = format!("key_{}", i);
        let expected = format!("value_{}", i);
        let actual = memtable.get(key.as_bytes(), u64::MAX).unwrap();
        assert_eq!(actual, expected.as_bytes());
        println!("  ✓ {} = {}", key, String::from_utf8_lossy(&actual));
    }
    println!();
}
