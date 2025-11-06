// Concurrent Access Tests for AiDb
// These tests verify thread-safety and concurrent access patterns

use aidb::{Options, DB};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

/// Test concurrent writes from multiple threads
#[test]
fn test_concurrent_writes() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    let num_threads = 10;
    let writes_per_thread = 100;

    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let handle = thread::spawn(move || {
            for i in 0..writes_per_thread {
                let key = format!("thread_{}_key_{}", thread_id, i);
                let value = format!("thread_{}_value_{}", thread_id, i);
                db_clone.put(key.as_bytes(), value.as_bytes()).unwrap();
            }
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify all writes succeeded
    for thread_id in 0..num_threads {
        for i in 0..writes_per_thread {
            let key = format!("thread_{}_key_{}", thread_id, i);
            let expected = format!("thread_{}_value_{}", thread_id, i);
            assert_eq!(
                db.get(key.as_bytes()).unwrap(),
                Some(expected.as_bytes().to_vec())
            );
        }
    }
}

/// Test concurrent reads from multiple threads
#[test]
fn test_concurrent_reads() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Prepare data
    for i in 0..1000 {
        let key = format!("read_key_{}", i);
        let value = format!("read_value_{}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }

    let num_threads = 20;
    let reads_per_thread = 100;

    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let handle = thread::spawn(move || {
            for i in 0..reads_per_thread {
                let key = format!("read_key_{}", i);
                let expected = format!("read_value_{}", i);
                let result = db_clone.get(key.as_bytes()).unwrap();
                assert_eq!(
                    result,
                    Some(expected.as_bytes().to_vec()),
                    "Thread {} failed reading key {}",
                    thread_id,
                    key
                );
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }
}

/// Test mixed concurrent reads and writes
#[test]
fn test_concurrent_reads_and_writes() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Initial data
    for i in 0..100 {
        let key = format!("initial_key_{}", i);
        db.put(key.as_bytes(), b"initial_value").unwrap();
    }

    let num_readers = 10;
    let num_writers = 5;
    let barrier = Arc::new(Barrier::new(num_readers + num_writers));

    let mut handles = vec![];

    // Spawn writer threads
    for writer_id in 0..num_writers {
        let db_clone = Arc::clone(&db);
        let barrier_clone = Arc::clone(&barrier);
        let handle = thread::spawn(move || {
            barrier_clone.wait(); // Synchronize start

            for i in 0..50 {
                let key = format!("writer_{}_key_{}", writer_id, i);
                let value = format!("writer_{}_value_{}", writer_id, i);
                db_clone.put(key.as_bytes(), value.as_bytes()).unwrap();

                // Occasionally sleep to create interleaving
                if i % 10 == 0 {
                    thread::sleep(Duration::from_micros(100));
                }
            }
        });
        handles.push(handle);
    }

    // Spawn reader threads
    for reader_id in 0..num_readers {
        let db_clone = Arc::clone(&db);
        let barrier_clone = Arc::clone(&barrier);
        let handle = thread::spawn(move || {
            barrier_clone.wait(); // Synchronize start

            for i in 0..100 {
                // Read initial data (should always be present)
                let key = format!("initial_key_{}", i % 100);
                let result = db_clone.get(key.as_bytes()).unwrap();
                assert!(
                    result.is_some(),
                    "Reader {} failed to find initial_key_{}",
                    reader_id,
                    i % 100
                );

                // Try reading writer data (may or may not exist yet)
                for writer_id in 0..5 {
                    let key = format!("writer_{}_key_{}", writer_id, i % 50);
                    let _ = db_clone.get(key.as_bytes());
                }
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify all writes completed
    for writer_id in 0..num_writers {
        for i in 0..50 {
            let key = format!("writer_{}_key_{}", writer_id, i);
            let expected = format!("writer_{}_value_{}", writer_id, i);
            assert_eq!(
                db.get(key.as_bytes()).unwrap(),
                Some(expected.as_bytes().to_vec())
            );
        }
    }
}

/// Test concurrent writes to the same key
#[test]
fn test_concurrent_writes_same_key() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    let num_threads = 20;
    let writes_per_thread = 100;
    let counter = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let counter_clone = Arc::clone(&counter);
        let handle = thread::spawn(move || {
            for _ in 0..writes_per_thread {
                let value = counter_clone.fetch_add(1, Ordering::SeqCst);
                let value_str = format!("value_{}", value);
                db_clone
                    .put(b"shared_key", value_str.as_bytes())
                    .unwrap();
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Key should have some value (the last write wins)
    let result = db.get(b"shared_key").unwrap();
    assert!(result.is_some(), "Shared key should have a value");

    // Total writes should match
    assert_eq!(
        counter.load(Ordering::SeqCst),
        num_threads * writes_per_thread
    );
}

/// Test concurrent deletes
#[test]
fn test_concurrent_deletes() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Prepare data
    for i in 0..1000 {
        let key = format!("delete_key_{}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }

    let num_threads = 10;
    let deletes_per_thread = 100;

    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let handle = thread::spawn(move || {
            let start = thread_id * deletes_per_thread;
            let end = start + deletes_per_thread;

            for i in start..end {
                let key = format!("delete_key_{}", i);
                db_clone.delete(key.as_bytes()).unwrap();
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify deletions
    for i in 0..1000 {
        let key = format!("delete_key_{}", i);
        let result = db.get(key.as_bytes()).unwrap();

        if i < num_threads * deletes_per_thread {
            assert_eq!(result, None, "Key {} should be deleted", key);
        } else {
            assert_eq!(
                result,
                Some(b"value".to_vec()),
                "Key {} should still exist",
                key
            );
        }
    }
}

/// Test concurrent writes during flush
#[test]
fn test_concurrent_writes_during_flush() {
    let dir = TempDir::new().unwrap();

    let mut options = Options::default();
    options.memtable_size = 1024 * 128; // 128KB to trigger flushes

    let db = Arc::new(DB::open(dir.path(), options).unwrap());

    let num_threads = 5;
    let writes_per_thread = 1000;

    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let handle = thread::spawn(move || {
            for i in 0..writes_per_thread {
                let key = format!("flush_thread_{}_key_{}", thread_id, i);
                let value = vec![b'x'; 200]; // 200 bytes
                db_clone.put(key.as_bytes(), &value).unwrap();
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify all data
    for thread_id in 0..num_threads {
        for i in 0..writes_per_thread {
            let key = format!("flush_thread_{}_key_{}", thread_id, i);
            let result = db.get(key.as_bytes()).unwrap();
            assert!(result.is_some(), "Key {} should exist", key);
            assert_eq!(result.unwrap().len(), 200);
        }
    }
}

/// Test no data races with concurrent access
#[test]
fn test_no_data_races() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Pre-populate
    for i in 0..100 {
        db.put(format!("key_{}", i).as_bytes(), b"initial").unwrap();
    }

    let num_threads = 20;
    let barrier = Arc::new(Barrier::new(num_threads));

    let mut handles = vec![];

    for thread_id in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let barrier_clone = Arc::clone(&barrier);
        let handle = thread::spawn(move || {
            barrier_clone.wait(); // Synchronize start for maximum contention

            for i in 0..100 {
                let key = format!("key_{}", i);

                // Read
                let _ = db_clone.get(key.as_bytes());

                // Write
                let value = format!("thread_{}_update", thread_id);
                db_clone.put(key.as_bytes(), value.as_bytes()).unwrap();

                // Read again
                let _ = db_clone.get(key.as_bytes());
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // All operations should complete without panics or data corruption
    // Verify database is still functional
    for i in 0..100 {
        let result = db.get(format!("key_{}", i).as_bytes()).unwrap();
        assert!(result.is_some(), "Key key_{} should exist", i);
    }
}

/// Test concurrent iteration (if iterator is implemented)
#[test]
fn test_concurrent_iteration() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Prepare sorted data
    for i in 0..1000 {
        let key = format!("iter_key_{:06}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }

    // Note: This test assumes iterator is thread-safe
    // If iterator is not yet implemented, this can be a placeholder

    println!("Iterator concurrent test - placeholder for future iterator implementation");
}

/// Test database remains consistent under high contention
#[test]
fn test_consistency_under_contention() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Initialize counter
    db.put(b"counter", b"0").unwrap();

    let num_threads = 50;
    let increments_per_thread = 20;

    let mut handles = vec![];

    for _ in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let handle = thread::spawn(move || {
            for _ in 0..increments_per_thread {
                // Read current value
                let current = db_clone.get(b"counter").unwrap().unwrap();
                let current_val: usize = String::from_utf8(current)
                    .unwrap()
                    .parse()
                    .unwrap();

                // Increment
                let new_val = current_val + 1;

                // Write back
                db_clone
                    .put(b"counter", new_val.to_string().as_bytes())
                    .unwrap();
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Note: Due to race conditions, final value may not be exactly
    // num_threads * increments_per_thread (this is expected without explicit locking)
    // But database should remain consistent and readable
    let final_value = db.get(b"counter").unwrap().unwrap();
    let final_count: usize = String::from_utf8(final_value).unwrap().parse().unwrap();

    println!("Final counter value: {}", final_count);
    println!(
        "Expected if serialized: {}",
        num_threads * increments_per_thread
    );

    // The database should at least be functional
    assert!(final_count > 0);
    assert!(final_count <= num_threads * increments_per_thread);
}

/// Test concurrent flush calls
#[test]
fn test_concurrent_flush_calls() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Write some data
    for i in 0..100 {
        db.put(format!("key_{}", i).as_bytes(), b"value").unwrap();
    }

    let num_threads = 10;
    let mut handles = vec![];

    // Multiple threads call flush simultaneously
    for _ in 0..num_threads {
        let db_clone = Arc::clone(&db);
        let handle = thread::spawn(move || {
            db_clone.flush().unwrap();
        });
        handles.push(handle);
    }

    // All should complete without error
    for handle in handles {
        handle.join().unwrap();
    }

    // Data should still be accessible
    for i in 0..100 {
        assert_eq!(
            db.get(format!("key_{}", i).as_bytes()).unwrap(),
            Some(b"value".to_vec())
        );
    }
}
