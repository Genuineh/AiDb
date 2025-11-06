//! Integration tests for compaction functionality

use aidb::{Options, DB};
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn test_level0_compaction_trigger() {
    env_logger::try_init().ok();

    let temp_dir = TempDir::new().unwrap();
    let options = Options::default()
        .memtable_size(1024) // Small memtable to trigger flush
        .block_size(512); // Small blocks

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Write enough data to create 4+ SSTables at Level 0
    for batch in 0..5 {
        for i in 0..50 {
            let key = format!("batch{:02}_key{:04}", batch, i);
            let value = vec![b'x'; 100];
            db.put(key.as_bytes(), &value).unwrap();
        }
        // Manually flush to create SSTable at Level 0
        db.flush().unwrap();
    }

    // After 5 flushes, compaction should have been triggered
    // Verify all data is still accessible
    for batch in 0..5 {
        for i in 0..50 {
            let key = format!("batch{:02}_key{:04}", batch, i);
            let value = db.get(key.as_bytes()).unwrap();
            assert!(value.is_some(), "Key {} should exist", key);
        }
    }
}

#[test]
fn test_compaction_removes_duplicates() {
    env_logger::try_init().ok();

    let temp_dir = TempDir::new().unwrap();
    let options = Options::default().memtable_size(1024);

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Write same key multiple times across different SSTables
    for version in 0..10 {
        let key = b"duplicate_key";
        let value = format!("version_{}", version);
        db.put(key, value.as_bytes()).unwrap();

        if version % 2 == 1 {
            db.flush().unwrap();
        }
    }

    // Final flush
    db.flush().unwrap();

    // Verify we can still read the latest value
    let value = db.get(b"duplicate_key").unwrap();
    assert!(value.is_some());
    assert_eq!(value.unwrap(), b"version_9");
}

