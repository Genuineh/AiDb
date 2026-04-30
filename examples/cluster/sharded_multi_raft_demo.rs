//! Sharded Multi-Raft Demo
//!
//! This example demonstrates Stage 3 of the Multi-Raft + Sharding plan:
//! automatic key routing to Raft groups with sharded AiDb instances.
//!
//! Run with: `cargo run --example sharded_multi_raft_demo --features raft-cluster`

use aidb::cluster::{ClusterMeta, GroupMeta, MultiRaftNode, Router};
use aidb::config::Options;
use openraft::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::init();

    println!("=== Sharded Multi-Raft Demo ===\n");

    // Step 1: Create a Multi-Raft node
    println!("1. Creating Multi-Raft node...");
    let temp_dir = tempfile::TempDir::new()?;
    let config = Config::default();
    let mut node = MultiRaftNode::new(1, temp_dir.path(), config, None).await?;
    println!("   ✓ Node created with ID: {}\n", node.node_id());

    // Step 2: Initialize MetaRaft
    println!("2. Initializing MetaRaft...");
    let meta_config = Config::default();
    node.init_meta_raft(meta_config).await?;

    // Bootstrap MetaRaft cluster (single node for demo)
    node.initialize_meta_cluster(vec![(1, "127.0.0.1:50051".to_string())]).await?;
    println!("   ✓ MetaRaft initialized\n");

    // Step 3: Initialize Router
    println!("3. Initializing Router...");
    node.init_router()?;
    println!("   ✓ Router initialized\n");

    // Step 4: Initialize Sharded State Machine
    println!("4. Initializing Sharded State Machine...");
    let options = Options::default();
    node.init_state_machine(options)?;
    println!("   ✓ State Machine initialized\n");

    // Step 5: Create multiple Raft groups
    println!("5. Creating Raft groups...");
    let group_count = 4;
    for group_id in 0..group_count {
        println!("   Creating group {}...", group_id);
        node.create_raft_group(group_id, vec![1]).await?;
    }
    println!("   ✓ Created {} Raft groups\n", group_count);

    // Step 6: Update MetaRaft with slot mappings
    println!("6. Configuring slot mappings...");
    if let Some(_meta_raft) = node.meta_raft() {
        // Create uniform slot distribution
        let mut meta = ClusterMeta::with_uniform_distribution(group_count);

        // Add group metadata
        for group_id in 0..group_count {
            meta.groups.insert(group_id, GroupMeta::new(group_id, vec![1]));
        }

        // Update MetaRaft (simplified - in real system would use proper requests)
        // For demo, we'll manually update and refresh router
        let router = node.router().unwrap();
        router.update_metadata(meta);

        println!("   ✓ Slot mappings configured (16384 slots → {} groups)\n", group_count);
    }

    // Step 7: Demonstrate automatic key routing
    println!("7. Demonstrating automatic key routing...");
    println!();

    let test_data = vec![
        ("user:1000", "Alice"),
        ("user:2000", "Bob"),
        ("order:100", "Order data 1"),
        ("order:200", "Order data 2"),
        ("product:50", "Product info"),
        ("session:abc", "Session data"),
    ];

    // Write keys - they will be automatically routed to correct groups
    println!("   Writing keys (with automatic routing):");
    for (key, value) in &test_data {
        // Calculate which slot and group this key will go to
        let slot = Router::key_to_slot(key.as_bytes());
        let router = node.router().unwrap();
        let group_id = router.slot_to_group(slot)?;

        println!("     {} → slot {} → group {}", key, slot, group_id);

        // Note: In a real system, we would use node.put() here
        // For demo purposes, we'll use the state machine directly
        if let Some(state_machine) = node.state_machine() {
            state_machine.put_routed(key.as_bytes().to_vec(), value.as_bytes().to_vec())?;
        }
    }
    println!();

    // Read keys back
    println!("   Reading keys back:");
    for (key, expected_value) in &test_data {
        if let Some(state_machine) = node.state_machine() {
            let value = state_machine.get_routed(key.as_bytes())?;

            if let Some(v) = value {
                let value_str = String::from_utf8_lossy(&v);
                assert_eq!(value_str, *expected_value);
                println!("     {} = {} ✓", key, value_str);
            } else {
                println!("     {} = <not found> ✗", key);
            }
        }
    }
    println!();

    // Step 8: Show distribution statistics
    println!("8. Distribution statistics:");
    if let Some(state_machine) = node.state_machine() {
        let active_groups = state_machine.list_groups();
        println!("   Active groups: {} out of {}", active_groups.len(), group_count);

        for &group_id in &active_groups {
            println!("     Group {}: active", group_id);
        }
    }
    println!();

    // Step 9: Demonstrate key distribution
    println!("9. Testing key distribution with 1000 keys:");
    let mut group_counts = std::collections::HashMap::new();

    for i in 0..1000 {
        let key = format!("test_key:{}", i);
        let slot = Router::key_to_slot(key.as_bytes());
        let router = node.router().unwrap();
        let group_id = router.slot_to_group(slot)?;

        *group_counts.entry(group_id).or_insert(0) += 1;
    }

    println!("   Keys per group:");
    for group_id in 0..group_count {
        let count = group_counts.get(&group_id).unwrap_or(&0);
        println!(
            "     Group {}: {} keys ({:.1}%)",
            group_id,
            count,
            (*count as f64 / 1000.0) * 100.0
        );
    }
    println!();

    // Step 10: Cleanup
    println!("10. Shutting down...");
    node.shutdown().await?;
    println!("   ✓ Node shutdown complete\n");

    println!("=== Demo Complete ===");
    println!();
    println!("Summary:");
    println!("  • Created {} Raft groups", group_count);
    println!("  • Wrote and read {} key-value pairs", test_data.len());
    println!("  • All keys automatically routed to correct groups");
    println!("  • Demonstrated even key distribution");
    println!();
    println!("Next steps:");
    println!("  • Stage 4: Dynamic member management");
    println!("  • Stage 5: Online slot migration");
    println!("  • Stage 6: Production optimizations");

    Ok(())
}
