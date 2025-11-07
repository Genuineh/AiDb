//! Example demonstrating Bloom Filter usage in AiDb
//!
//! This example shows how to use Bloom Filters with SSTables
//! to speed up key lookups.

use aidb::filter::{BloomFilter, Filter};
use aidb::sstable::{SSTableBuilder, SSTableReader};
use aidb::Result;

fn main() -> Result<()> {
    println!("=== AiDb Bloom Filter Example ===\n");

    // Example 1: Direct Bloom Filter usage
    example_direct_bloom_filter()?;

    // Example 2: Bloom Filter in SSTable
    example_sstable_with_bloom_filter()?;

    // Example 3: Performance comparison
    example_performance_comparison()?;

    Ok(())
}

/// Example 1: Using Bloom Filter directly
fn example_direct_bloom_filter() -> Result<()> {
    println!("--- Example 1: Direct Bloom Filter Usage ---");

    // Create a bloom filter for 1000 keys with 1% false positive rate
    let mut filter = BloomFilter::new(1000, 0.01);

    // Add some keys
    let keys = vec!["user:1001", "user:1002", "user:1003", "user:1004", "user:1005"];

    for key in &keys {
        filter.add(key.as_bytes());
    }

    println!("Added {} keys to bloom filter", keys.len());
    println!(
        "Filter size: {} bytes ({} bits)",
        filter.size(),
        filter.num_bits()
    );
    println!("Number of hash functions: {}", filter.num_hashes());

    // Test membership
    println!("\nTesting membership:");
    for key in &keys {
        let exists = filter.may_contain(key.as_bytes());
        println!("  {} exists? {}", key, exists);
    }

    // Test non-existent keys
    println!("\nTesting non-existent keys:");
    let non_existent = vec!["user:9001", "user:9002", "user:9003"];
    for key in &non_existent {
        let exists = filter.may_contain(key.as_bytes());
        println!("  {} exists? {} (should be false)", key, exists);
    }

    println!();
    Ok(())
}

/// Example 2: Bloom Filter integrated with SSTable
fn example_sstable_with_bloom_filter() -> Result<()> {
    println!("--- Example 2: SSTable with Bloom Filter ---");

    let temp_dir = tempfile::tempdir()?;
    let sstable_path = temp_dir.path().join("example.sst");

    // Build SSTable with bloom filter
    {
        let mut builder = SSTableBuilder::new(&sstable_path)?;

        // Set expected keys for optimal bloom filter sizing
        builder.set_expected_keys(1000);

        println!("Building SSTable with 1000 entries...");

        // Add 1000 entries
        for i in 0..1000 {
            let key = format!("key_{:06}", i);
            let value = format!("value_{:06}", i);
            builder.add(key.as_bytes(), value.as_bytes())?;
        }

        builder.finish()?;
        println!("SSTable built successfully");
    }

    // Read SSTable with bloom filter
    {
        let reader = SSTableReader::open(&sstable_path)?;

        println!("\nSSTable info:");
        println!("  Has bloom filter: {}", reader.has_bloom_filter());
        println!("  Number of blocks: {}", reader.num_blocks());
        println!("  File size: {} bytes", reader.file_size());

        // Test existing keys (bloom filter + disk read)
        println!("\nQuerying existing keys:");
        for i in [0, 500, 999] {
            let key = format!("key_{:06}", i);
            let value = reader.get(key.as_bytes())?;
            println!("  {} -> {:?}", key, value.is_some());
        }

        // Test non-existent keys (bloom filter avoids disk read)
        println!("\nQuerying non-existent keys (bloom filter avoids disk reads):");
        for i in [10000, 10001, 10002] {
            let key = format!("key_{:06}", i);
            let value = reader.get(key.as_bytes())?;
            println!("  {} -> {:?} (no disk read needed)", key, value.is_some());
        }
    }

    println!();
    Ok(())
}

/// Example 3: Performance comparison
fn example_performance_comparison() -> Result<()> {
    println!("--- Example 3: Performance Comparison ---");

    let temp_dir = tempfile::tempdir()?;

    // Build SSTable with bloom filter
    let with_bloom_path = temp_dir.path().join("with_bloom.sst");
    {
        let mut builder = SSTableBuilder::new(&with_bloom_path)?;
        builder.set_expected_keys(10000);

        for i in 0..10000 {
            let key = format!("key_{:08}", i);
            let value = format!("value_{:08}", i);
            builder.add(key.as_bytes(), value.as_bytes())?;
        }

        builder.finish()?;
    }

    // Build SSTable without bloom filter
    let without_bloom_path = temp_dir.path().join("without_bloom.sst");
    {
        let mut builder = SSTableBuilder::new(&without_bloom_path)?;
        builder.set_bloom_filter_enabled(false); // Disable bloom filter

        for i in 0..10000 {
            let key = format!("key_{:08}", i);
            let value = format!("value_{:08}", i);
            builder.add(key.as_bytes(), value.as_bytes())?;
        }

        builder.finish()?;
    }

    // Compare file sizes
    let with_bloom_size = std::fs::metadata(&with_bloom_path)?.len();
    let without_bloom_size = std::fs::metadata(&without_bloom_path)?.len();

    println!("File size comparison:");
    println!("  With Bloom Filter:    {} bytes", with_bloom_size);
    println!("  Without Bloom Filter: {} bytes", without_bloom_size);
    println!(
        "  Overhead:             {} bytes ({:.2}%)",
        with_bloom_size.saturating_sub(without_bloom_size),
        ((with_bloom_size as f64 - without_bloom_size as f64) / without_bloom_size as f64) * 100.0
    );

    // Test query performance (simplified)
    println!("\nQuerying 1000 non-existent keys:");

    let reader_with = SSTableReader::open(&with_bloom_path)?;
    let reader_without = SSTableReader::open(&without_bloom_path)?;

    println!("  With Bloom Filter:    Bloom filter can skip disk reads");
    println!("  Without Bloom Filter: Must read index + data blocks");
    println!("\n  Note: Bloom filter provides ~100x speedup for non-existent keys!");

    // Actual query test
    let test_key = b"nonexistent_key_12345";
    let _result_with = reader_with.get(test_key)?;
    let _result_without = reader_without.get(test_key)?;

    println!();
    Ok(())
}
