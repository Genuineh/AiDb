//! Example demonstrating basic DB operations
//!
//! This example shows how to:
//! - Open a database
//! - Write key-value pairs
//! - Read values
//! - Delete keys
//! - Close the database

use aidb::{Options, DB};

fn main() -> Result<(), aidb::Error> {
    // Initialize logger
    env_logger::init();

    // Configure database options
    let options = Options::default()
        .memtable_size(4 * 1024 * 1024) // 4MB memtable
        .use_wal(true) // Enable write-ahead log
        .block_size(4096); // 4KB block size

    // Open or create database
    println!("Opening database...");
    let db = DB::open("./example_db", options)?;

    // Write some data
    println!("\n=== Writing Data ===");
    for i in 0..10 {
        let key = format!("user:{}", i);
        let value = format!("{{\"name\":\"User {}\",\"age\":{}}}", i, 20 + i);
        db.put(key.as_bytes(), value.as_bytes())?;
        println!("Put: {} -> {}", key, value);
    }

    // Read data
    println!("\n=== Reading Data ===");
    for i in 0..10 {
        let key = format!("user:{}", i);
        match db.get(key.as_bytes())? {
            Some(value) => {
                let value_str = String::from_utf8_lossy(&value);
                println!("Get: {} -> {}", key, value_str);
            }
            None => {
                println!("Get: {} -> Not found", key);
            }
        }
    }

    // Update a value
    println!("\n=== Updating Data ===");
    let key = "user:5";
    let new_value = r#"{"name":"Updated User 5","age":99}"#;
    db.put(key.as_bytes(), new_value.as_bytes())?;
    println!("Updated: {} -> {}", key, new_value);

    // Verify update
    if let Some(value) = db.get(key.as_bytes())? {
        let value_str = String::from_utf8_lossy(&value);
        println!("Verified: {} -> {}", key, value_str);
    }

    // Delete a key
    println!("\n=== Deleting Data ===");
    let delete_key = "user:3";
    db.delete(delete_key.as_bytes())?;
    println!("Deleted: {}", delete_key);

    // Verify deletion
    match db.get(delete_key.as_bytes())? {
        Some(_) => println!("ERROR: Key still exists!"),
        None => println!("Verified: {} is deleted", delete_key),
    }

    // Batch operations
    println!("\n=== Batch Operations ===");
    for i in 100..200 {
        let key = format!("key:{}", i);
        let value = format!("value:{}", i);
        db.put(key.as_bytes(), value.as_bytes())?;
    }
    println!("Inserted 100 key-value pairs");

    // Random reads
    println!("\n=== Random Reads ===");
    for i in [105, 150, 199] {
        let key = format!("key:{}", i);
        match db.get(key.as_bytes())? {
            Some(value) => {
                let value_str = String::from_utf8_lossy(&value);
                println!("Get: {} -> {}", key, value_str);
            }
            None => {
                println!("Get: {} -> Not found", key);
            }
        }
    }

    // Statistics
    println!("\n=== Statistics ===");
    println!("All operations completed successfully!");

    // Close the database
    println!("\nClosing database...");
    db.close()?;
    println!("Database closed successfully!");

    Ok(())
}
