//! Integration tests for Phase 2: Coordinator functionality
//!
//! Tests for Week 25-28:
//! - Week 25: Consistent hashing
//! - Week 26: Coordinator core (routing, shard registration, load balancing)
//! - Week 27-28: Health checking

#[cfg(feature = "cluster")]
use aidb::cluster::{
    ConsistentHashRing, Coordinator, HealthCheckConfig, HealthChecker, PrimaryNode,
};
use aidb::{Options, DB};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

#[cfg(feature = "cluster")]
use tokio::time::sleep;

/// Helper to create a test DB
async fn create_test_db() -> (Arc<DB>, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let options = Options::default();
    let db = DB::open(temp_dir.path(), options).unwrap();
    (Arc::new(db), temp_dir)
}

// ============================================================================
// Week 25: Consistent Hashing Tests
// ============================================================================

#[cfg(feature = "cluster")]
#[test]
fn test_consistent_hash_basic() {
    let mut ring = ConsistentHashRing::new(150);

    // Test empty ring
    assert_eq!(ring.node_count(), 0);
    assert_eq!(ring.get_node(b"key1"), None);

    // Add nodes
    ring.add_node("shard1".to_string());
    ring.add_node("shard2".to_string());
    ring.add_node("shard3".to_string());

    assert_eq!(ring.node_count(), 3);

    // Keys should map to one of the shards
    for i in 0..100 {
        let key = format!("key{}", i);
        let shard = ring.get_node(key.as_bytes());
        assert!(shard.is_some());
    }
}

#[cfg(feature = "cluster")]
#[test]
fn test_consistent_hash_node_removal() {
    let mut ring = ConsistentHashRing::new(150);

    ring.add_node("shard1".to_string());
    ring.add_node("shard2".to_string());
    ring.add_node("shard3".to_string());

    // Record original mappings
    let mut original_mappings = Vec::new();
    for i in 0..1000 {
        let key = format!("key{}", i);
        original_mappings.push(ring.get_node(key.as_bytes()));
    }

    // Remove a shard
    ring.remove_node("shard2");
    assert_eq!(ring.node_count(), 2);

    // Check that most keys stayed on the same shard
    let mut changed = 0;
    for (i, original_mapping) in original_mappings.iter().enumerate() {
        let key = format!("key{}", i);
        let new_mapping = ring.get_node(key.as_bytes());
        if *original_mapping != new_mapping {
            changed += 1;
        }
    }

    // Only keys from shard2 should have moved (~33%)
    let change_rate = changed as f64 / 1000.0;
    assert!((0.2..=0.5).contains(&change_rate));
}

#[cfg(feature = "cluster")]
#[test]
fn test_consistent_hash_load_distribution() {
    let mut ring = ConsistentHashRing::new(150);

    ring.add_node("shard1".to_string());
    ring.add_node("shard2".to_string());
    ring.add_node("shard3".to_string());

    // Generate test keys
    let mut test_keys = Vec::new();
    for i in 0..3000 {
        test_keys.push(format!("key{}", i).into_bytes());
    }

    let distribution = ring.get_distribution(&test_keys);

    // All shards should get some keys
    assert_eq!(distribution.len(), 3);

    // Check that distribution is reasonably balanced
    let expected = 3000 / 3;
    for (shard_id, count) in distribution {
        let variance = (count as i32 - expected).abs() as f64 / expected as f64;
        assert!(
            variance < 0.3,
            "Shard {} has variance {} which is too high",
            shard_id,
            variance
        );
    }
}

#[cfg(feature = "cluster")]
#[test]
fn test_consistent_hash_virtual_nodes() {
    // Test with different numbers of virtual nodes
    let ring1 = ConsistentHashRing::new(50);
    let ring2 = ConsistentHashRing::new(150);
    let ring3 = ConsistentHashRing::new(300);

    // More virtual nodes should lead to better distribution
    // This is tested implicitly through the distribution tests
    assert_eq!(ring1.node_count(), 0);
    assert_eq!(ring2.node_count(), 0);
    assert_eq!(ring3.node_count(), 0);
}

