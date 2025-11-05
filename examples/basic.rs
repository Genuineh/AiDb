//! Basic usage example for AiDb
//!
//! This example demonstrates the fundamental operations:
//! - Opening a database
//! - Writing key-value pairs
//! - Reading values
//! - Deleting keys

use aidb::{Options, DB};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger
    env_logger::init();

    // Configure database options
    let options = Options::default()
        .memtable_size(4 * 1024 * 1024) // 4MB
        .use_wal(true);

    // Open database (will be created if it doesn't exist)
    let db = DB::open("./example_data", options)?;

    println!("Database opened successfully");

    // Write some key-value pairs
    println!("Writing data...");
    db.put(b"key1", b"value1")?;
    db.put(b"key2", b"value2")?;
    db.put(b"key3", b"value3")?;

    // Read values
    println!("Reading data...");
    if let Some(value) = db.get(b"key1")? {
        println!("key1 => {:?}", String::from_utf8_lossy(&value));
    }

    // Delete a key
    println!("Deleting key2...");
    db.delete(b"key2")?;

    // Try to read deleted key
    match db.get(b"key2")? {
        Some(_) => println!("key2 still exists (unexpected)"),
        None => println!("key2 was successfully deleted"),
    }

    // Close database
    db.close()?;
    println!("Database closed");

    Ok(())
}
