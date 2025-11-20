//! Raft-based cluster demonstration
//!
//! This example demonstrates using Raft consensus for a distributed cluster.
//! It shows how to create Raft nodes, propose changes, and achieve consensus.

use aidb::cluster::{
    encode_delete, encode_put, RaftConfig, RaftNode, RaftStateMachine, RaftStorage, StateMachine,
};
use aidb::{Options, DB};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    println!("🚀 Starting Raft-Based Cluster Demo");
    println!("===================================\n");

    // Create three Raft nodes
    println!("📦 Creating Raft nodes...");

    // Node 1
    let db1 = DB::open("./data/raft_node1", Options::default())?;
    let storage1 = RaftStorage::new(Arc::new(db1))?;
    let mut peers = HashMap::new();
    peers.insert(2, "127.0.0.1:50052".to_string());
    peers.insert(3, "127.0.0.1:50053".to_string());

    let config1 = RaftConfig {
        id: 1,
        election_tick: 10,
        heartbeat_tick: 3,
        max_size_per_msg: 1024 * 1024,
        max_inflight_msgs: 256,
    };

    let node1 = Arc::new(RaftNode::new(config1, storage1, peers.clone())?);

    // Node 2
    let db2 = DB::open("./data/raft_node2", Options::default())?;
    let storage2 = RaftStorage::new(Arc::new(db2))?;
    let mut peers2 = HashMap::new();
    peers2.insert(1, "127.0.0.1:50051".to_string());
    peers2.insert(3, "127.0.0.1:50053".to_string());

    let config2 = RaftConfig { id: 2, ..config1 };

    let node2 = Arc::new(RaftNode::new(config2, storage2, peers2)?);

    // Node 3
    let db3 = DB::open("./data/raft_node3", Options::default())?;
    let storage3 = RaftStorage::new(Arc::new(db3))?;
    let mut peers3 = HashMap::new();
    peers3.insert(1, "127.0.0.1:50051".to_string());
    peers3.insert(2, "127.0.0.1:50052".to_string());

    let config3 = RaftConfig { id: 3, ..config1 };

    let node3 = Arc::new(RaftNode::new(config3, storage3, peers3)?);

    println!("✅ Created 3 Raft nodes\n");

    println!("📊 Raft Cluster Configuration:");
    println!("------------------------------");
    println!(
        "  Node 1: ID={}, Election Tick={}, Heartbeat Tick={}",
        node1.id(),
        config1.election_tick,
        config1.heartbeat_tick
    );
    println!(
        "  Node 2: ID={}, Election Tick={}, Heartbeat Tick={}",
        node2.id(),
        config2.election_tick,
        config2.heartbeat_tick
    );
    println!(
        "  Node 3: ID={}, Election Tick={}, Heartbeat Tick={}",
        node3.id(),
        config3.election_tick,
        config3.heartbeat_tick
    );
    println!();

    // Simulate some ticks for leader election
    println!("⏰ Simulating Raft ticks for leader election...");
    for _ in 0..15 {
        node1.tick()?;
        node2.tick()?;
        node3.tick()?;
        sleep(Duration::from_millis(100)).await;
    }
    println!();

    // Check leader status
    println!("👑 Leader Status:");
    println!("-----------------");
    let leader1 = node1.leader();
    let leader2 = node2.leader();
    let leader3 = node3.leader();

    println!("  Node 1 sees leader: {:?}", leader1);
    println!("  Node 2 sees leader: {:?}", leader2);
    println!("  Node 3 sees leader: {:?}", leader3);

    if node1.is_leader() {
        println!("  ✅ Node 1 is the LEADER");
    }
    if node2.is_leader() {
        println!("  ✅ Node 2 is the LEADER");
    }
    if node3.is_leader() {
        println!("  ✅ Node 3 is the LEADER");
    }
    println!();

    // Get status info
    println!("📈 Raft Status Information:");
    println!("---------------------------");
    let (term1, committed1, is_leader1) = node1.status_info();
    let (term2, committed2, is_leader2) = node2.status_info();
    let (term3, committed3, is_leader3) = node3.status_info();

    println!("  Node 1: term={}, committed={}, leader={}", term1, committed1, is_leader1);
    println!("  Node 2: term={}, committed={}, leader={}", term2, committed2, is_leader2);
    println!("  Node 3: term={}, committed={}, leader={}", term3, committed3, is_leader3);
    println!();

    // Demonstrate command encoding
    println!("🔧 Command Encoding Demo:");
    println!("-------------------------");

    let put_cmd = encode_put(b"user:1001", b"Alice");
    println!("  PUT command size: {} bytes", put_cmd.len());

    let del_cmd = encode_delete(b"user:1001");
    println!("  DELETE command size: {} bytes", del_cmd.len());
    println!();

    // Test state machine
    println!("🔄 State Machine Test:");
    println!("----------------------");

    let test_db = DB::open("./data/raft_test", Options::default())?;
    let mut state_machine = RaftStateMachine::new(Arc::new(test_db));

    // Apply a PUT command
    let put_result = state_machine.apply(&encode_put(b"test_key", b"test_value"))?;
    println!("  PUT result: {:?}", String::from_utf8_lossy(&put_result));

    // Apply a DELETE command
    let del_result = state_machine.apply(&encode_delete(b"test_key"))?;
    println!("  DELETE result: {:?}", String::from_utf8_lossy(&del_result));
    println!();

    println!("🎉 Raft Cluster Demo Complete!");
    println!();
    println!("Key Features Demonstrated:");
    println!("  ✓ Raft node creation and configuration");
    println!("  ✓ Leader election simulation");
    println!("  ✓ Status information retrieval");
    println!("  ✓ Command encoding (PUT/DELETE)");
    println!("  ✓ State machine operation");
    println!();

    println!("Next Steps:");
    println!("  • Implement Raft message transport (RPC)");
    println!("  • Add log replication");
    println!("  • Integrate with PeerNode");
    println!("  • Add cluster membership changes");
    println!("  • Implement snapshot and recovery");
    println!();

    Ok(())
}
