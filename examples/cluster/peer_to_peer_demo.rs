//! Peer-to-peer cluster demonstration
//!
//! This example demonstrates the peer-to-peer cluster architecture where
//! nodes are equal peers without a centralized coordinator.

use aidb::cluster::PeerNode;
use aidb::{Options, DB};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    println!("🚀 Starting Peer-to-Peer Cluster Demo");
    println!("=====================================\n");

    // Create three peer nodes
    println!("📦 Creating peer nodes...");

    // Peer 1
    let db1 = DB::open("./data/peer1", Options::default())?;
    let peer1 = Arc::new(PeerNode::new(
        "peer1".to_string(),
        "127.0.0.1:50051".to_string(),
        Arc::new(db1),
        Some(1000), // Enable cache with 1000 entries
        150,        // 150 virtual nodes for consistent hashing
    ));

    // Peer 2
    let db2 = DB::open("./data/peer2", Options::default())?;
    let peer2 = Arc::new(PeerNode::new(
        "peer2".to_string(),
        "127.0.0.1:50052".to_string(),
        Arc::new(db2),
        Some(1000),
        150,
    ));

    // Peer 3
    let db3 = DB::open("./data/peer3", Options::default())?;
    let peer3 = Arc::new(PeerNode::new(
        "peer3".to_string(),
        "127.0.0.1:50053".to_string(),
        Arc::new(db3),
        Some(1000),
        150,
    ));

    println!("✅ Created 3 peer nodes\n");

    // Note: In a real deployment, you would start RPC servers like this:
    // let server1 = tokio::spawn(async move {
    //     let addr = "127.0.0.1:50051".parse().unwrap();
    //     peer1.serve(addr).await
    // });
    // But for this demo, we'll skip the servers and just demonstrate the P2P logic

    // Join peers to form a cluster
    println!("🤝 Forming peer-to-peer cluster...");

    // Peer1 joins Peer2 and Peer3
    peer1
        .join_peer("peer2".to_string(), "http://127.0.0.1:50052".to_string())
        .await?;
    peer1
        .join_peer("peer3".to_string(), "http://127.0.0.1:50053".to_string())
        .await?;

    // Peer2 joins Peer1 and Peer3
    peer2
        .join_peer("peer1".to_string(), "http://127.0.0.1:50051".to_string())
        .await?;
    peer2
        .join_peer("peer3".to_string(), "http://127.0.0.1:50053".to_string())
        .await?;

    // Peer3 joins Peer1 and Peer2
    peer3
        .join_peer("peer1".to_string(), "http://127.0.0.1:50051".to_string())
        .await?;
    peer3
        .join_peer("peer2".to_string(), "http://127.0.0.1:50052".to_string())
        .await?;

    println!("✅ Cluster formed with 3 peers\n");

    // Display cluster topology
    println!("📊 Cluster Topology:");
    println!("-------------------");
    for peer_info in peer1.list_peers() {
        println!(
            "  • {} @ {} [{}]",
            peer_info.id,
            peer_info.address,
            if peer_info.healthy {
                "healthy"
            } else {
                "unhealthy"
            }
        );
    }
    println!();

    // Demonstrate data routing
    println!("🔀 Demonstrating Consistent Hashing Routing:");
    println!("--------------------------------------------");

    let test_keys = vec![
        b"user:1001".to_vec(),
        b"user:1002".to_vec(),
        b"user:1003".to_vec(),
        b"product:2001".to_vec(),
        b"product:2002".to_vec(),
        b"order:3001".to_vec(),
    ];

    for key in &test_keys {
        let routed_peer = peer1.route_key(key);
        println!(
            "  Key {:?} → {}",
            String::from_utf8_lossy(key),
            routed_peer.unwrap_or_else(|| "none".to_string())
        );
    }
    println!();

    // Perform operations through different peers
    println!("✍️  Writing data through different peers...");

    // Each peer can handle writes for keys it's responsible for
    peer1.handle_local_put(b"key1", b"value1")?;
    peer2.handle_local_put(b"key2", b"value2")?;
    peer3.handle_local_put(b"key3", b"value3")?;

    println!("✅ Data written\n");

    // Display statistics
    println!("📈 Peer Statistics:");
    println!("------------------");

    let stats1 = peer1.stats();
    println!(
        "  Peer1: local={} forwarded={} hit_rate={:.2}%",
        stats1.local_requests,
        stats1.forwarded_requests,
        stats1.hit_rate() * 100.0
    );

    let stats2 = peer2.stats();
    println!(
        "  Peer2: local={} forwarded={} hit_rate={:.2}%",
        stats2.local_requests,
        stats2.forwarded_requests,
        stats2.hit_rate() * 100.0
    );

    let stats3 = peer3.stats();
    println!(
        "  Peer3: local={} forwarded={} hit_rate={:.2}%",
        stats3.local_requests,
        stats3.forwarded_requests,
        stats3.hit_rate() * 100.0
    );
    println!();

    // Demonstrate peer failure and health monitoring
    println!("⚠️  Simulating peer2 failure...");
    peer1.mark_unhealthy("peer2");
    peer3.mark_unhealthy("peer2");
    println!("✅ Peer2 marked as unhealthy\n");

    println!("📊 Updated Cluster Status:");
    println!("-------------------------");
    for peer_info in peer1.list_peers() {
        println!(
            "  • {} @ {} [{}]",
            peer_info.id,
            peer_info.address,
            if peer_info.healthy {
                "healthy"
            } else {
                "unhealthy"
            }
        );
    }
    println!();

    // Demonstrate peer recovery
    println!("🔄 Simulating peer2 recovery...");
    peer1.mark_healthy("peer2");
    peer3.mark_healthy("peer2");
    println!("✅ Peer2 marked as healthy\n");

    println!("🎉 Peer-to-Peer Cluster Demo Complete!");
    println!();
    println!("Key Features Demonstrated:");
    println!("  ✓ No centralized coordinator");
    println!("  ✓ Equal peer nodes");
    println!("  ✓ Consistent hashing for data distribution");
    println!("  ✓ Peer discovery and membership");
    println!("  ✓ Health monitoring");
    println!("  ✓ Decentralized routing");
    println!();

    // Note: Since we didn't start actual servers, no cleanup needed
    // In a real deployment, you would cleanup like: server1.abort(); server2.abort(); server3.abort();

    Ok(())
}
