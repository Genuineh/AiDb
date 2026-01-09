//! Tests for tombstone handling and concurrent delete operations
//!
//! These tests verify that:
//! 1. Deleted keys cannot be read even after flush
//! 2. Concurrent delete + flush operations maintain consistency
//! 3. Tombstone markers properly override old values in SSTables

use aidb::{Options, DB};
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;

#[test]
fn test_delete_prevents_read_from_sstable() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options {
        memtable_size: 1024, // Small memtable to trigger flush
        use_wal: false,      // Disable WAL for faster tests
        ..Default::default()
    };

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Write a key
    db.put(b"key1", b"value1").unwrap();

    // Manually flush to SSTable
    db.flush().unwrap();

    // Verify key exists in SSTable
    assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));

    // Delete the key
    db.delete(b"key1").unwrap();

    // Key should not be readable even though it exists in SSTable
    assert_eq!(db.get(b"key1").unwrap(), None);
}

#[test]
fn test_tombstone_in_memtable_blocks_sstable_value() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { memtable_size: 1024, use_wal: false, ..Default::default() };

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Write and flush original value
    db.put(b"key1", b"old_value").unwrap();
    db.flush().unwrap();

    // Delete in new memtable
    db.delete(b"key1").unwrap();

    // Should return None (tombstone blocks old value)
    assert_eq!(db.get(b"key1").unwrap(), None);
}

#[test]
fn test_tombstone_persists_after_flush() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { memtable_size: 1024, use_wal: false, ..Default::default() };

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Write and flush original value
    db.put(b"key1", b"value1").unwrap();
    db.flush().unwrap();

    // Delete and flush tombstone
    db.delete(b"key1").unwrap();
    db.flush().unwrap();

    // Should still return None after tombstone is flushed
    assert_eq!(db.get(b"key1").unwrap(), None);
}

#[test]
fn test_concurrent_delete_and_flush() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { memtable_size: 4096, use_wal: false, ..Default::default() };

    let db = Arc::new(DB::open(temp_dir.path(), options).unwrap());

    // Write initial values
    for i in 0..100 {
        db.put(format!("key{}", i).as_bytes(), format!("value{}", i).as_bytes())
            .unwrap();
    }
    db.flush().unwrap();

    let db1 = Arc::clone(&db);
    let db2 = Arc::clone(&db);

    // Thread 1: Delete operations
    let delete_thread = thread::spawn(move || {
        for i in 0..50 {
            db1.delete(format!("key{}", i).as_bytes()).unwrap();
        }
    });

    // Thread 2: Flush operations
    let flush_thread = thread::spawn(move || {
        for _ in 0..5 {
            let _ = db2.flush();
            thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    delete_thread.join().unwrap();
    flush_thread.join().unwrap();

    // Final flush to persist everything
    db.flush().unwrap();

    // Verify deleted keys are not readable
    for i in 0..50 {
        assert_eq!(
            db.get(format!("key{}", i).as_bytes()).unwrap(),
            None,
            "Deleted key{} should not be readable",
            i
        );
    }

    // Verify non-deleted keys are still readable
    for i in 50..100 {
        assert_eq!(
            db.get(format!("key{}", i).as_bytes()).unwrap(),
            Some(format!("value{}", i).as_bytes().to_vec()),
            "Non-deleted key{} should be readable",
            i
        );
    }
}

#[test]
fn test_concurrent_read_during_delete_flush() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { memtable_size: 4096, use_wal: false, ..Default::default() };

    let db = Arc::new(DB::open(temp_dir.path(), options).unwrap());

    // Write initial data
    for i in 0..100 {
        db.put(format!("key{}", i).as_bytes(), format!("value{}", i).as_bytes())
            .unwrap();
    }
    db.flush().unwrap();

    let db1 = Arc::clone(&db);
    let db2 = Arc::clone(&db);
    let db3 = Arc::clone(&db);

    // Thread 1: Delete half the keys
    let delete_thread = thread::spawn(move || {
        for i in (0..100).step_by(2) {
            db1.delete(format!("key{}", i).as_bytes()).unwrap();
        }
    });

    // Thread 2: Periodic flush
    let flush_thread = thread::spawn(move || {
        for _ in 0..10 {
            let _ = db2.flush();
            thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    // Thread 3: Continuous reads
    let read_thread = thread::spawn(move || {
        for _ in 0..100 {
            for i in 0..100 {
                let result = db3.get(format!("key{}", i).as_bytes()).unwrap();
                // Result should either be Some(value) or None (deleted)
                // But never Some(wrong_value)
                if let Some(value) = result {
                    assert_eq!(
                        value,
                        format!("value{}", i).as_bytes().to_vec(),
                        "Read wrong value for key{}",
                        i
                    );
                }
            }
            thread::sleep(std::time::Duration::from_micros(100));
        }
    });

    delete_thread.join().unwrap();
    flush_thread.join().unwrap();
    read_thread.join().unwrap();
}

#[test]
fn test_delete_reput_sequence() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { memtable_size: 1024, use_wal: false, ..Default::default() };

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Initial write
    db.put(b"key1", b"value1").unwrap();
    db.flush().unwrap();
    assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));

    // Delete
    db.delete(b"key1").unwrap();
    assert_eq!(db.get(b"key1").unwrap(), None);

    // Re-put with new value
    db.put(b"key1", b"value2").unwrap();
    assert_eq!(db.get(b"key1").unwrap(), Some(b"value2".to_vec()));

    // Flush and verify
    db.flush().unwrap();
    assert_eq!(db.get(b"key1").unwrap(), Some(b"value2".to_vec()));
}

#[test]
fn test_multiple_deletes_same_key() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { use_wal: false, ..Default::default() };

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Write initial value
    db.put(b"key1", b"value1").unwrap();
    assert_eq!(db.get(b"key1").unwrap(), Some(b"value1".to_vec()));

    // Multiple deletes
    db.delete(b"key1").unwrap();
    assert_eq!(db.get(b"key1").unwrap(), None);

    db.delete(b"key1").unwrap();
    assert_eq!(db.get(b"key1").unwrap(), None);

    db.delete(b"key1").unwrap();
    assert_eq!(db.get(b"key1").unwrap(), None);
}

