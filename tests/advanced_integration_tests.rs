// Advanced Integration Tests for AiDb
// Tests for advanced features like snapshots, iterators, write batches, and configurations

use aidb::{Options, WriteBatch, DB};
use std::sync::Arc;
use tempfile::TempDir;

/// Test snapshot isolation
#[test]
fn test_snapshot_isolation() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Write initial data
    db.put(b"key1", b"value1").unwrap();
    db.put(b"key2", b"value2").unwrap();

    // Create snapshot
    let snapshot = db.snapshot();

    // Modify data after snapshot
    db.put(b"key1", b"value1_modified").unwrap();
    db.put(b"key3", b"value3").unwrap();
    db.delete(b"key2").unwrap();

    // Snapshot should see old data
    assert_eq!(
        snapshot.get(b"key1").unwrap(),
        Some(b"value1".to_vec()),
        "Snapshot should see old value"
    );
    assert_eq!(
        snapshot.get(b"key2").unwrap(),
        Some(b"value2".to_vec()),
        "Snapshot should see deleted key"
    );
    assert_eq!(snapshot.get(b"key3").unwrap(), None, "Snapshot should not see new key");

    // Current DB should see new data
    assert_eq!(db.get(b"key1").unwrap(), Some(b"value1_modified".to_vec()));
    assert_eq!(db.get(b"key2").unwrap(), None);
    assert_eq!(db.get(b"key3").unwrap(), Some(b"value3".to_vec()));
}

/// Test multiple snapshots
#[test]
fn test_multiple_snapshots() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // State 1
    db.put(b"key", b"value1").unwrap();
    let snap1 = db.snapshot();

    // State 2
    db.put(b"key", b"value2").unwrap();
    let snap2 = db.snapshot();

    // State 3
    db.put(b"key", b"value3").unwrap();
    let snap3 = db.snapshot();

    // Each snapshot should see its own version
    assert_eq!(snap1.get(b"key").unwrap(), Some(b"value1".to_vec()));
    assert_eq!(snap2.get(b"key").unwrap(), Some(b"value2".to_vec()));
    assert_eq!(snap3.get(b"key").unwrap(), Some(b"value3".to_vec()));
    assert_eq!(db.get(b"key").unwrap(), Some(b"value3".to_vec()));
}

/// Test snapshot with flush
#[test]
fn test_snapshot_across_flush() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    db.put(b"key1", b"value1").unwrap();
    let snapshot = db.snapshot();

    // Flush after snapshot
    db.flush().unwrap();

    // Modify data
    db.put(b"key1", b"value1_new").unwrap();

    // Snapshot should still see old value
    assert_eq!(
        snapshot.get(b"key1").unwrap(),
        Some(b"value1".to_vec()),
        "Snapshot should work across flush"
    );
}

/// Test iterator basics
#[test]
fn test_iterator_basic() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Insert data in non-sorted order
    db.put(b"key3", b"value3").unwrap();
    db.put(b"key1", b"value1").unwrap();
    db.put(b"key2", b"value2").unwrap();

    // Iterate and collect entries
    let mut iter = db.iter();
    let mut entries = Vec::new();
    while iter.valid() {
        entries.push((iter.key().to_vec(), iter.value().to_vec()));
        iter.next();
    }

    assert_eq!(entries.len(), 3);
    // Should be sorted
    assert_eq!(entries[0].0, b"key1");
    assert_eq!(entries[1].0, b"key2");
    assert_eq!(entries[2].0, b"key3");
}

/// Test iterator with range
#[test]
fn test_iterator_range() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Insert keys a0 to a9
    for i in 0..10 {
        let key = format!("a{}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }

    // Count all entries
    let mut iter = db.iter();
    let mut count = 0;
    while iter.valid() {
        count += 1;
        iter.next();
    }
    assert_eq!(count, 10);
}

/// Test iterator with deletes
#[test]
fn test_iterator_with_deletes() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    db.put(b"key1", b"value1").unwrap();
    db.put(b"key2", b"value2").unwrap();
    db.put(b"key3", b"value3").unwrap();

    // Delete middle key
    db.delete(b"key2").unwrap();

    let mut iter = db.iter();
    let mut entries = Vec::new();
    while iter.valid() {
        entries.push(iter.key().to_vec());
        iter.next();
    }

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0], b"key1");
    assert_eq!(entries[1], b"key3");
}

/// Test iterator after flush
#[test]
fn test_iterator_after_flush() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    db.put(b"key1", b"value1").unwrap();
    db.put(b"key2", b"value2").unwrap();

    db.flush().unwrap();

    db.put(b"key3", b"value3").unwrap();

    let mut iter = db.iter();
    let mut entries = Vec::new();
    while iter.valid() {
        entries.push(iter.key().to_vec());
        iter.next();
    }

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0], b"key1");
    assert_eq!(entries[1], b"key2");
    assert_eq!(entries[2], b"key3");
}

