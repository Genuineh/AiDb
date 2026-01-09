//! Test concurrent delete and clear operations
//! This reproduces issues seen in AiKv when clearing data concurrently

use aidb::{Options, WriteBatch, DB};
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;

#[test]
fn test_concurrent_clear_and_exists() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options {
        use_wal: false,
        memtable_size: 8192,
        ..Default::default()
    };
    let db = Arc::new(DB::open(temp_dir.path(), options).unwrap());

    // Insert initial data
    for i in 0..100 {
        let key = format!("key{}", i);
        let value = format!("value{}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }

    // Verify all keys exist
    for i in 0..100 {
        let key = format!("key{}", i);
        assert!(
            db.get(key.as_bytes()).unwrap().is_some(),
            "Key {} should exist before clear",
            key
        );
    }

    // Thread 1: Clear all data (delete all keys)
    let db1 = Arc::clone(&db);
    let clear_thread = thread::spawn(move || {
        for i in 0..100 {
            let key = format!("key{}", i);
            db1.delete(key.as_bytes()).unwrap();
        }
    });

    // Thread 2: Check if keys exist
    let db2 = Arc::clone(&db);
    let check_thread = thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(10));
        let mut found_count = 0;
        for i in 0..100 {
            let key = format!("key{}", i);
            if db2.get(key.as_bytes()).unwrap().is_some() {
                found_count += 1;
            }
        }
        found_count
    });

    clear_thread.join().unwrap();
    let found_count = check_thread.join().unwrap();

    println!("Found {} keys after clear (should be 0 or partial)", found_count);

    // After both threads finish, NO keys should exist
    for i in 0..100 {
        let key = format!("key{}", i);
        let result = db.get(key.as_bytes()).unwrap();
        assert!(
            result.is_none(),
            "Key {} should NOT exist after clear, but got: {:?}",
            key,
            result
        );
    }
}

#[test]
fn test_batch_clear_with_concurrent_reads() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options {
        use_wal: false,
        ..Default::default()
    };
    let db = Arc::new(DB::open(temp_dir.path(), options).unwrap());

    // Insert data
    for i in 0..50 {
        let key = format!("key{}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }

    // Use WriteBatch to clear data
    let db1 = Arc::clone(&db);
    let clear_thread = thread::spawn(move || {
        let mut batch = WriteBatch::new();
        for i in 0..50 {
            let key = format!("key{}", i);
            batch.delete(key.as_bytes());
        }
        db1.write(batch).unwrap();
    });

    // Concurrent reads
    let db2 = Arc::clone(&db);
    let read_thread = thread::spawn(move || {
        for _ in 0..100 {
            for i in 0..50 {
                let key = format!("key{}", i);
                let _ = db2.get(key.as_bytes()).unwrap();
            }
        }
    });

    clear_thread.join().unwrap();
    read_thread.join().unwrap();

    // All keys should be deleted
    for i in 0..50 {
        let key = format!("key{}", i);
        assert!(db.get(key.as_bytes()).unwrap().is_none());
    }
}

#[test]
fn test_clear_with_flush() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options {
        use_wal: false,
        memtable_size: 4096,
        ..Default::default()
    };
    let db = Arc::new(DB::open(temp_dir.path(), options).unwrap());

    // Insert and flush to SSTable
    for i in 0..100 {
        let key = format!("key{}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }
    db.flush().unwrap();

    // Clear data (delete all keys)
    let db1 = Arc::clone(&db);
    let clear_thread = thread::spawn(move || {
        for i in 0..100 {
            let key = format!("key{}", i);
            db1.delete(key.as_bytes()).unwrap();
        }
    });

    // Concurrent flush
    let db2 = Arc::clone(&db);
    let flush_thread = thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(5));
        for _ in 0..5 {
            let _ = db2.flush();
            thread::sleep(std::time::Duration::from_millis(2));
        }
    });

    clear_thread.join().unwrap();
    flush_thread.join().unwrap();

    // Flush one more time to ensure all deletes are persisted
    db.flush().unwrap();

    // All keys should be deleted
    for i in 0..100 {
        let key = format!("key{}", i);
        let result = db.get(key.as_bytes()).unwrap();
        assert!(
            result.is_none(),
            "Key {} should be deleted after clear+flush, but got: {:?}",
            key,
            result
        );
    }
}

