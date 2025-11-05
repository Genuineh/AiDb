//! Example demonstrating WAL (Write-Ahead Log) usage.
//!
//! This example shows how to:
//! - Write entries to the WAL
//! - Sync data to disk
//! - Recover data from WAL
//! - Handle large entries that get fragmented

use aidb::wal::{WALReader, WALWriter, WAL};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("=== WAL Example ===\n");

    // Example 1: Basic usage
    basic_usage()?;

    // Example 2: Large data handling
    large_data_example()?;

    // Example 3: Crash recovery simulation
    crash_recovery_example()?;

    println!("\n=== All examples completed successfully! ===");
    Ok(())
}

/// Basic WAL usage: write and read
fn basic_usage() -> Result<(), Box<dyn Error>> {
    println!("1. Basic Usage Example");
    println!("{}", "-".repeat(50));

    let wal_path = "/tmp/example_basic.wal";

    // Write some entries
    {
        println!("Writing entries to WAL...");
        let mut wal = WAL::open(wal_path)?;

        let entries = [
            b"user:1:name:Alice".to_vec(),
            b"user:1:email:alice@example.com".to_vec(),
            b"user:2:name:Bob".to_vec(),
            b"user:2:email:bob@example.com".to_vec(),
        ];

        for (i, entry) in entries.iter().enumerate() {
            wal.append(entry)?;
            println!("  Written entry {}: {} bytes", i + 1, entry.len());
        }

        // Sync to ensure durability
        wal.sync()?;
        println!("Synced {} bytes to disk", wal.size());
    }

    // Recover the entries
    {
        println!("\nRecovering entries from WAL...");
        let recovered = WAL::recover(wal_path)?;

        println!("Recovered {} entries:", recovered.len());
        for (i, entry) in recovered.iter().enumerate() {
            let data = String::from_utf8_lossy(entry);
            println!("  Entry {}: {}", i + 1, data);
        }
    }

    // Cleanup
    std::fs::remove_file(wal_path).ok();

    println!();
    Ok(())
}

/// Example with large data that gets fragmented
fn large_data_example() -> Result<(), Box<dyn Error>> {
    println!("2. Large Data Example");
    println!("{}", "-".repeat(50));

    let wal_path = "/tmp/example_large.wal";

    // Generate large data (100KB)
    let large_data = vec![0xAB; 100_000];

    // Write large entry
    {
        println!("Writing large entry (100KB)...");
        let mut writer = WALWriter::new(wal_path)?;
        writer.append(&large_data)?;
        writer.sync()?;
        println!(
            "Written {} bytes (file size: {} bytes)",
            large_data.len(),
            writer.file_size()
        );
    }

    // Recover large entry
    {
        println!("\nRecovering large entry...");
        let mut reader = WALReader::new(wal_path)?;
        if let Some(recovered) = reader.read_next()? {
            println!(
                "Recovered {} bytes (matches original: {})",
                recovered.len(),
                recovered == large_data
            );
        }
    }

    // Cleanup
    std::fs::remove_file(wal_path).ok();

    println!();
    Ok(())
}

/// Simulate crash recovery scenario
fn crash_recovery_example() -> Result<(), Box<dyn Error>> {
    println!("3. Crash Recovery Example");
    println!("{}", "-".repeat(50));

    let wal_path = "/tmp/example_recovery.wal";

    // Simulate normal operation
    {
        println!("Simulating normal operation...");
        let mut wal = WAL::open(wal_path)?;

        // Write some entries
        wal.append(b"operation:1:start")?;
        wal.append(b"operation:1:data:important_value")?;
        wal.sync()?;

        wal.append(b"operation:2:start")?;
        wal.append(b"operation:2:data:another_value")?;
        wal.sync()?;

        println!("  Written 4 operations");

        // Simulate a crash - wal is dropped without calling close
        // In a real scenario, this would be a power failure or process kill
    }

    // Simulate restart and recovery
    {
        println!("\nSimulating restart and recovery...");
        let recovered = WAL::recover(wal_path)?;

        println!("Recovered {} operations:", recovered.len());
        for (i, entry) in recovered.iter().enumerate() {
            let data = String::from_utf8_lossy(entry);
            println!("  Op {}: {}", i + 1, data);
        }

        println!("\n✓ All operations recovered successfully!");
    }

    // Continue normal operation
    {
        println!("\nContinuing normal operation...");
        let mut wal = WAL::open(wal_path)?;
        wal.append(b"operation:3:start")?;
        wal.sync()?;
        println!("  Written new operation");
        println!("  Current WAL size: {} bytes", wal.size());
    }

    // Cleanup
    std::fs::remove_file(wal_path).ok();

    println!();
    Ok(())
}
