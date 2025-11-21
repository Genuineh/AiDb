//! Dynamic Member Management Demo
//!
//! This example demonstrates Stage 4 of the Multi-Raft + Sharding plan:
//! - Automatic node joining
//! - Replica allocation and rebalancing
//! - Dynamic membership changes
//!
//! Run with: `cargo run --example dynamic_member_demo --features raft-cluster`

use aidb::cluster::{MetaStateMachine, MultiRaftNode, ReplicaAllocator};
use aidb::config::Options;
use openraft::Config;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    println!("=== Dynamic Member Management Demo ===\n");

    // Step 1: Create MetaStateMachine with replica allocator
    println!("1. Creating MetaStateMachine with replication factor 3...");
    let temp_dir = tempfile::TempDir::new()?;
    let meta_state = MetaStateMachine::with_replication_factor(temp_dir.path(), 3)?;
    println!("   ✓ MetaStateMachine created\n");

    // Step 2: Add initial nodes
    println!("2. Adding initial nodes to the cluster...");
    for node_id in 1..=5 {
        let addr = format!("127.0.0.1:{}", 50050 + node_id);
        let (response, changes) = meta_state.handle_add_node(node_id, addr.clone())?;
        
        match response {
            aidb::cluster::MetaResponse::Ok => {
                println!("   ✓ Node {} added at {}", node_id, addr);
                if !changes.is_empty() {
                    println!("      {} membership changes triggered", changes.len());
                }
            }
            aidb::cluster::MetaResponse::Error(e) => {
                println!("   ✗ Failed to add node {}: {}", node_id, e);
            }
            _ => {}
        }
    }
    println!();

    // Step 3: Show cluster state
    println!("3. Current cluster state:");
    let meta = meta_state.get_cluster_meta();
    println!("   Nodes: {}", meta.nodes.len());
    for (node_id, node_info) in &meta.nodes {
        println!("     - Node {}: {} (groups: {})", 
                 node_id, node_info.addr, node_info.group_count);
    }
    println!();

    // Step 4: Demonstrate replica allocation
    println!("4. Demonstrating replica allocation...");
    let allocator = ReplicaAllocator::new(3);
    let available_nodes: Vec<u64> = meta.nodes.keys().copied().collect();
    let mut current_allocation: HashMap<u64, Vec<u64>> = HashMap::new();

    // Allocate replicas for several groups
    for group_id in 100..105 {
        let replicas = allocator.allocate_replicas(
            group_id,
            &available_nodes,
            &current_allocation,
        )?;
        
        println!("   Group {}: replicas = {:?}", group_id, replicas);
        current_allocation.insert(group_id, replicas);
    }
    println!();

    // Step 5: Show load distribution
    println!("5. Load distribution across nodes:");
    let mut node_loads: HashMap<u64, usize> = HashMap::new();
    for replicas in current_allocation.values() {
        for &replica in replicas {
            *node_loads.entry(replica).or_insert(0) += 1;
        }
    }
    
    for node_id in 1..=5 {
        let load = node_loads.get(&node_id).unwrap_or(&0);
        println!("   Node {}: {} groups", node_id, load);
    }
    println!();

    // Step 6: Simulate node removal and rebalancing
    println!("6. Simulating node 3 removal...");
    let remaining_nodes: Vec<u64> = vec![1, 2, 4, 5];
    let new_allocation = allocator.rebalance(&remaining_nodes, current_allocation.clone())?;
    
    println!("   Rebalanced allocation:");
    for (group_id, replicas) in &new_allocation {
        let old_replicas = &current_allocation[group_id];
        if old_replicas != replicas {
            println!("     Group {}: {:?} → {:?}", group_id, old_replicas, replicas);
        }
    }
    println!();

    // Step 7: Calculate new load distribution
    println!("7. New load distribution after node 3 removal:");
    let mut new_node_loads: HashMap<u64, usize> = HashMap::new();
    for replicas in new_allocation.values() {
        for &replica in replicas {
            *new_node_loads.entry(replica).or_insert(0) += 1;
        }
    }
    
    for node_id in &remaining_nodes {
        let load = new_node_loads.get(node_id).unwrap_or(&0);
        println!("   Node {}: {} groups", node_id, load);
    }
    println!();

    // Step 8: Demonstrate MultiRaftNode start
    println!("8. Creating and starting MultiRaftNode...");
    let node_dir = tempfile::TempDir::new()?;
    let config = Config::default();
    let mut node = MultiRaftNode::new(1, node_dir.path(), config).await?;
    
    // Initialize MetaRaft
    let meta_config = Config::default();
    node.init_meta_raft(meta_config).await?;
    println!("   ✓ MetaRaft initialized");

    // Bootstrap cluster
    node.initialize_meta_cluster(vec![(1, "127.0.0.1:50051".to_string())]).await?;
    println!("   ✓ MetaRaft cluster bootstrapped");

    // Initialize router and state machine
    node.init_router()?;
    let options = Options::default();
    node.init_state_machine(options)?;
    println!("   ✓ Router and state machine initialized");

    // Start as bootstrap node
    node.start(true, None).await?;
    println!("   ✓ Node started successfully\n");

    // Step 9: Summary
    println!("9. Summary:");
    println!("   ✓ Replica allocator ensures balanced load distribution");
    println!("   ✓ Node addition/removal triggers automatic rebalancing");
    println!("   ✓ MultiRaftNode supports automatic startup and joining");
    println!("   ✓ Replication factor is configurable (default: 3)");
    println!("\n=== Demo Complete ===");

    Ok(())
}
