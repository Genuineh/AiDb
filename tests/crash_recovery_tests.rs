// Crash Recovery Tests for AiDb
// These tests verify data consistency after simulated crashes

use aidb::{Options, DB};
use std::fs;
use tempfile::TempDir;

/// Helper function to simulate a crash by dropping DB without proper close
/// Uses mem::forget to prevent Drop from running (simulates abrupt termination)
fn simulate_crash(db: DB) {
    std::mem::forget(db);
}

/// Test recovery after crash during write operations
#[test]
fn test_recovery_after_write_crash() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    // First session: write data and simulate crash
    {
        let db = DB::open(&path, Options::default()).unwrap();

        // Write some data
        for i in 0..100 {
            let key = format!("key_{}", i);
            let value = format!("value_{}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }

        // Simulate crash (no clean shutdown)
        simulate_crash(db);
    }

    // Second session: recover and verify
    {
        let db = DB::open(&path, Options::default()).unwrap();

        // All WAL-persisted data should be recovered
        for i in 0..100 {
            let key = format!("key_{}", i);
            let expected = format!("value_{}", i);
            let result = db.get(key.as_bytes()).unwrap();
            assert_eq!(
                result,
                Some(expected.as_bytes().to_vec()),
                "Key {} should be recovered after crash",
                key
            );
        }
    }
}

/// Test recovery with partially written data
#[test]
fn test_recovery_partial_writes() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    // Session 1: Write and flush some data
    {
        let db = DB::open(&path, Options::default()).unwrap();

        for i in 0..50 {
            let key = format!("stable_key_{}", i);
            db.put(key.as_bytes(), b"stable_value").unwrap();
        }

        db.flush().unwrap();

        // Write more data without flush
        for i in 0..50 {
            let key = format!("partial_key_{}", i);
            db.put(key.as_bytes(), b"partial_value").unwrap();
        }

        simulate_crash(db);
    }

    // Session 2: Recover
    {
        let db = DB::open(&path, Options::default()).unwrap();

        // Flushed data should be present
        for i in 0..50 {
            let key = format!("stable_key_{}", i);
            assert_eq!(db.get(key.as_bytes()).unwrap(), Some(b"stable_value".to_vec()));
        }

        // WAL data should also be recovered
        for i in 0..50 {
            let key = format!("partial_key_{}", i);
            assert_eq!(db.get(key.as_bytes()).unwrap(), Some(b"partial_value".to_vec()));
        }
    }
}

/// Test recovery after crash during flush
#[test]
fn test_recovery_during_flush() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    let options = Options {
        memtable_size: 1024 * 64, // Small memtable to trigger flush
        ..Default::default()
    };

    // Session 1: Fill memtable to trigger auto-flush
    {
        let db = DB::open(&path, options.clone()).unwrap();

        // Write enough to trigger flush
        for i in 0..1000 {
            let key = format!("key_{:06}", i);
            let value = vec![b'x'; 100];
            db.put(key.as_bytes(), &value).unwrap();
        }

        // Note: Flush may be in progress, simulate crash
        simulate_crash(db);
    }

    // Session 2: Recover - data should be consistent
    {
        let db = DB::open(&path, options).unwrap();

        // Verify all data (either from SSTable or WAL)
        for i in 0..1000 {
            let key = format!("key_{:06}", i);
            let result = db.get(key.as_bytes()).unwrap();

            // Data should exist (either flushed or in WAL)
            assert!(result.is_some(), "Key {} should exist after recovery", key);
            assert_eq!(result.unwrap().len(), 100);
        }
    }
}