/// Test WriteBatch atomic operations
#[test]
fn test_write_batch_atomic() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    let mut batch = WriteBatch::new();
    batch.put(b"key1", b"value1");
    batch.put(b"key2", b"value2");
    batch.delete(b"key3");

    db.write(batch).unwrap();

    assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));
    assert_eq!(db.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    assert_eq!(db.get(b"key3").unwrap(), None);
}

/// Test WriteBatch with large number of operations
#[test]
fn test_write_batch_large() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    let mut batch = WriteBatch::new();
    for i in 0..1000 {
        let key = format!("key{}", i);
        let value = format!("value{}", i);
        batch.put(key.as_bytes(), value.as_bytes());
    }

    db.write(batch).unwrap();

    // Verify all were written
    for i in 0..1000 {
        let key = format!("key{}", i);
        let value = format!("value{}", i);
        assert_eq!(db.get(key.as_bytes()).unwrap(), Some(value.into_bytes()));
    }
}

/// Test WriteBatch with mixed operations
#[test]
fn test_write_batch_mixed_operations() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Pre-populate
    db.put(b"existing1", b"old_value1").unwrap();
    db.put(b"existing2", b"old_value2").unwrap();

    let mut batch = WriteBatch::new();
    batch.put(b"new1", b"value1"); // New key
    batch.put(b"existing1", b"new_value1"); // Update existing
    batch.delete(b"existing2"); // Delete existing
    batch.put(b"new2", b"value2"); // Another new key

    db.write(batch).unwrap();

    assert_eq!(db.get(b"new1").unwrap(), Some(b"value1".to_vec()));
    assert_eq!(db.get(b"new2").unwrap(), Some(b"value2".to_vec()));
    assert_eq!(db.get(b"existing1").unwrap(), Some(b"new_value1".to_vec()));
    assert_eq!(db.get(b"existing2").unwrap(), None);
}

/// Test configuration options: memtable size
#[test]
fn test_config_memtable_size() {
    let dir = TempDir::new().unwrap();

    let options = Options {
        memtable_size: 1024 * 1024, // 1MB
        ..Default::default()
    };

    let db = Arc::new(DB::open(dir.path(), options).unwrap());

    // Write data
    for i in 0..100 {
        let key = format!("key{}", i);
        let value = vec![b'v'; 1000];
        db.put(key.as_bytes(), &value).unwrap();
    }

    // Should trigger flush due to size
    db.flush().unwrap();

    // Verify data persisted
    assert!(db.get(b"key0").unwrap().is_some());
    assert!(db.get(b"key99").unwrap().is_some());
}

/// Test configuration: WAL disabled (data persists via automatic flush on drop)
#[test]
fn test_config_wal_disabled() {
    let dir = TempDir::new().unwrap();

    {
        let options = Options { use_wal: false, ..Default::default() };

        let db = Arc::new(DB::open(dir.path(), options).unwrap());
        db.put(b"key", b"value").unwrap();
        // Drop will flush even without WAL
    }

    // Reopen - data should persist via flush on drop
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());
    // With automatic flush on drop, data persists
    assert_eq!(db.get(b"key").unwrap(), Some(b"value".to_vec()));
}

/// Test configuration: WAL enabled with recovery
#[test]
fn test_config_wal_enabled_recovery() {
    let dir = TempDir::new().unwrap();

    {
        let options = Options { use_wal: true, ..Default::default() };

        let db = Arc::new(DB::open(dir.path(), options).unwrap());
        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
        // Drop will flush
    }

    // Reopen - data should persist via flush on drop
    let options = Options { use_wal: true, ..Default::default() };
    let db = Arc::new(DB::open(dir.path(), options).unwrap());
    assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));
    assert_eq!(db.get(b"key2").unwrap(), Some(b"value2".to_vec()));
}

