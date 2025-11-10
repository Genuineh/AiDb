//! Example: Using WriteBatch and Compression Features
//!
//! This example demonstrates the new Week 13-14 features:
//! - WriteBatch for atomic batch operations
//! - Compression configuration
//! - Performance monitoring

use aidb::{config::CompressionType, Options, WriteBatch, DB};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== AiDb Week 13-14 Features Demo ===\n");

    // Example 1: Basic WriteBatch Usage
    example_write_batch()?;

    // Example 2: Compression Configuration
    example_compression()?;

    // Example 3: Batch Performance
    example_batch_performance()?;

    // Example 4: Cache Statistics
    example_cache_stats()?;

    Ok(())
}

/// Example 1: Using WriteBatch for atomic operations
fn example_write_batch() -> Result<(), Box<dyn std::error::Error>> {
    println!("Example 1: WriteBatch");
    println!("---------------------");

    let temp_dir = tempfile::tempdir()?;
    let db = DB::open(temp_dir.path(), Options::default())?;

    // Create a batch of operations
    let mut batch = WriteBatch::new();
    batch.put(b"user:1:name", b"Alice");
    batch.put(b"user:1:email", b"alice@example.com");
    batch.put(b"user:1:age", b"30");

    // Apply all operations atomically
    db.write(batch)?;

    println!("Batch written atomically!");

    // Verify data
    if let Some(name) = db.get(b"user:1:name")? {
        println!("User name: {}", String::from_utf8_lossy(&name));
    }

    // Update with another batch
    let mut update_batch = WriteBatch::new();
    update_batch.put(b"user:1:age", b"31"); // Update age
    update_batch.delete(b"user:1:email"); // Remove email
    update_batch.put(b"user:1:verified", b"true"); // Add new field

    db.write(update_batch)?;
    println!("User data updated!");

    println!();
    Ok(())
}

/// Example 2: Configuring compression
fn example_compression() -> Result<(), Box<dyn std::error::Error>> {
    println!("Example 2: Compression");
    println!("----------------------");

    let temp_dir = tempfile::tempdir()?;

    // Database without compression
    let opts_no_compress = Options::default().compression(CompressionType::None);
    let db_no_compress = DB::open(temp_dir.path().join("no_compress"), opts_no_compress)?;

    // Write some compressible data
    for i in 0..100 {
        let key = format!("key{}", i);
        let value = vec![b'x'; 1000]; // Highly compressible
        db_no_compress.put(key.as_bytes(), &value)?;
    }
    db_no_compress.flush()?;

    println!("Data written without compression");

    // Database with Snappy compression
    #[cfg(feature = "snappy")]
    {
        let opts_compress = Options::default().compression(CompressionType::Snappy);
        let db_compress = DB::open(temp_dir.path().join("with_compress"), opts_compress)?;

        for i in 0..100 {
            let key = format!("key{}", i);
            let value = vec![b'x'; 1000]; // Highly compressible
            db_compress.put(key.as_bytes(), &value)?;
        }
        db_compress.flush()?;

        println!("Data written with Snappy compression");
        println!("Note: Check disk usage to see compression benefits");
    }

    println!();
    Ok(())
}

/// Example 3: Comparing batch vs individual writes
fn example_batch_performance() -> Result<(), Box<dyn std::error::Error>> {
    println!("Example 3: Batch Performance");
    println!("-----------------------------");

    let temp_dir = tempfile::tempdir()?;
    let db = DB::open(temp_dir.path(), Options::default())?;

    // Individual writes
    let start = std::time::Instant::now();
    for i in 0..100 {
        let key = format!("individual:{}", i);
        let value = format!("value{}", i);
        db.put(key.as_bytes(), value.as_bytes())?;
    }
    let individual_time = start.elapsed();

    println!("100 individual writes: {:.2}ms", individual_time.as_secs_f64() * 1000.0);

    // Batch write
    let start = std::time::Instant::now();
    let mut batch = WriteBatch::new();
    for i in 0..100 {
        let key = format!("batch:{}", i);
        let value = format!("value{}", i);
        batch.put(key.as_bytes(), value.as_bytes());
    }
    db.write(batch)?;
    let batch_time = start.elapsed();

    println!("1 batch with 100 writes: {:.2}ms", batch_time.as_secs_f64() * 1000.0);

    if batch_time < individual_time {
        let speedup = individual_time.as_secs_f64() / batch_time.as_secs_f64();
        println!("Batch is {:.2}x faster!", speedup);
    }

    println!();
    Ok(())
}

/// Example 4: Monitoring cache statistics
fn example_cache_stats() -> Result<(), Box<dyn std::error::Error>> {
    println!("Example 4: Cache Statistics");
    println!("----------------------------");

    let temp_dir = tempfile::tempdir()?;
    let db = DB::open(temp_dir.path(), Options::default())?;

    // Write and flush data
    for i in 0..100 {
        let key = format!("key{}", i);
        let value = format!("value{}", i);
        db.put(key.as_bytes(), value.as_bytes())?;
    }
    db.flush()?;

    // Reset stats to get clean measurements
    db.reset_cache_stats();

    // First read - will miss cache
    let _ = db.get(b"key0")?;
    let stats = db.cache_stats();
    println!("After first read:");
    println!("  Hits: {}, Misses: {}", stats.hits, stats.misses);

    // Second read - should hit cache
    let _ = db.get(b"key0")?;
    let stats = db.cache_stats();
    println!("After second read:");
    println!("  Hits: {}, Misses: {}", stats.hits, stats.misses);
    println!("  Hit rate: {:.1}%", stats.hit_rate() * 100.0);

    // Read more keys
    for i in 1..10 {
        let key = format!("key{}", i);
        let _ = db.get(key.as_bytes())?;
    }

    let stats = db.cache_stats();
    println!("After reading 10 keys:");
    println!("  Lookups: {}", stats.lookups);
    println!("  Hits: {}, Misses: {}", stats.hits, stats.misses);
    println!("  Hit rate: {:.1}%", stats.hit_rate() * 100.0);

    // Clear cache and show effect
    db.clear_cache();
    let _ = db.get(b"key0")?;
    let stats = db.cache_stats();
    println!("After clearing cache and reading key0:");
    println!("  Cache should miss: Misses={}", stats.misses);

    println!();
    Ok(())
}
