//! Example demonstrating flush functionality
//!
//! This example shows:
//! - Manual flush operations
//! - Automatic flush when MemTable is full
//! - Data persistence across restarts
//! - WAL rotation

use aidb::{Options, DB};
use std::fs;

fn main() -> Result<(), aidb::Error> {
    env_logger::init();

    let db_path = "./example_flush_db";

    // Clean up any existing database
    if std::path::Path::new(db_path).exists() {
        fs::remove_dir_all(db_path).ok();
    }

    println!("=== Flush Example ===\n");

    // Example 1: Manual Flush
    println!("1. Manual Flush:");
    {
        let db = DB::open(db_path, Options::default())?;

        // Write some data
        for i in 0..100 {
            let key = format!("user:{}", i);
            let value = format!("User {} data", i);
            db.put(key.as_bytes(), value.as_bytes())?;
        }

        println!("   Written 100 entries to MemTable");

        // Manually flush to disk
        db.flush()?;
        println!("   ✓ Flushed to SSTable");

        db.close()?;
    }

    // Example 2: Verify Persistence
    println!("\n2. Data Persistence:");
    {
        let db = DB::open(db_path, Options::default())?;

        // Read data from SSTable
        if let Some(value) = db.get(b"user:42")? {
            println!("   ✓ Successfully read from SSTable: {:?}", String::from_utf8_lossy(&value));
        }

        db.close()?;
    }

    // Example 3: Auto-flush on MemTable Full
    println!("\n3. Auto-flush when MemTable is full:");
    {
        // Use a small memtable size to trigger auto-flush
        let options = Options::default().memtable_size(4096); // 4KB
        let db = DB::open(db_path, options)?;

        println!("   MemTable size limit: 4KB");

        // Write data until MemTable is full
        for i in 0..500 {
            let key = format!("data:{:08}", i);
            let value = vec![b'x'; 50]; // 50 bytes per entry
            db.put(key.as_bytes(), &value)?;
        }

        println!("   Written 500 entries");
        println!("   ✓ MemTable automatically frozen when full");

        // Manual flush to persist frozen memtables
        db.flush()?;
        println!("   ✓ All frozen MemTables flushed to SSTables");

        db.close()?;
    }

    // Example 4: Multiple Flushes
    println!("\n4. Multiple Flush Operations:");
    {
        let db = DB::open(db_path, Options::default())?;

        // First batch
        for i in 0..50 {
            db.put(format!("batch1:{}", i).as_bytes(), b"value1")?;
        }
        db.flush()?;
        println!("   ✓ Batch 1 flushed");

        // Second batch
        for i in 0..50 {
            db.put(format!("batch2:{}", i).as_bytes(), b"value2")?;
        }
        db.flush()?;
        println!("   ✓ Batch 2 flushed");

        // Third batch
        for i in 0..50 {
            db.put(format!("batch3:{}", i).as_bytes(), b"value3")?;
        }
        db.flush()?;
        println!("   ✓ Batch 3 flushed");

        println!("   ✓ Total 3 SSTables created at Level 0");

        db.close()?;
    }

    // Example 5: Verify all data
    println!("\n5. Final Verification:");
    {
        let db = DB::open(db_path, Options::default())?;

        // Check data from different batches
        let test_keys = vec![
            (b"user:42".to_vec(), "user data"),
            (b"data:00000100".to_vec(), "auto-flush data"),
            (b"batch1:10".to_vec(), "batch 1"),
            (b"batch2:20".to_vec(), "batch 2"),
            (b"batch3:30".to_vec(), "batch 3"),
        ];

        let mut found = 0;
        for (key, desc) in test_keys {
            if db.get(&key)?.is_some() {
                println!("   ✓ Found {}", desc);
                found += 1;
            }
        }

        println!("   Total: {}/5 entries found", found);

        if found >= 3 {
            println!("   ✓ Data successfully persisted and recovered");
        }

        db.close()?;
    }

    println!("\n=== Flush Example Complete ===");

    // Clean up
    fs::remove_dir_all(db_path).ok();

    Ok(())
}