/// Test recovery with corrupted/incomplete WAL entry
#[test]
fn test_recovery_with_wal_corruption() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    // Session 1: Write data but crash before flush (so data is only in WAL)
    {
        let db = DB::open(&path, Options::default()).unwrap();

        // Write data that will only be in WAL (not flushed to SSTable)
        for i in 0..50 {
            let key = format!("wal_key_{}", i);
            db.put(key.as_bytes(), b"wal_value").unwrap();
        }

        // Simulate crash (don't flush, don't close properly)
        simulate_crash(db);
    }

    // Corrupt the WAL file by truncating it
    // WAL files are in the database root directory (path), not in a subdirectory
    let mut corrupted = false;
    if let Ok(entries) = fs::read_dir(&path) {
        for entry in entries.flatten() {
            let file_path = entry.path();
            // WAL files end with .log (e.g., 000001.log)
            if file_path.extension().and_then(|s| s.to_str()) == Some("log") {
                // Truncate the file to simulate corruption
                if let Ok(metadata) = fs::metadata(&file_path) {
                    let size = metadata.len();
                    if size > 100 {
                        println!(
                            "Corrupting WAL file: {:?} (truncating from {} to {} bytes)",
                            file_path,
                            size,
                            size / 2
                        );
                        // Truncate to half size, which should corrupt some entries
                        fs::write(&file_path, vec![0u8; (size / 2) as usize]).ok();
                        corrupted = true;
                    }
                }
            }
        }
    }

    assert!(corrupted, "WAL file should have been found and corrupted");

    // Session 2: Recovery should handle corruption gracefully
    {
        let db_result = DB::open(&path, Options::default());

        // Database should either:
        // 1. Open successfully and recover valid entries (up to corruption point)
        // 2. Return an error indicating corruption
        match db_result {
            Ok(db) => {
                // If opened, some data may be lost due to corruption
                println!("Database opened after WAL corruption");

                // Some keys might be recovered (before corruption point)
                // Some keys might be lost (after corruption point)
                let mut recovered = 0;
                let mut lost = 0;

                for i in 0..50 {
                    let key = format!("wal_key_{}", i);
                    if db.get(key.as_bytes()).unwrap().is_some() {
                        recovered += 1;
                    } else {
                        lost += 1;
                    }
                }

                println!("Recovered {} keys, lost {} keys due to WAL corruption", recovered, lost);

                // We expect some data loss due to corruption
                // (exact amount depends on where corruption occurred)
                assert!(lost > 0, "Some data should be lost due to WAL corruption");
            }
            Err(e) => {
                // Corruption detected and database refused to open
                println!("Database correctly detected WAL corruption: {:?}", e);
            }
        }
    }
}

/// Test recovery with deletes
#[test]
fn test_recovery_with_deletes() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    // Session 1: Write and delete data
    {
        let db = DB::open(&path, Options::default()).unwrap();

        // Write data
        for i in 0..100 {
            let key = format!("key_{}", i);
            db.put(key.as_bytes(), b"value").unwrap();
        }

        // Delete half of it
        for i in 0..50 {
            let key = format!("key_{}", i);
            db.delete(key.as_bytes()).unwrap();
        }

        simulate_crash(db);
    }

    // Session 2: Recover and verify
    {
        let db = DB::open(&path, Options::default()).unwrap();

        // Deleted keys should not exist
        for i in 0..50 {
            let key = format!("key_{}", i);
            assert_eq!(
                db.get(key.as_bytes()).unwrap(),
                None,
                "Deleted key {} should not exist",
                key
            );
        }

        // Remaining keys should exist
        for i in 50..100 {
            let key = format!("key_{}", i);
            assert_eq!(
                db.get(key.as_bytes()).unwrap(),
                Some(b"value".to_vec()),
                "Key {} should exist",
                key
            );
        }
    }
}

/// Test multiple crash-recovery cycles
#[test]
fn test_multiple_crash_recovery_cycles() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    // Cycle 1
    {
        let db = DB::open(&path, Options::default()).unwrap();
        db.put(b"cycle1", b"data1").unwrap();
        simulate_crash(db);
    }

    // Cycle 2
    {
        let db = DB::open(&path, Options::default()).unwrap();
        assert_eq!(db.get(b"cycle1").unwrap(), Some(b"data1".to_vec()));
        db.put(b"cycle2", b"data2").unwrap();
        simulate_crash(db);
    }

    // Cycle 3
    {
        let db = DB::open(&path, Options::default()).unwrap();
        assert_eq!(db.get(b"cycle1").unwrap(), Some(b"data1".to_vec()));
        assert_eq!(db.get(b"cycle2").unwrap(), Some(b"data2".to_vec()));
        db.put(b"cycle3", b"data3").unwrap();
        simulate_crash(db);
    }

    // Final verification
    {
        let db = DB::open(&path, Options::default()).unwrap();
        assert_eq!(db.get(b"cycle1").unwrap(), Some(b"data1".to_vec()));
        assert_eq!(db.get(b"cycle2").unwrap(), Some(b"data2".to_vec()));
        assert_eq!(db.get(b"cycle3").unwrap(), Some(b"data3".to_vec()));
    }
}