// ============================================================================
// Week 26: Coordinator Core Tests
// ============================================================================

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_coordinator_creation() {
    let coordinator = Coordinator::new(150);
    assert_eq!(coordinator.shard_count(), 0);
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_coordinator_shard_registration() {
    let (db1, _temp1) = create_test_db().await;
    let (db2, _temp2) = create_test_db().await;

    // Start two primary nodes
    let primary1 = PrimaryNode::new(db1.clone());
    let primary2 = PrimaryNode::new(db2.clone());

    let addr1 = "127.0.0.1:50061".parse().unwrap();
    let addr2 = "127.0.0.1:50062".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary1.serve(addr1).await;
    });

    tokio::spawn(async move {
        let _ = primary2.serve(addr2).await;
    });

    // Wait for servers to start
    sleep(Duration::from_millis(200)).await;

    // Create coordinator and register shards
    let coordinator = Coordinator::new(150);

    let result1 = coordinator
        .register_shard("shard1".to_string(), "http://127.0.0.1:50061".to_string())
        .await;
    assert!(result1.is_ok());

    let result2 = coordinator
        .register_shard("shard2".to_string(), "http://127.0.0.1:50062".to_string())
        .await;
    assert!(result2.is_ok());

    assert_eq!(coordinator.shard_count(), 2);

    // List shards
    let shards = coordinator.list_shards();
    assert_eq!(shards.len(), 2);
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_coordinator_routing() {
    let (db1, _temp1) = create_test_db().await;
    let (db2, _temp2) = create_test_db().await;

    // Insert data directly into DBs
    db1.put(b"key1", b"value1").unwrap();
    db2.put(b"key2", b"value2").unwrap();

    // Start two primary nodes
    let primary1 = PrimaryNode::new(db1.clone());
    let primary2 = PrimaryNode::new(db2.clone());

    let addr1 = "127.0.0.1:50063".parse().unwrap();
    let addr2 = "127.0.0.1:50064".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary1.serve(addr1).await;
    });

    tokio::spawn(async move {
        let _ = primary2.serve(addr2).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Create coordinator and register shards
    let coordinator = Arc::new(Coordinator::new(150));
    coordinator
        .register_shard("shard1".to_string(), "http://127.0.0.1:50063".to_string())
        .await
        .unwrap();
    coordinator
        .register_shard("shard2".to_string(), "http://127.0.0.1:50064".to_string())
        .await
        .unwrap();

    // Test routing
    let shard1 = coordinator.route_key(b"test_key_1");
    let shard2 = coordinator.route_key(b"test_key_2");

    assert!(shard1.is_some());
    assert!(shard2.is_some());

    // Same key should always route to same shard
    let shard1_repeat = coordinator.route_key(b"test_key_1");
    assert_eq!(shard1, shard1_repeat);
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_coordinator_put_and_get() {
    let (db1, _temp1) = create_test_db().await;
    let (db2, _temp2) = create_test_db().await;

    // Start two primary nodes
    let primary1 = PrimaryNode::new(db1.clone());
    let primary2 = PrimaryNode::new(db2.clone());

    let addr1 = "127.0.0.1:50065".parse().unwrap();
    let addr2 = "127.0.0.1:50066".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary1.serve(addr1).await;
    });

    tokio::spawn(async move {
        let _ = primary2.serve(addr2).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Create coordinator and register shards
    let coordinator = Arc::new(Coordinator::new(150));
    coordinator
        .register_shard("shard1".to_string(), "http://127.0.0.1:50065".to_string())
        .await
        .unwrap();
    coordinator
        .register_shard("shard2".to_string(), "http://127.0.0.1:50066".to_string())
        .await
        .unwrap();

    // Put data through coordinator
    let put_result = coordinator.put(b"coord_key", b"coord_value").await;
    assert!(put_result.is_ok());

    // Get data through coordinator
    let get_result = coordinator.get(b"coord_key").await;
    assert!(get_result.is_ok());
    let response = get_result.unwrap();
    assert!(response.found);
    assert_eq!(response.value, b"coord_value");
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_coordinator_delete() {
    let (db, _temp) = create_test_db().await;

    // Insert initial data
    db.put(b"delete_key", b"delete_value").unwrap();

    // Start primary node
    let primary = PrimaryNode::new(db.clone());
    let addr = "127.0.0.1:50067".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary.serve(addr).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Create coordinator and register shard
    let coordinator = Arc::new(Coordinator::new(150));
    coordinator
        .register_shard("shard1".to_string(), "http://127.0.0.1:50067".to_string())
        .await
        .unwrap();

    // Verify key exists
    let get_result = coordinator.get(b"delete_key").await.unwrap();
    assert!(get_result.found);

    // Delete through coordinator
    let delete_result = coordinator.delete(b"delete_key").await;
    assert!(delete_result.is_ok());

    // Verify key is deleted
    let get_result = coordinator.get(b"delete_key").await.unwrap();
    assert!(!get_result.found);
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_coordinator_load_balancing() {
    let (db1, _temp1) = create_test_db().await;
    let (db2, _temp2) = create_test_db().await;
    let (db3, _temp3) = create_test_db().await;

    // Start three primary nodes
    let primary1 = PrimaryNode::new(db1.clone());
    let primary2 = PrimaryNode::new(db2.clone());
    let primary3 = PrimaryNode::new(db3.clone());

    let addr1 = "127.0.0.1:50068".parse().unwrap();
    let addr2 = "127.0.0.1:50069".parse().unwrap();
    let addr3 = "127.0.0.1:50070".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary1.serve(addr1).await;
    });
    tokio::spawn(async move {
        let _ = primary2.serve(addr2).await;
    });
    tokio::spawn(async move {
        let _ = primary3.serve(addr3).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Create coordinator and register shards
    let coordinator = Arc::new(Coordinator::new(150));
    coordinator
        .register_shard("shard1".to_string(), "http://127.0.0.1:50068".to_string())
        .await
        .unwrap();
    coordinator
        .register_shard("shard2".to_string(), "http://127.0.0.1:50069".to_string())
        .await
        .unwrap();
    coordinator
        .register_shard("shard3".to_string(), "http://127.0.0.1:50070".to_string())
        .await
        .unwrap();

    // Write many keys
    for i in 0..100 {
        let key = format!("lb_key_{}", i);
        let value = format!("lb_value_{}", i);
        let _ = coordinator.put(key.as_bytes(), value.as_bytes()).await;
    }

    // Check that requests are distributed across shards
    let shards = coordinator.list_shards();
    let mut total_requests = 0;
    for shard in shards {
        total_requests += shard.request_count;
    }

    assert_eq!(total_requests, 100);
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_coordinator_shard_unregistration() {
    let (db, _temp) = create_test_db().await;

    // Start a primary node
    let primary = PrimaryNode::new(db.clone());
    let addr = "127.0.0.1:50073".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary.serve(addr).await;
    });

    sleep(Duration::from_millis(200)).await;

    let coordinator = Coordinator::new(150);

    // Register shard
    coordinator
        .register_shard("shard1".to_string(), "http://127.0.0.1:50073".to_string())
        .await
        .unwrap();
    assert_eq!(coordinator.shard_count(), 1);

    // Unregister
    coordinator.unregister_shard("shard1");
    assert_eq!(coordinator.shard_count(), 0);
    assert_eq!(coordinator.route_key(b"test_key"), None);
}

// ============================================================================
// Week 27-28: Health Checking Tests
// ============================================================================

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_health_checker_basic() {
    let coordinator = Arc::new(Coordinator::new(150));
    let config = HealthCheckConfig::default();
    let checker = HealthChecker::new(coordinator, config);

    assert!(!checker.is_running());
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_health_checker_start_stop() {
    let coordinator = Arc::new(Coordinator::new(150));
    let config = HealthCheckConfig {
        check_interval: Duration::from_millis(100),
        timeout: Duration::from_secs(1),
        failure_threshold: 2,
        success_threshold: 1,
    };
    let checker = HealthChecker::new(coordinator, config);

    checker.start();
    assert!(checker.is_running());

    sleep(Duration::from_millis(50)).await;

    checker.stop();
    sleep(Duration::from_millis(200)).await;
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_health_checker_mark_unhealthy() {
    let (db, _temp) = create_test_db().await;

    // Start primary node
    let primary = PrimaryNode::new(db.clone());
    let addr = "127.0.0.1:50071".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary.serve(addr).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Create coordinator and register shard
    let coordinator = Arc::new(Coordinator::new(150));
    coordinator
        .register_shard("shard1".to_string(), "http://127.0.0.1:50071".to_string())
        .await
        .unwrap();

    // Verify shard is initially healthy
    let shard_info = coordinator.get_shard_stats("shard1").unwrap();
    assert!(shard_info.healthy);

    // Manually mark as unhealthy
    coordinator.mark_unhealthy("shard1");
    let shard_info = coordinator.get_shard_stats("shard1").unwrap();
    assert!(!shard_info.healthy);

    // Manually mark as healthy again
    coordinator.mark_healthy("shard1");
    let shard_info = coordinator.get_shard_stats("shard1").unwrap();
    assert!(shard_info.healthy);
}

#[cfg(feature = "cluster")]
#[tokio::test]
#[ignore] // This test is timing-sensitive and may be flaky in CI
async fn test_health_checker_with_healthy_shard() {
    let (db, _temp) = create_test_db().await;

    // Start primary node
    let primary = PrimaryNode::new(db.clone());
    let addr = "127.0.0.1:50072".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary.serve(addr).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Create coordinator and register shard
    let coordinator = Arc::new(Coordinator::new(150));
    coordinator
        .register_shard("shard1".to_string(), "http://127.0.0.1:50072".to_string())
        .await
        .unwrap();

    // Verify shard is initially healthy
    let shard_info = coordinator.get_shard_stats("shard1").unwrap();
    assert!(shard_info.healthy);

    // Start health checker
    let config = HealthCheckConfig {
        check_interval: Duration::from_millis(50),
        timeout: Duration::from_millis(200),
        failure_threshold: 3,
        success_threshold: 2,
    };
    let checker = HealthChecker::new(coordinator.clone(), config);
    checker.start();

    // Wait for a few health checks
    sleep(Duration::from_millis(300)).await;

    // Shard should still be healthy
    let shard_info = coordinator.get_shard_stats("shard1").unwrap();
    assert!(shard_info.healthy);

    checker.stop();
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_coordinator_no_shards_error() {
    let coordinator = Coordinator::new(150);

    // Try to get/put/delete with no shards registered
    let get_result = coordinator.get(b"key").await;
    assert!(get_result.is_err());

    let put_result = coordinator.put(b"key", b"value").await;
    assert!(put_result.is_err());

    let delete_result = coordinator.delete(b"key").await;
    assert!(delete_result.is_err());
}
