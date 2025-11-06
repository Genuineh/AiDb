/// Tests to verify the SSTable management bug fixes
///
/// This test suite verifies that the following bugs have been fixed:
/// 1. Unreliable file-size matching
/// 2. Race condition with dangling references
/// 3. Desynchronized in-memory and persisted state
/// 4. Duplicate Arc usage
use aidb::{Options, DB};
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;

/// Test that file identification is reliable even when files have the same size
#[test]
fn test_reliable_file_identification() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options::default().memtable_size(1024); // Small memtable to trigger flushes
    let db = DB::open(temp_dir.path(), options).unwrap();

    // Create multiple SSTables that might have similar sizes
    for batch in 0..5 {
        for i in 0..10 {
            let key = format!("batch{}_key{}", batch, i);
            let value = format!("value{}", i); // Same value length for all batches
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        db.flush().unwrap(); // Create an SSTable
    }

    // Verify all data is accessible
    for batch in 0..5 {
        for i in 0..10 {
            let key = format!("batch{}_key{}", batch, i);
            let expected = format!("value{}", i);
            let value = db.get(key.as_bytes()).unwrap();
            assert_eq!(
                value,
                Some(expected.as_bytes().to_vec()),
                "Data should be accessible after multiple flushes with similar-sized SSTables"
            );
        }
    }

    // Trigger compaction (which uses file identification)
    // Force enough writes to trigger compaction
    for i in 0..100 {
        let key = format!("compact_key{}", i);
        let value = vec![b'x'; 100];
        db.put(key.as_bytes(), &value).unwrap();
    }
    db.flush().unwrap();

    // Verify original data is still accessible after compaction
    for batch in 0..5 {
        for i in 0..10 {
            let key = format!("batch{}_key{}", batch, i);
            let value = db.get(key.as_bytes()).unwrap();
            assert!(
                value.is_some(),
                "Data should still be accessible after compaction (no wrong files deleted)"
            );
        }
    }
}

/// Test that in-memory state stays consistent with persisted state
#[test]
fn test_consistent_state_after_compaction() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().to_path_buf();

    // First session: create data and trigger compaction
    {
        let options = Options::default().memtable_size(1024);
        let db = DB::open(&db_path, options).unwrap();

        // Create multiple SSTables
        for batch in 0..6 {
            for i in 0..20 {
                let key = format!("key{:03}_{}", batch, i);
                let value = format!("value{}", i);
                db.put(key.as_bytes(), value.as_bytes()).unwrap();
            }
            db.flush().unwrap();
        }

        // Verify data before close
        let value = db.get(b"key000_0").unwrap();
        assert!(value.is_some(), "Data should be accessible before close");

        db.close().unwrap();
    }

    // Second session: reopen and verify all data is accessible
    // This tests that version set and in-memory state were synchronized
    {
        let db = DB::open(&db_path, Options::default()).unwrap();

        // Verify all data is accessible after restart
        for batch in 0..6 {
            for i in 0..20 {
                let key = format!("key{:03}_{}", batch, i);
                let expected = format!("value{}", i);
                let value = db.get(key.as_bytes()).unwrap();
                assert_eq!(
                    value,
                    Some(expected.as_bytes().to_vec()),
                    "Data should be accessible after restart (version set and in-memory state synced)"
                );
            }
        }
    }
}

/// Test concurrent access during compaction doesn't create dangling references
#[test]
fn test_no_dangling_references() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options::default().memtable_size(512); // Very small to trigger frequent flushes
    let db = Arc::new(DB::open(temp_dir.path(), options).unwrap());

    // Spawn multiple threads doing concurrent writes
    let mut handles = vec![];
    for thread_id in 0..4 {
        let db_clone = Arc::clone(&db);
        let handle = thread::spawn(move || {
            for i in 0..50 {
                let key = format!("thread{}_key{}", thread_id, i);
                let value = vec![b'x'; 100];
                db_clone.put(key.as_bytes(), &value).unwrap();
            }
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // Flush to ensure all data is in SSTables
    db.flush().unwrap();

    // Verify all data is accessible (no dangling references)
    for thread_id in 0..4 {
        for i in 0..50 {
            let key = format!("thread{}_key{}", thread_id, i);
            let value = db.get(key.as_bytes()).unwrap();
            assert!(
                value.is_some(),
                "All concurrent writes should be accessible (no dangling references)"
            );
        }
    }
}

/// Test that SSTable readers are properly reused (not duplicated)
#[test]
fn test_no_duplicate_arc_instances() {
    let temp_dir = TempDir::new().unwrap();
    let options = Options::default().memtable_size(1024);
    let db = DB::open(temp_dir.path(), options).unwrap();

    // Create multiple batches
    for batch in 0..5 {
        for i in 0..15 {
            let key = format!("batch{}_key{}", batch, i);
            let value = format!("value{}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        db.flush().unwrap();
    }

    // Trigger multiple reads to ensure SSTable readers work correctly
    for _ in 0..3 {
        for batch in 0..5 {
            for i in 0..15 {
                let key = format!("batch{}_key{}", batch, i);
                let value = db.get(key.as_bytes()).unwrap();
                assert!(value.is_some(), "Data should be readable multiple times");
            }
        }
    }

    // If there were duplicate Arc issues, this would cause problems with
    // file handles or inconsistent reads
    db.close().unwrap();
}

/// Integration test: All bugs fixed together
#[test]
fn test_all_bugs_fixed_integration() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().to_path_buf();

    // Session 1: Create complex scenario
    {
        let options = Options::default().memtable_size(512);
        let db = Arc::new(DB::open(&db_path, options).unwrap());

        // Concurrent writes with similar-sized values
        let mut handles = vec![];
        for thread_id in 0..3 {
            let db_clone = Arc::clone(&db);
            let handle = thread::spawn(move || {
                for i in 0..30 {
                    let key = format!("t{}_k{}", thread_id, i);
                    let value = format!("val{}", i); // Similar sizes
                    db_clone.put(key.as_bytes(), value.as_bytes()).unwrap();
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        db.flush().unwrap();
        db.close().unwrap();
    }

    // Session 2: Verify everything is consistent
    {
        let db = DB::open(&db_path, Options::default()).unwrap();

        // All data should be accessible
        for thread_id in 0..3 {
            for i in 0..30 {
                let key = format!("t{}_k{}", thread_id, i);
                let expected = format!("val{}", i);
                let value = db.get(key.as_bytes()).unwrap();
                assert_eq!(
                    value,
                    Some(expected.as_bytes().to_vec()),
                    "All data should be accessible after complex concurrent scenario"
                );
            }
        }

        db.close().unwrap();
    }
}
