//! Example demonstrating coordinator usage with multiple shards
//!
//! This example shows how to:
//! 1. Start multiple primary nodes (shards)
//! 2. Register shards with a coordinator
//! 3. Route requests through the coordinator
//! 4. Enable health checking
//!
//! Run with: cargo run --example coordinator_demo --features cluster

use aidb::cluster::{Coordinator, HealthCheckConfig, HealthChecker, PrimaryNode};
use aidb::{Options, DB};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("=== AiDb Coordinator Demo ===\n");

    // Create three database instances
    println!("1. Creating database instances...");
    let temp1 = TempDir::new()?;
    let temp2 = TempDir::new()?;
    let temp3 = TempDir::new()?;

    let db1 = Arc::new(DB::open(temp1.path(), Options::default())?);
    let db2 = Arc::new(DB::open(temp2.path(), Options::default())?);
    let db3 = Arc::new(DB::open(temp3.path(), Options::default())?);

    // Start three primary nodes on different ports
    println!("2. Starting primary nodes...");
    let primary1 = PrimaryNode::new(db1.clone());
    let primary2 = PrimaryNode::new(db2.clone());
    let primary3 = PrimaryNode::new(db3.clone());

    let addr1 = "127.0.0.1:50051".parse().unwrap();
    let addr2 = "127.0.0.1:50052".parse().unwrap();
    let addr3 = "127.0.0.1:50053".parse().unwrap();

    tokio::spawn(async move {
        println!("   Shard 1 listening on 127.0.0.1:50051");
        let _ = primary1.serve(addr1).await;
    });

    tokio::spawn(async move {
        println!("   Shard 2 listening on 127.0.0.1:50052");
        let _ = primary2.serve(addr2).await;
    });

    tokio::spawn(async move {
        println!("   Shard 3 listening on 127.0.0.1:50053");
        let _ = primary3.serve(addr3).await;
    });

    // Wait for servers to start
    sleep(Duration::from_millis(500)).await;

    // Create coordinator with 150 virtual nodes per shard
    println!("\n3. Creating coordinator with consistent hashing...");
    let coordinator = Arc::new(Coordinator::new(150));

    // Register shards
    println!("4. Registering shards with coordinator...");
    coordinator
        .register_shard("shard1".to_string(), "http://127.0.0.1:50051".to_string())
        .await?;
    coordinator
        .register_shard("shard2".to_string(), "http://127.0.0.1:50052".to_string())
        .await?;
    coordinator
        .register_shard("shard3".to_string(), "http://127.0.0.1:50053".to_string())
        .await?;

    println!("   Registered {} shards", coordinator.shard_count());

    // Start health checker
    println!("\n5. Starting health checker...");
    let health_config = HealthCheckConfig {
        check_interval: Duration::from_secs(5),
        timeout: Duration::from_secs(2),
        failure_threshold: 3,
        success_threshold: 2,
    };
    let health_checker = HealthChecker::new(coordinator.clone(), health_config);
    health_checker.start();
    println!("   Health checker running with 5s interval");

    // Write data through coordinator
    println!("\n6. Writing data through coordinator...");
    for i in 0..30 {
        let key = format!("key_{}", i);
        let value = format!("value_{}", i);

        let shard = coordinator.route_key(key.as_bytes()).unwrap();
        coordinator.put(key.as_bytes(), value.as_bytes()).await?;

        if i < 5 {
            println!("   {} -> {} (routed to {})", key, value, shard);
        }
    }
    println!("   ... wrote 30 key-value pairs");

    // Read data through coordinator
    println!("\n7. Reading data through coordinator...");
    for i in 0..5 {
        let key = format!("key_{}", i);
        let response = coordinator.get(key.as_bytes()).await?;

        if response.found {
            let value = String::from_utf8_lossy(&response.value);
            println!("   {} = {}", key, value);
        }
    }

    // Show shard statistics
    println!("\n8. Shard statistics:");
    let shards = coordinator.list_shards();
    for shard_info in shards {
        println!(
            "   {} ({}): {} requests, healthy: {}",
            shard_info.id, shard_info.address, shard_info.request_count, shard_info.healthy
        );
    }

    // Test routing consistency
    println!("\n9. Testing routing consistency...");
    let test_key = b"consistency_test_key";
    let shard1 = coordinator.route_key(test_key);
    let shard2 = coordinator.route_key(test_key);
    let shard3 = coordinator.route_key(test_key);

    assert_eq!(shard1, shard2);
    assert_eq!(shard2, shard3);
    println!("   ✓ Same key always routes to same shard: {:?}", shard1);

    // Test delete operation
    println!("\n10. Testing delete operation...");
    coordinator.put(b"delete_test", b"to_be_deleted").await?;

    let response = coordinator.get(b"delete_test").await?;
    assert!(response.found);
    println!("   Before delete: found = {}", response.found);

    coordinator.delete(b"delete_test").await?;

    let response = coordinator.get(b"delete_test").await?;
    assert!(!response.found);
    println!("   After delete: found = {}", response.found);

    // Show load distribution
    println!("\n11. Analyzing load distribution...");
    let mut distribution = std::collections::HashMap::new();
    for i in 0..1000 {
        let key = format!("dist_key_{}", i);
        let shard = coordinator.route_key(key.as_bytes()).unwrap();
        *distribution.entry(shard).or_insert(0) += 1;
    }

    for (shard_id, count) in distribution {
        let percentage = (count as f64 / 1000.0) * 100.0;
        println!("   {}: {} keys ({:.1}%)", shard_id, count, percentage);
    }

    println!("\n=== Demo Complete ===");
    println!("\nThe coordinator provides:");
    println!("  ✓ Consistent hashing for key routing");
    println!("  ✓ Load balancing across shards");
    println!("  ✓ Health checking and failure detection");
    println!("  ✓ Transparent request forwarding");

    // Keep running for a bit to show health checks
    println!("\nHealth checker will continue running for 10 seconds...");
    sleep(Duration::from_secs(10)).await;

    health_checker.stop();
    println!("Health checker stopped.\n");

    Ok(())
}
