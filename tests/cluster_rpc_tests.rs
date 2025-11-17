//! Integration tests for RPC cluster functionality

#[cfg(feature = "cluster")]
use aidb::cluster::{PrimaryNode, ReplicaNode};
use aidb::{Options, DB};
use std::sync::Arc;
use tempfile::TempDir;

#[cfg(feature = "cluster")]
use tokio::time::{sleep, Duration};

/// Helper to create a test DB
#[allow(dead_code)]
async fn create_test_db() -> (Arc<DB>, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let options = Options::default();
    let db = DB::open(temp_dir.path(), options).unwrap();
    (Arc::new(db), temp_dir)
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_primary_node_basic_operations() {
    let (db, _temp) = create_test_db().await;

    // Insert some data directly into DB
    db.put(b"key1", b"value1").unwrap();
    db.put(b"key2", b"value2").unwrap();

    // Create primary node
    let primary = PrimaryNode::new(db.clone());

    // Verify stats are tracked
    let stats = primary.stats();
    assert_eq!(stats.total_requests, 0);

    // Start server in background
    let addr = "127.0.0.1:50051".parse().unwrap();
    tokio::spawn(async move {
        let _ = primary.serve(addr).await;
    });

    // Give server time to start
    sleep(Duration::from_millis(100)).await;
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_primary_node_rpc_get() {
    use aidb::cluster::rpc::proto::{storage_client::StorageClient, GetRequest};

    let (db, _temp) = create_test_db().await;

    // Insert test data
    db.put(b"test_key", b"test_value").unwrap();

    // Create primary node and start server
    let primary = PrimaryNode::new(db.clone());
    let addr = "127.0.0.1:50052".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary.serve(addr).await;
    });

    // Give server time to start
    sleep(Duration::from_millis(200)).await;

    // Create client and test RPC
    let mut client = StorageClient::connect("http://127.0.0.1:50052").await.unwrap();

    let request = tonic::Request::new(GetRequest { key: b"test_key".to_vec() });

    let response = client.get(request).await.unwrap();
    let resp = response.into_inner();

    assert!(resp.found);
    assert_eq!(resp.value, b"test_value");
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_primary_node_rpc_put() {
    use aidb::cluster::rpc::proto::{storage_client::StorageClient, GetRequest, PutRequest};

    let (db, _temp) = create_test_db().await;

    // Create primary node and start server
    let primary = PrimaryNode::new(db.clone());
    let addr = "127.0.0.1:50053".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary.serve(addr).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Create client
    let mut client = StorageClient::connect("http://127.0.0.1:50053").await.unwrap();

    // Test PUT
    let request =
        tonic::Request::new(PutRequest { key: b"new_key".to_vec(), value: b"new_value".to_vec() });

    let response = client.put(request).await.unwrap();
    assert!(response.into_inner().success);

    // Verify data was written
    let request = tonic::Request::new(GetRequest { key: b"new_key".to_vec() });

    let response = client.get(request).await.unwrap();
    let resp = response.into_inner();

    assert!(resp.found);
    assert_eq!(resp.value, b"new_value");
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_primary_node_health_check() {
    use aidb::cluster::rpc::proto::{storage_client::StorageClient, HealthCheckRequest};

    let (db, _temp) = create_test_db().await;

    let primary = PrimaryNode::new(db.clone());
    let addr = "127.0.0.1:50054".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary.serve(addr).await;
    });

    sleep(Duration::from_millis(200)).await;

    let mut client = StorageClient::connect("http://127.0.0.1:50054").await.unwrap();

    let request = tonic::Request::new(HealthCheckRequest { service: "storage".to_string() });

    let response = client.health_check(request).await.unwrap();
    let resp = response.into_inner();

    assert_eq!(resp.status, 1); // SERVING
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_replica_node_cache_hit() {
    let (db, _temp) = create_test_db().await;

    // Insert test data
    db.put(b"cached_key", b"cached_value").unwrap();

    // Start primary server
    let primary = PrimaryNode::new(db.clone());
    let addr = "127.0.0.1:50055".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary.serve(addr).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Create replica
    let mut replica = ReplicaNode::new("http://127.0.0.1:50055".to_string(), 100).await.unwrap();

    // First get - cache miss, forward to primary
    let value = replica.get(b"cached_key").await.unwrap();
    assert_eq!(value, Some(b"cached_value".to_vec()));

    let stats = replica.stats();
    assert_eq!(stats.cache_misses, 1);
    assert_eq!(stats.cache_hits, 0);

    // Second get - cache hit
    let value = replica.get(b"cached_key").await.unwrap();
    assert_eq!(value, Some(b"cached_value".to_vec()));

    let stats = replica.stats();
    assert_eq!(stats.cache_hits, 1);
    assert_eq!(stats.hit_rate(), 0.5); // 1 hit out of 2 requests
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_replica_node_put_invalidates_cache() {
    let (db, _temp) = create_test_db().await;

    // Start primary server
    let primary = PrimaryNode::new(db.clone());
    let addr = "127.0.0.1:50056".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary.serve(addr).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Create replica
    let mut replica = ReplicaNode::new("http://127.0.0.1:50056".to_string(), 100).await.unwrap();

    // Put a value
    replica.put(b"key", b"value1").await.unwrap();

    // Get - should be cache miss (put invalidated cache)
    let value = replica.get(b"key").await.unwrap();
    assert_eq!(value, Some(b"value1".to_vec()));

    let stats = replica.stats();
    assert_eq!(stats.cache_misses, 1);

    // Get again - cache hit
    let value = replica.get(b"key").await.unwrap();
    assert_eq!(value, Some(b"value1".to_vec()));

    let stats = replica.stats();
    assert_eq!(stats.cache_hits, 1);

    // Update the value
    replica.put(b"key", b"value2").await.unwrap();

    // Get - cache miss again (invalidated)
    let value = replica.get(b"key").await.unwrap();
    assert_eq!(value, Some(b"value2".to_vec()));

    let stats = replica.stats();
    assert_eq!(stats.cache_misses, 2);
}

#[cfg(feature = "cluster")]
#[tokio::test]
async fn test_replica_node_warmup() {
    let (db, _temp) = create_test_db().await;

    // Insert test data
    db.put(b"key1", b"value1").unwrap();
    db.put(b"key2", b"value2").unwrap();
    db.put(b"key3", b"value3").unwrap();

    // Start primary server
    let primary = PrimaryNode::new(db.clone());
    let addr = "127.0.0.1:50057".parse().unwrap();

    tokio::spawn(async move {
        let _ = primary.serve(addr).await;
    });

    sleep(Duration::from_millis(200)).await;

    // Create replica
    let mut replica = ReplicaNode::new("http://127.0.0.1:50057".to_string(), 100).await.unwrap();

    // Warmup cache
    let keys = vec![b"key1".to_vec(), b"key2".to_vec(), b"key3".to_vec()];

    let warmed = replica.warmup(keys).await.unwrap();
    assert_eq!(warmed, 3);
    assert_eq!(replica.cache_size(), 3);

    // All subsequent gets should be cache hits
    replica.get(b"key1").await.unwrap();
    replica.get(b"key2").await.unwrap();
    replica.get(b"key3").await.unwrap();

    let stats = replica.stats();
    // 3 from warmup + 3 cache hits = 6 total, 3 hits
    assert_eq!(stats.cache_hits, 3);
}