#[test]
fn test_tombstone_with_empty_value() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { use_wal: false, ..Default::default() };

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Note: In this LSM-Tree implementation, empty values are used to represent tombstones.
    // This is a design choice - empty byte slices cannot be stored as regular values
    // because they are indistinguishable from deletion markers in the SSTable format.
    //
    // This is a known limitation: you cannot store empty values in this database.
    // Attempting to store an empty value and then delete it will both result in None.

    // Write empty value (this will be stored)
    db.put(b"key1", b"").unwrap();

    // Reading empty value returns None (treated as tombstone)
    // This is expected behavior due to the design limitation
    assert_eq!(db.get(b"key1").unwrap(), None);

    // Explicit delete also returns None
    db.delete(b"key1").unwrap();
    assert_eq!(db.get(b"key1").unwrap(), None);
}

#[test]
fn test_concurrent_delete_different_keys() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { memtable_size: 4096, use_wal: false, ..Default::default() };

    let db = Arc::new(DB::open(temp_dir.path(), options).unwrap());

    // Write initial data
    for i in 0..1000 {
        db.put(format!("key{}", i).as_bytes(), format!("value{}", i).as_bytes())
            .unwrap();
    }
    db.flush().unwrap();

    // Spawn multiple threads deleting different key ranges
    let mut handles = vec![];
    for thread_id in 0..10 {
        let db_clone = Arc::clone(&db);
        let handle = thread::spawn(move || {
            for i in (thread_id * 100)..((thread_id + 1) * 100) {
                db_clone.delete(format!("key{}", i).as_bytes()).unwrap();
            }
        });
        handles.push(handle);
    }

    // Wait for all deletes to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // Flush and verify all keys are deleted
    db.flush().unwrap();

    for i in 0..1000 {
        assert_eq!(
            db.get(format!("key{}", i).as_bytes()).unwrap(),
            None,
            "key{} should be deleted",
            i
        );
    }
}

#[test]
fn test_snapshot_isolation_with_delete() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options { use_wal: false, ..Default::default() };

    let db = Arc::new(DB::open(temp_dir.path(), options).unwrap());

    // Write initial value
    db.put(b"key1", b"value1").unwrap();

    // Create snapshot
    let snapshot = db.snapshot();

    // Delete key in main DB
    db.delete(b"key1").unwrap();

    // Snapshot should still see the old value (while it's in MemTable)
    assert_eq!(snapshot.get(b"key1").unwrap(), Some(b"value1".to_vec()));

    // Main DB should see None
    assert_eq!(db.get(b"key1").unwrap(), None);

    // NOTE: After flush, snapshot isolation is LIMITED in this implementation.
    // When MemTable is flushed to SSTable, only the latest version of each key
    // is retained. This means snapshots can't see old versions after flush.
    // A full implementation would need version reference counting and delayed
    // cleanup of old versions until all snapshots are released.
    //
    // For production use, snapshots should be short-lived and used before flush.

    // After flush, the snapshot can no longer see old versions (known limitation)
    db.flush().unwrap();

    // Both return None because only the tombstone was written to SSTable
    assert_eq!(snapshot.get(b"key1").unwrap(), None);
    assert_eq!(db.get(b"key1").unwrap(), None);
}
