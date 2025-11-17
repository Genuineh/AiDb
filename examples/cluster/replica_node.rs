//! Example: Running a Replica node
//!
//! This example demonstrates how to start a Replica node that caches
//! frequently accessed data and forwards misses to a Primary node.
//!
//! Start a Primary node first, then run:
//!   cargo run --example replica_node --features cluster

use aidb::cluster::ReplicaNode;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("Starting Replica node...");

    // Connect to primary node
    let primary_addr = "http://127.0.0.1:50051".to_string();
    let cache_capacity = 1000; // Cache up to 1000 entries

    let mut replica = ReplicaNode::new(primary_addr, cache_capacity).await?;
    println!("Replica node connected to primary");

    // Check if primary is healthy
    match replica.health_check().await {
        Ok(true) => println!("Primary node is healthy"),
        Ok(false) => println!("WARNING: Primary node is not healthy"),
        Err(e) => {
            eprintln!("Error checking primary health: {}", e);
            return Ok(());
        }
    }

    // Warm up cache with some keys
    println!("Warming up cache...");
    let warmup_keys = vec![b"hello".to_vec(), b"foo".to_vec()];
    let warmed = replica.warmup(warmup_keys).await?;
    println!("Warmed up {} keys", warmed);

    // Simulate some read operations
    println!("\nSimulating read operations...");
    for _ in 0..5 {
        // This should be a cache hit
        match replica.get(b"hello").await {
            Ok(Some(value)) => println!("Got value: {:?}", String::from_utf8_lossy(&value)),
            Ok(None) => println!("Key not found"),
            Err(e) => eprintln!("Error: {}", e),
        }

        sleep(Duration::from_secs(1)).await;
    }

    // Print statistics
    let stats = replica.stats();
    println!("\n=== Replica Statistics ===");
    println!("Total requests: {}", stats.total_requests);
    println!("Cache hits: {}", stats.cache_hits);
    println!("Cache misses: {}", stats.cache_misses);
    println!("Hit rate: {:.2}%", stats.hit_rate() * 100.0);
    println!("Forwarded requests: {}", stats.forwarded_requests);
    println!("Cache size: {}", replica.cache_size());

    Ok(())
}
