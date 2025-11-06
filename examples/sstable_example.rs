//! Example demonstrating SSTable usage.
//!
//! This example shows how to:
//! - Build an SSTable from key-value pairs
//! - Read data from an SSTable
//! - Iterate over all entries

use aidb::sstable::{SSTableBuilder, SSTableReader};
use aidb::Result;
use std::fs;

fn main() -> Result<()> {
    println!("=== SSTable Example ===\n");

    // Create a temporary directory for our example
    let temp_dir = std::env::temp_dir().join("aidb_sstable_example");
    fs::create_dir_all(&temp_dir)?;
    let sstable_path = temp_dir.join("example.sst");

    // === Part 1: Building an SSTable ===
    println!("1. Building an SSTable...");
    {
        let mut builder = SSTableBuilder::new(&sstable_path)?;

        // Configure the builder (optional)
        builder.set_block_size(4096); // 4KB blocks

        // Add key-value pairs (must be in sorted order)
        let entries: Vec<(&[u8], &[u8])> = vec![
            (b"apple", b"A red or green fruit"),
            (b"banana", b"A yellow tropical fruit"),
            (b"cherry", b"A small red stone fruit"),
            (b"date", b"A sweet brown fruit from palm trees"),
            (b"elderberry", b"A dark purple berry"),
            (b"fig", b"A soft sweet fruit with many seeds"),
            (b"grape", b"A small round fruit that grows in clusters"),
        ];

        for (key, value) in &entries {
            builder.add(*key, *value)?;
        }

        let file_size = builder.finish()?;
        println!("   ✓ SSTable created: {} bytes", file_size);
        println!("   ✓ {} entries written\n", entries.len());
    }

    // === Part 2: Reading from an SSTable ===
    println!("2. Reading from the SSTable...");
    {
        let reader = SSTableReader::open(&sstable_path)?;

        println!("   File size: {} bytes", reader.file_size());
        println!("   Number of blocks: {}\n", reader.num_blocks());

        // Read specific keys
        println!("   Looking up keys:");
        let keys_to_lookup: Vec<&[u8]> = vec![b"banana", b"fig", b"mango"];

        for key in &keys_to_lookup {
            match reader.get(*key)? {
                Some(value) => {
                    println!(
                        "     '{}' -> '{}'",
                        String::from_utf8_lossy(key),
                        String::from_utf8_lossy(&value)
                    );
                }
                None => {
                    println!("     '{}' -> NOT FOUND", String::from_utf8_lossy(key));
                }
            }
        }
        println!();

        // Get smallest and largest keys
        if let Some(smallest) = reader.smallest_key()? {
            println!("   Smallest key: '{}'", String::from_utf8_lossy(&smallest));
        }
        if let Some(largest) = reader.largest_key()? {
            println!("   Largest key: '{}'", String::from_utf8_lossy(&largest));
        }
        println!();
    }

    // === Part 3: Iterating over all entries ===
    println!("3. Iterating over all entries...");
    {
        let reader = SSTableReader::open(&sstable_path)?;
        let mut iter = reader.iter();

        iter.seek_to_first()?;

        let mut count = 0;
        while iter.next()? {
            if iter.valid() {
                let key = String::from_utf8_lossy(iter.key());
                let value = String::from_utf8_lossy(iter.value());
                println!("   {} -> {}", key, value);
                count += 1;
            }
        }
        println!("   ✓ Iterated {} entries\n", count);
    }

    // === Part 4: Building a large SSTable ===
    println!("4. Building a large SSTable...");
    {
        let large_path = temp_dir.join("large.sst");
        let mut builder = SSTableBuilder::new(&large_path)?;
        builder.set_block_size(1024); // Smaller blocks to demonstrate multiple blocks

        // Add many entries
        for i in 0..10000 {
            let key = format!("key{:08}", i);
            let value = format!("value_{:08}_with_some_extra_data", i);
            builder.add(key.as_bytes(), value.as_bytes())?;
        }

        let file_size = builder.finish()?;
        println!("   ✓ Large SSTable created: {} bytes", file_size);

        // Read it back
        let reader = SSTableReader::open(&large_path)?;
        println!("   ✓ Number of blocks: {}", reader.num_blocks());

        // Random access test
        let test_key = b"key00005000";
        if let Some(value) = reader.get(test_key)? {
            println!(
                "   ✓ Random access: {} -> {}",
                String::from_utf8_lossy(test_key),
                String::from_utf8_lossy(&value)
            );
        }
    }

    // Clean up
    println!("\n✓ Example completed successfully!");
    println!("  (Temporary files at: {})", temp_dir.display());

    Ok(())
}
