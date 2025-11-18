//! Example demonstrating Prometheus metrics collection and export
//!
//! This example shows how to:
//! 1. Start a metrics server
//! 2. Perform database operations
//! 3. Export metrics in Prometheus format
//!
//! Run with: cargo run --example metrics_server --features monitoring

use aidb::monitoring::MetricsServer;
use aidb::{Options, DB};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // Create metrics server on port 9090
    let addr = "127.0.0.1:9090".parse()?;
    let server = MetricsServer::new(addr);
    let collector = server.collector();

    // Start metrics server in background
    tokio::spawn(async move {
        if let Err(e) = server.run().await {
            eprintln!("Metrics server error: {}", e);
        }
    });

    println!("Metrics server started on http://127.0.0.1:9090");
    println!("View metrics at: http://127.0.0.1:9090/metrics");
    println!();

    // Open database
    let temp_dir = tempfile::TempDir::new()?;
    let db = Arc::new(DB::open(temp_dir.path(), Options::default())?);

    println!("Performing database operations...");
    println!();

    // Simulate various operations and record metrics
    for i in 0..100 {
        let key = format!("key{:03}", i);
        let value = format!("value{:03}", i);

        // Record put operation
        let start = std::time::Instant::now();
        db.put(key.as_bytes(), value.as_bytes())?;
        let duration = start.elapsed().as_secs_f64();
        collector.record_put_success(duration);

        // Record get operation
        let start = std::time::Instant::now();
        let _ = db.get(key.as_bytes())?;
        let duration = start.elapsed().as_secs_f64();
        collector.record_get_success(duration);
    }

    // Record some cache operations
    for _ in 0..10 {
        collector.record_cache_hit("block_cache");
    }
    for _ in 0..3 {
        collector.record_cache_miss("block_cache");
    }

    // Flush and record metrics
    let start = std::time::Instant::now();
    db.flush()?;
    let duration = start.elapsed().as_secs_f64();
    collector.record_flush(duration, true);

    // Update system metrics
    collector.update_memory_usage("memtable", 1024 * 1024); // 1MB
    collector.update_disk_usage("/data", 10 * 1024 * 1024); // 10MB
    collector.update_wal_size(512 * 1024); // 512KB

    // Update SSTable metrics
    collector.update_sstable_stats(0, 5, 5 * 1024 * 1024); // Level 0: 5 files, 5MB
    collector.update_sstable_stats(1, 10, 50 * 1024 * 1024); // Level 1: 10 files, 50MB

    println!("Database operations completed!");
    println!();

    // Export metrics
    println!("Current metrics:");
    println!("================");
    let metrics_text = collector.export()?;
    println!("{}", metrics_text);

    println!();
    println!("Server will continue running for 30 seconds...");
    println!("Access http://127.0.0.1:9090/metrics to view in Prometheus format");
    println!("Access http://127.0.0.1:9090/health for health check");
    println!("Access http://127.0.0.1:9090/ for index page");

    // Keep server running
    sleep(Duration::from_secs(30)).await;

    Ok(())
}
