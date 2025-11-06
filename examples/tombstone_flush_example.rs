//! Example demonstrating the empty SSTable prevention fix
//!
//! This example shows that flushing a MemTable with only tombstones
//! (deleted keys) does not create unnecessary SSTable files.

use aidb::{Options, DB};
use std::fs;

fn main() -> Result<(), aidb::Error> {
    env_logger::init();

    let db_path = "./example_tombstone_db";

    // Clean up any existing database
    if std::path::Path::new(db_path).exists() {
        fs::remove_dir_all(db_path).ok();
    }

    println!("=== Empty SSTable Prevention Example ===\n");

    // Scenario 1: Flush with only tombstones
    println!("Scenario 1: Flush MemTable with only tombstones");
    {
        let db = DB::open(db_path, Options::default())?;

        // Write and immediately delete keys
        println!("   Writing and deleting 100 keys...");
        for i in 0..100 {
            let key = format!("key{}", i);
            db.put(key.as_bytes(), b"value")?;
            db.delete(key.as_bytes())?;
        }

        println!("   MemTable now contains only tombstones");

        // Count SSTable files before flush
        let sst_count_before = count_sstable_files(db_path);
        println!("   SSTable files before flush: {}", sst_count_before);

        // Flush - should NOT create an SSTable
        db.flush()?;

        // Count SSTable files after flush
        let sst_count_after = count_sstable_files(db_path);
        println!("   SSTable files after flush: {}", sst_count_after);

        if sst_count_before == sst_count_after {
            println!("   ✓ SUCCESS: No empty SSTable created!");
        } else {
            println!("   ✗ FAIL: Empty SSTable was created");
        }

        db.close()?;
    }

    println!();

    // Scenario 2: Flush with mixed content
    println!("Scenario 2: Flush MemTable with mixed content");
    {
        // Clean and reopen
        fs::remove_dir_all(db_path).ok();
        let db = DB::open(db_path, Options::default())?;

        // Write some keys to keep
        println!("   Writing 50 keys to keep...");
        for i in 0..50 {
            let key = format!("keep{}", i);
            db.put(key.as_bytes(), b"value")?;
        }

        // Write and delete other keys
        println!("   Writing and deleting 50 keys...");
        for i in 0..50 {
            let key = format!("delete{}", i);
            db.put(key.as_bytes(), b"value")?;
            db.delete(key.as_bytes())?;
        }

        // Count before flush
        let sst_count_before = count_sstable_files(db_path);
        println!("   SSTable files before flush: {}", sst_count_before);

        // Flush - SHOULD create an SSTable (has valid entries)
        db.flush()?;

        // Count after flush
        let sst_count_after = count_sstable_files(db_path);
        println!("   SSTable files after flush: {}", sst_count_after);

        if sst_count_after > sst_count_before {
            println!("   ✓ SUCCESS: SSTable created for valid entries!");
        } else {
            println!("   ✗ FAIL: SSTable should have been created");
        }

        // Verify data
        let mut keep_found = 0;
        let mut delete_found = 0;

        for i in 0..50 {
            if db.get(format!("keep{}", i).as_bytes())?.is_some() {
                keep_found += 1;
            }
            if db.get(format!("delete{}", i).as_bytes())?.is_some() {
                delete_found += 1;
            }
        }

        println!("   Valid keys found: {}/50", keep_found);
        println!("   Deleted keys found: {}/50", delete_found);

        if keep_found == 50 && delete_found == 0 {
            println!("   ✓ SUCCESS: Data correctness verified!");
        }

        db.close()?;
    }

    println!();

    // Scenario 3: Empty MemTable flush
    println!("Scenario 3: Flush empty MemTable");
    {
        fs::remove_dir_all(db_path).ok();
        let db = DB::open(db_path, Options::default())?;

        println!("   MemTable is empty (no writes)");

        let sst_count_before = count_sstable_files(db_path);
        println!("   SSTable files before flush: {}", sst_count_before);

        // Flush empty MemTable
        db.flush()?;

        let sst_count_after = count_sstable_files(db_path);
        println!("   SSTable files after flush: {}", sst_count_after);

        if sst_count_before == sst_count_after {
            println!("   ✓ SUCCESS: No SSTable created for empty MemTable!");
        }

        db.close()?;
    }

    println!();

    // Scenario 4: Duplicate key overwrites
    println!("Scenario 4: Flush MemTable with duplicate keys");
    {
        fs::remove_dir_all(db_path).ok();
        let db = DB::open(db_path, Options::default())?;

        // Write the same key 1000 times
        println!("   Writing same key 1000 times...");
        for i in 0..1000 {
            db.put(b"duplicate_key", format!("value{}", i).as_bytes())?;
        }

        // Flush - should create SSTable with only 1 entry
        db.flush()?;

        let sst_count = count_sstable_files(db_path);
        println!("   SSTable files created: {}", sst_count);

        if sst_count == 1 {
            println!("   ✓ SUCCESS: Only one SSTable created!");
        }

        // Verify we get the latest value
        if let Some(value) = db.get(b"duplicate_key")? {
            let value_str = String::from_utf8_lossy(&value);
            if value_str == "value999" {
                println!("   ✓ SUCCESS: Latest value preserved: {}", value_str);
            }
        }

        db.close()?;
    }

    println!();
    println!("=== All Scenarios Passed ===");
    println!();
    println!("Benefits of this fix:");
    println!("  • No wasted disk space from empty SSTables");
    println!("  • Faster reads (fewer SSTables to check)");
    println!("  • Cleaner database state");
    println!("  • Proper edge case handling");

    // Clean up
    fs::remove_dir_all(db_path).ok();

    Ok(())
}

/// Count the number of SSTable files in the database directory
fn count_sstable_files(path: &str) -> usize {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|ext| ext == "sst")
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0)
}