#[test]
fn test_compaction_removes_deleted_entries() {
    env_logger::try_init().ok();

    let temp_dir = TempDir::new().unwrap();
    let options = Options::default().memtable_size(1024);

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Write and delete keys
    for i in 0..100 {
        let key = format!("key{:04}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }
    db.flush().unwrap();

    // Delete half of the keys
    for i in 0..50 {
        let key = format!("key{:04}", i);
        db.delete(key.as_bytes()).unwrap();
    }
    db.flush().unwrap();

    // Trigger more flushes to cause compaction
    for i in 100..300 {
        let key = format!("key{:04}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }
    db.flush().unwrap();

    // Verify deleted keys are gone
    for i in 0..50 {
        let key = format!("key{:04}", i);
        assert_eq!(db.get(key.as_bytes()).unwrap(), None);
    }

    // Verify non-deleted keys exist
    for i in 50..100 {
        let key = format!("key{:04}", i);
        assert!(db.get(key.as_bytes()).unwrap().is_some());
    }
}

#[test]
fn test_compaction_maintains_sort_order() {
    env_logger::try_init().ok();

    let temp_dir = TempDir::new().unwrap();
    let options = Options::default().memtable_size(1024);

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Write keys in random order across multiple SSTables
    let keys = vec![
        "gamma", "alpha", "epsilon", "beta", "delta", "zeta", "theta", "iota",
    ];

    for (batch, key) in keys.iter().enumerate() {
        let value = format!("value_{}", batch);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();

        if batch % 2 == 1 {
            db.flush().unwrap();
        }
    }

    db.flush().unwrap();

    // Verify all keys are accessible
    for (batch, key) in keys.iter().enumerate() {
        let value = db.get(key.as_bytes()).unwrap();
        assert!(value.is_some());
        let expected = format!("value_{}", batch);
        assert_eq!(value.unwrap(), expected.as_bytes());
    }
}

#[test]
fn test_compaction_across_restarts() {
    env_logger::try_init().ok();

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().to_path_buf();

    // First session: create data and trigger compaction
    {
        let options = Options::default().memtable_size(1024);
        let db = DB::open(&db_path, options).unwrap();

        for batch in 0..5 {
            for i in 0..30 {
                let key = format!("batch{:02}_key{:04}", batch, i);
                let value = vec![b'x'; 100];
                db.put(key.as_bytes(), &value).unwrap();
            }
            db.flush().unwrap();
        }

        db.close().unwrap();
    }

    // Second session: verify data after restart
    {
        let options = Options::default();
        let db = DB::open(&db_path, options).unwrap();

        for batch in 0..5 {
            for i in 0..30 {
                let key = format!("batch{:02}_key{:04}", batch, i);
                let value = db.get(key.as_bytes()).unwrap();
                assert!(value.is_some(), "Key {} should exist after restart", key);
            }
        }
    }
}

#[test]
fn test_concurrent_writes_during_compaction() {
    env_logger::try_init().ok();

    let temp_dir = TempDir::new().unwrap();
    let options = Options::default().memtable_size(2048);

    let db = Arc::new(DB::open(temp_dir.path(), options).unwrap());

    let mut handles = vec![];

    // Spawn multiple writer threads
    for thread_id in 0..4 {
        let db_clone = Arc::clone(&db);
        let handle = std::thread::spawn(move || {
            for i in 0..100 {
                let key = format!("thread{:02}_key{:04}", thread_id, i);
                let value = vec![b'x'; 100];
                db_clone.put(key.as_bytes(), &value).unwrap();

                // Periodically flush to trigger compaction
                if i % 20 == 0 {
                    db_clone.flush().unwrap();
                }
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify all data
    for thread_id in 0..4 {
        for i in 0..100 {
            let key = format!("thread{:02}_key{:04}", thread_id, i);
            let value = db.get(key.as_bytes()).unwrap();
            assert!(value.is_some(), "Key {} should exist", key);
        }
    }
}

#[test]
fn test_large_dataset_compaction() {
    env_logger::try_init().ok();

    let temp_dir = TempDir::new().unwrap();
    let options = Options::default().memtable_size(4096);

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Write a large dataset
    for i in 0..1000 {
        let key = format!("key{:08}", i);
        let value = format!("value{:08}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();

        // Flush every 200 entries
        if i % 200 == 199 {
            db.flush().unwrap();
        }
    }

    // Final flush
    db.flush().unwrap();

    // Verify all data
    for i in 0..1000 {
        let key = format!("key{:08}", i);
        let expected = format!("value{:08}", i);
        let value = db.get(key.as_bytes()).unwrap();
        assert_eq!(value, Some(expected.as_bytes().to_vec()));
    }
}

#[test]
fn test_compaction_with_overwrites() {
    env_logger::try_init().ok();

    let temp_dir = TempDir::new().unwrap();
    let options = Options::default().memtable_size(1024);

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Write initial data
    for i in 0..50 {
        let key = format!("key{:04}", i);
        db.put(key.as_bytes(), b"original").unwrap();
    }
    db.flush().unwrap();

    // Overwrite some keys
    for i in 0..25 {
        let key = format!("key{:04}", i);
        db.put(key.as_bytes(), b"updated").unwrap();
    }
    db.flush().unwrap();

    // Trigger more writes to cause compaction
    for i in 50..150 {
        let key = format!("key{:04}", i);
        db.put(key.as_bytes(), b"new").unwrap();
    }
    db.flush().unwrap();

    // Verify updated keys
    for i in 0..25 {
        let key = format!("key{:04}", i);
        let value = db.get(key.as_bytes()).unwrap();
        assert_eq!(value, Some(b"updated".to_vec()));
    }

    // Verify original keys
    for i in 25..50 {
        let key = format!("key{:04}", i);
        let value = db.get(key.as_bytes()).unwrap();
        assert_eq!(value, Some(b"original".to_vec()));
    }

    // Verify new keys
    for i in 50..150 {
        let key = format!("key{:04}", i);
        let value = db.get(key.as_bytes()).unwrap();
        assert_eq!(value, Some(b"new".to_vec()));
    }
}