/// Test data consistency across crash
#[test]
fn test_data_consistency_after_crash() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    // Session 1: Create consistent state
    {
        let db = DB::open(&path, Options::default()).unwrap();

        // Write related data (e.g., user and their posts)
        db.put(b"user:1:name", b"Alice").unwrap();
        db.put(b"user:1:post:1", b"Hello World").unwrap();
        db.put(b"user:1:post:2", b"Second Post").unwrap();

        simulate_crash(db);
    }

    // Session 2: Verify consistency
    {
        let db = DB::open(&path, Options::default()).unwrap();

        // All related data should be present or none
        let name = db.get(b"user:1:name").unwrap();
        let post1 = db.get(b"user:1:post:1").unwrap();
        let post2 = db.get(b"user:1:post:2").unwrap();

        // Since WAL persists, all should be present
        assert_eq!(name, Some(b"Alice".to_vec()));
        assert_eq!(post1, Some(b"Hello World".to_vec()));
        assert_eq!(post2, Some(b"Second Post".to_vec()));
    }
}

/// Test recovery with empty database
#[test]
fn test_recovery_empty_database() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    // Session 1: Open and immediately crash
    {
        let db = DB::open(&path, Options::default()).unwrap();
        simulate_crash(db);
    }

    // Session 2: Should recover empty database
    {
        let db = DB::open(&path, Options::default()).unwrap();
        assert_eq!(db.get(b"any_key").unwrap(), None);
    }
}

/// Test recovery after proper shutdown (no crash)
#[test]
fn test_recovery_after_proper_shutdown() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    // Session 1: Normal operation with proper close
    {
        let db = DB::open(&path, Options::default()).unwrap();

        for i in 0..100 {
            let key = format!("key_{}", i);
            db.put(key.as_bytes(), b"value").unwrap();
        }

        // Proper shutdown (drop with flush)
        drop(db);
    }

    // Session 2: Should open normally
    {
        let db = DB::open(&path, Options::default()).unwrap();

        for i in 0..100 {
            let key = format!("key_{}", i);
            assert_eq!(db.get(key.as_bytes()).unwrap(), Some(b"value".to_vec()));
        }
    }
}

/// Test WAL replay correctness
#[test]
fn test_wal_replay_correctness() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    // Session 1: Perform operations that should be in WAL
    {
        let db = DB::open(&path, Options::default()).unwrap();

        // Initial write
        db.put(b"counter", b"0").unwrap();

        // Multiple updates
        for i in 1..=10 {
            db.put(b"counter", format!("{}", i).as_bytes()).unwrap();
        }

        simulate_crash(db);
    }

    // Session 2: Verify WAL replay gives correct final state
    {
        let db = DB::open(&path, Options::default()).unwrap();

        // Should have the last written value
        assert_eq!(db.get(b"counter").unwrap(), Some(b"10".to_vec()));
    }
}

/// Test recovery with mixed operations
#[test]
fn test_recovery_mixed_operations() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().to_path_buf();

    // Session 1: Mix of puts and deletes
    {
        let db = DB::open(&path, Options::default()).unwrap();

        db.put(b"key1", b"value1").unwrap();
        db.put(b"key2", b"value2").unwrap();
        db.delete(b"key1").unwrap();
        db.put(b"key3", b"value3").unwrap();
        db.put(b"key2", b"value2_updated").unwrap();

        simulate_crash(db);
    }

    // Session 2: Verify final state
    {
        let db = DB::open(&path, Options::default()).unwrap();

        assert_eq!(db.get(b"key1").unwrap(), None);
        assert_eq!(db.get(b"key2").unwrap(), Some(b"value2_updated".to_vec()));
        assert_eq!(db.get(b"key3").unwrap(), Some(b"value3".to_vec()));
    }
}