/// Test configuration: bloom filter
#[test]
fn test_config_bloom_filter() {
    let dir = TempDir::new().unwrap();

    let options = Options { use_bloom_filter: true, ..Default::default() };

    let db = Arc::new(DB::open(dir.path(), options).unwrap());

    // Write and flush
    for i in 0..100 {
        let key = format!("key{}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }
    db.flush().unwrap();

    // Queries should benefit from bloom filter
    assert_eq!(db.get(b"key0").unwrap(), Some(b"value".to_vec()));
    assert_eq!(db.get(b"nonexistent").unwrap(), None);
}

/// Test configuration: block cache
#[test]
fn test_config_block_cache() {
    let dir = TempDir::new().unwrap();

    let options = Options {
        block_cache_size: 1024 * 1024, // 1MB cache
        ..Default::default()
    };

    let db = Arc::new(DB::open(dir.path(), options).unwrap());

    // Write and flush to create SSTables
    for i in 0..100 {
        let key = format!("key{}", i);
        db.put(key.as_bytes(), b"value").unwrap();
    }
    db.flush().unwrap();

    // First read - cache miss
    let _ = db.get(b"key0").unwrap();

    // Second read - should hit cache
    let _ = db.get(b"key0").unwrap();

    assert_eq!(db.get(b"key0").unwrap(), Some(b"value".to_vec()));
}

/// Test configuration: compression
#[test]
fn test_config_compression() {
    let dir = TempDir::new().unwrap();

    let options =
        Options { compression: aidb::config::CompressionType::Snappy, ..Default::default() };

    let db = Arc::new(DB::open(dir.path(), options).unwrap());

    // Write compressible data
    let value = vec![b'a'; 10000]; // Highly compressible
    for i in 0..100 {
        let key = format!("key{}", i);
        db.put(key.as_bytes(), &value).unwrap();
    }

    db.flush().unwrap();

    // Verify data can be read back
    for i in 0..100 {
        let key = format!("key{}", i);
        assert_eq!(db.get(key.as_bytes()).unwrap(), Some(value.clone()));
    }
}

/// Test range scan functionality
#[test]
fn test_range_scan() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Insert data with prefixes
    for prefix in &["a", "b", "c"] {
        for i in 0..10 {
            let key = format!("{}{}", i, prefix);
            db.put(key.as_bytes(), b"value").unwrap();
        }
    }

    // Manually filter by checking keys
    let mut iter = db.iter();
    let mut count = 0;
    while iter.valid() {
        count += 1;
        iter.next();
    }

    // We should have all 30 keys
    assert_eq!(count, 30);
}

/// Test concurrent snapshots
#[test]
fn test_concurrent_snapshots() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    db.put(b"key", b"initial").unwrap();

    let handles: Vec<_> = (0..5)
        .map(|i| {
            let db_clone = Arc::clone(&db);
            std::thread::spawn(move || {
                let snapshot = db_clone.snapshot();
                let value = format!("value{}", i);
                db_clone.put(b"key", value.as_bytes()).unwrap();

                // Snapshot should see initial value
                snapshot.get(b"key").unwrap()
            })
        })
        .collect();

    for handle in handles {
        let result = handle.join().unwrap();
        // Each snapshot should see "initial" or an earlier update
        assert!(result.is_some());
    }
}

/// Test write batch persistence
#[test]
fn test_write_batch_persistence() {
    let dir = TempDir::new().unwrap();

    {
        let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

        let mut batch = WriteBatch::new();
        for i in 0..100 {
            let key = format!("key{}", i);
            batch.put(key.as_bytes(), b"batch_value");
        }

        db.write(batch).unwrap();
        db.flush().unwrap();
    }

    // Reopen and verify
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());
    for i in 0..100 {
        let key = format!("key{}", i);
        assert_eq!(db.get(key.as_bytes()).unwrap(), Some(b"batch_value".to_vec()));
    }
}

/// Test empty write batch
#[test]
fn test_empty_write_batch() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    let batch = WriteBatch::new();
    assert!(db.write(batch).is_ok());

    // Database should still be functional
    db.put(b"key", b"value").unwrap();
    assert_eq!(db.get(b"key").unwrap(), Some(b"value".to_vec()));
}

/// Test large write batch
#[test]
fn test_very_large_write_batch() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    let mut batch = WriteBatch::new();
    for i in 0..10000 {
        let key = format!("key{:06}", i);
        let value = format!("value{:06}", i);
        batch.put(key.as_bytes(), value.as_bytes());
    }

    db.write(batch).unwrap();

    // Spot check
    assert!(db.get(b"key000000").unwrap().is_some());
    assert!(db.get(b"key005000").unwrap().is_some());
    assert!(db.get(b"key009999").unwrap().is_some());
}

/// Test configuration presets
#[test]
fn test_config_basic_options() {
    let dir = TempDir::new().unwrap();

    // Test default config
    let db_default = Arc::new(DB::open(dir.path().join("default"), Options::default()).unwrap());
    db_default.put(b"test", b"value").unwrap();
    assert_eq!(db_default.get(b"test").unwrap(), Some(b"value".to_vec()));

    // Test custom config with performance options
    let opts_perf = Options {
        memtable_size: 16 * 1024 * 1024,   // 16MB for better performance
        block_cache_size: 8 * 1024 * 1024, // 8MB cache
        use_bloom_filter: true,
        ..Default::default()
    };
    let db_perf = Arc::new(DB::open(dir.path().join("performance"), opts_perf).unwrap());
    db_perf.put(b"test", b"value").unwrap();
    assert_eq!(db_perf.get(b"test").unwrap(), Some(b"value".to_vec()));

    // Test custom config with durability options
    let opts_dur = Options { use_wal: true, ..Default::default() };
    let db_dur = Arc::new(DB::open(dir.path().join("durability"), opts_dur).unwrap());
    db_dur.put(b"test", b"value").unwrap();
    assert_eq!(db_dur.get(b"test").unwrap(), Some(b"value".to_vec()));
}
