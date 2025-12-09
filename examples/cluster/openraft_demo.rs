//! OpenRaft cluster demonstration
//!
//! This example demonstrates how to create a multi-node Raft cluster using OpenRaft.
//! It shows cluster initialization, adding nodes, and performing replicated operations.

use aidb::cluster::{OpenRaftNode, RaftNetworkClientFactory, RaftNodeConfig};
use aidb::{Options, DB};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("=== OpenRaft Cluster Demo ===\n");

    // Create temporary directories for three nodes
    let temp_dir1 = tempfile::tempdir()?;
    let temp_dir2 = tempfile::tempdir()?;
    let temp_dir3 = tempfile::tempdir()?;

    println!("Creating three Raft nodes...");

    // Node 1 - Leader
    let db1 = DB::open(temp_dir1.path(), Options::default())?;
    let network_factory1 = RaftNetworkClientFactory::new(1);
    let config1 = RaftNodeConfig { node_id: 1, ..Default::default() };
    let node1 = Arc::new(OpenRaftNode::new(config1, Arc::new(db1), network_factory1).await?);
    println!("✓ Node 1 created (will be leader)");

    // Node 2 - Follower
    let db2 = DB::open(temp_dir2.path(), Options::default())?;
    let network_factory2 = RaftNetworkClientFactory::new(2);
    let config2 = RaftNodeConfig { node_id: 2, ..Default::default() };
    let node2 = Arc::new(OpenRaftNode::new(config2, Arc::new(db2), network_factory2).await?);
    println!("✓ Node 2 created");

    // Node 3 - Follower
    let db3 = DB::open(temp_dir3.path(), Options::default())?;
    let network_factory3 = RaftNetworkClientFactory::new(3);
    let config3 = RaftNodeConfig { node_id: 3, ..Default::default() };
    let node3 = Arc::new(OpenRaftNode::new(config3, Arc::new(db3), network_factory3).await?);
    println!("✓ Node 3 created\n");

    // Start RPC servers for each node
    println!("Starting RPC servers...");
    let addr1 = "127.0.0.1:50001".parse()?;
    let addr2 = "127.0.0.1:50002".parse()?;
    let addr3 = "127.0.0.1:50003".parse()?;

    let node1_clone = node1.clone();
    let server1 = tokio::spawn(async move { node1_clone.start_server(addr1).await });

    let node2_clone = node2.clone();
    let server2 = tokio::spawn(async move { node2_clone.start_server(addr2).await });

    let node3_clone = node3.clone();
    let server3 = tokio::spawn(async move { node3_clone.start_server(addr3).await });

    println!("✓ RPC servers started on ports 50001, 50002, 50003");
    println!("Waiting for servers to be ready...\n");
    sleep(Duration::from_millis(500)).await;

    // Initialize cluster on node 1
    println!("Initializing cluster with 3 nodes...");
    let nodes = vec![
        (1, "http://127.0.0.1:50001".to_string()),
        (2, "http://127.0.0.1:50002".to_string()),
        (3, "http://127.0.0.1:50003".to_string()),
    ];
    node1.initialize(nodes).await?;
    println!("✓ Cluster initialized\n");

    // Wait for leader election
    println!("Waiting for leader election...");
    sleep(Duration::from_millis(500)).await;

    // Check leadership
    let is_leader = node1.is_leader().await;
    println!("Node 1 is leader: {}", is_leader);

    if let Some(leader) = node1.get_leader().await {
        println!("Current leader: Node {}\n", leader);
    }

    // Perform some write operations
    println!("=== Performing Write Operations ===\n");

    println!("Writing key1=value1...");
    match node1.put(b"key1".to_vec(), b"value1".to_vec()).await {
        Ok(_) => println!("✓ Write successful"),
        Err(e) => println!("✗ Write failed: {}", e),
    }

    println!("Writing key2=value2...");
    match node1.put(b"key2".to_vec(), b"value2".to_vec()).await {
        Ok(_) => println!("✓ Write successful"),
        Err(e) => println!("✗ Write failed: {}", e),
    }

    println!("Writing key3=value3...");
    match node1.put(b"key3".to_vec(), b"value3".to_vec()).await {
        Ok(_) => println!("✓ Write successful\n"),
        Err(e) => println!("✗ Write failed: {}\n", e),
    }

    // Delete a key
    println!("Deleting key2...");
    match node1.delete(b"key2".to_vec()).await {
        Ok(_) => println!("✓ Delete successful\n"),
        Err(e) => println!("✗ Delete failed: {}\n", e),
    }

    // Display metrics
    println!("=== Cluster Metrics ===\n");
    let metrics = node1.metrics().await;
    println!("Node 1 Metrics:");
    println!("  Current term: {}", metrics.current_term);
    println!("  Current leader: {:?}", metrics.current_leader);
    println!("  Last log index: {:?}", metrics.last_log_index);
    println!("  Last applied: {:?}", metrics.last_applied);
    println!("  State: {:?}\n", metrics.state);

    // Demonstrate adding a learner
    println!("=== Adding Learner Node ===\n");
    println!("Adding node 4 as learner...");
    match node1.add_learner(4, "http://127.0.0.1:50004".to_string()).await {
        Ok(_) => println!("✓ Learner added"),
        Err(e) => println!("✗ Failed to add learner: {}", e),
    }

    // Demonstrate membership change
    println!("\n=== Changing Membership ===\n");
    println!("Promoting node 4 to voter...");
    match node1.change_membership(vec![1, 2, 3, 4]).await {
        Ok(_) => println!("✓ Membership changed"),
        Err(e) => println!("✗ Failed to change membership: {}", e),
    }

    println!("\n=== Demo Complete ===\n");
    println!("Shutting down nodes...");

    node1.shutdown().await?;
    node2.shutdown().await?;
    node3.shutdown().await?;

    // Abort the server tasks
    server1.abort();
    server2.abort();
    server3.abort();

    println!("✓ All nodes shut down successfully");

    Ok(())
}