#[test]
fn test_reopen_after_concurrent_clear() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path();

    // First session: insert, clear concurrently, and close
    {
        let db = Arc::new(DB::open(db_path, Options::default()).unwrap());

        // Insert data
        for i in 0..100 {
            let key = format!("key{}", i);
            db.put(key.as_bytes(), b"value").unwrap();
        }

        // Concurrent clear
        let db1 = Arc::clone(&db);
        let db2 = Arc::clone(&db);

        let t1 = thread::spawn(move || {
            for i in 0..50 {
                let key = format!("key{}", i);
                db1.delete(key.as_bytes()).unwrap();
            }
        });

        let t2 = thread::spawn(move || {
            for i in 50..100 {
                let key = format!("key{}", i);
                db2.delete(key.as_bytes()).unwrap();
            }
        });

        t1.join().unwrap();
        t2.join().unwrap();

        db.flush().unwrap();
        db.close().unwrap();
    }

    // Reopen and verify all keys are deleted
    {
        let db = DB::open(db_path, Options::default()).unwrap();

        for i in 0..100 {
            let key = format!("key{}", i);
            let result = db.get(key.as_bytes()).unwrap();
            assert!(
                result.is_none(),
                "Key {} should be deleted after reopen, but got: {:?}",
                key,
                result
            );
        }
    }
}

#[test]
fn test_delete_same_key_concurrently() {
    let temp_dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(temp_dir.path(), Options::default()).unwrap());

    // Insert a key
    db.put(b"key", b"value").unwrap();

    // Multiple threads try to delete the same key
    let mut handles = vec![];
    for _ in 0..10 {
        let db_clone = Arc::clone(&db);
        let handle = thread::spawn(move || {
            db_clone.delete(b"key").unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Key should be deleted
    assert!(db.get(b"key").unwrap().is_none());
}

#[test]
fn test_clear_pattern_like_redis_flushdb() {
    // Simulate Redis FLUSHDB operation pattern
    let temp_dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(temp_dir.path(), Options::default()).unwrap());

    // Populate database
    for i in 0..200 {
        let key = format!("key{}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }

    // Flush to SSTable
    db.flush().unwrap();

    // FLUSHDB operation: delete all keys
    // This might happen while other operations are ongoing
    let db1 = Arc::clone(&db);
    let db2 = Arc::clone(&db);
    let db3 = Arc::clone(&db);

    // Thread 1: Delete keys 0-66
    let t1 = thread::spawn(move || {
        for i in 0..67 {
            let key = format!("key{}", i);
            db1.delete(key.as_bytes()).unwrap();
        }
    });

    // Thread 2: Delete keys 67-133
    let t2 = thread::spawn(move || {
        for i in 67..134 {
            let key = format!("key{}", i);
            db2.delete(key.as_bytes()).unwrap();
        }
    });

    // Thread 3: Delete keys 134-199
    let t3 = thread::spawn(move || {
        for i in 134..200 {
            let key = format!("key{}", i);
            db3.delete(key.as_bytes()).unwrap();
        }
    });

    t1.join().unwrap();
    t2.join().unwrap();
    t3.join().unwrap();

    // Flush deletes
    db.flush().unwrap();

    // Verify all keys are deleted
    for i in 0..200 {
        let key = format!("key{}", i);
        let result = db.get(key.as_bytes()).unwrap();
        assert!(
            result.is_none(),
            "Key {} should be deleted after FLUSHDB, but got: {:?}",
            key,
            result
        );
    }
}

#[test]
fn test_exists_check_during_concurrent_clear() {
    let temp_dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(temp_dir.path(), Options::default()).unwrap());

    // Insert keys
    for i in 0..100 {
        let key = format!("key{}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }

    let db1 = Arc::clone(&db);
    let db2 = Arc::clone(&db);

    // Thread 1: Clear all
    let clear_handle = thread::spawn(move || {
        for i in 0..100 {
            let key = format!("key{}", i);
            db1.delete(key.as_bytes()).unwrap();
        }
    });

    // Thread 2: Keep checking exists
    let check_handle = thread::spawn(move || {
        for _ in 0..20 {
            for i in 0..100 {
                let key = format!("key{}", i);
                // Just check, don't assert - key might be deleted during iteration
                let _ = db2.get(key.as_bytes()).unwrap();
            }
            thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    clear_handle.join().unwrap();
    check_handle.join().unwrap();

    // Final verification: all should be deleted
    for i in 0..100 {
        let key = format!("key{}", i);
        assert!(
            db.get(key.as_bytes()).unwrap().is_none(),
            "Key {} should be deleted",
            key
        );
    }
}
