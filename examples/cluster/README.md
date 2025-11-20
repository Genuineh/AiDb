# AiDb Cluster Mode

This directory contains the distributed cluster implementation for AiDb, including RPC networking, consistent hashing, Raft consensus, and cluster coordination.

## Architecture Overview

AiDb supports three cluster architectures:

1. **Raft-Based P2P** (NEW - Recommended for production)
2. **Simple P2P** (Good for caching/eventual consistency)
3. **Traditional Primary-Replica** (Simple centralized setup)

### 1. Raft-Based P2P Architecture ⭐ NEW!

Production-ready distributed consensus cluster:

```
Application: RaftBasedPeer API
        ↓
Consensus: Raft (Leader Election, Log Replication)
        ↓
Storage: RaftStorage + LSM-Tree
```

**Features:**
- ✅ Strong consistency guarantees
- ✅ Automatic leader election  
- ✅ Fault-tolerant consensus (tikv/raft-rs)
- ✅ No single point of failure
- ✅ Log replication across nodes
- ✅ State machine for command application

### 2. Simple P2P Architecture

Lightweight peer-to-peer without consensus:

**Features:**
- ✅ No coordinator bottleneck
- ✅ Decentralized routing (consistent hashing)
- ✅ Simple peer discovery
- ✅ Lower latency
- ⚠️ Eventual consistency only

### 3. Traditional Architecture

Coordinator with Primary-Replica shards:

**Features:**
- ✅ Simple to understand and deploy
- ✅ Centralized control
- ⚠️ Coordinator is single point of failure

## Quick Start

### Raft Cluster (Recommended)

**OpenRaft Demo (Production-Ready):**
```bash
cargo run --example openraft_demo --features raft-cluster
```

Demonstrates:
- 3-node Raft cluster formation
- Automatic leader election
- Write operations through consensus (Put/Delete)
- State machine command application
- Adding learner nodes
- Membership changes
- Cluster metrics monitoring
- Graceful shutdown

### Simple P2P Cluster

```bash
cargo run --example peer_to_peer_demo --features cluster
```

### Traditional Cluster

```bash
# Terminal 1: Primary
cargo run --example primary_node --features cluster

# Terminal 2: Replica
cargo run --example replica_node --features cluster

# Terminal 3: Coordinator
cargo run --example coordinator_demo --features cluster
```

## API Examples

### OpenRaft-Based Node

```rust
use aidb::cluster::{OpenRaftNode, RaftNodeConfig, NodeId, Request};
use aidb::{Options, DB};
use std::collections::BTreeMap;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create nodes with addresses
    let mut nodes = BTreeMap::new();
    nodes.insert(1, "127.0.0.1:50051".to_string());
    nodes.insert(2, "127.0.0.1:50052".to_string());
    nodes.insert(3, "127.0.0.1:50053".to_string());
    
    // Create Raft node
    let db = DB::open("./data/node1", Options::default())?;
    let config = RaftNodeConfig {
        node_id: 1,
        listen_addr: "127.0.0.1:50051".to_string(),
    };
    
    let node = OpenRaftNode::new(config, Arc::new(db), nodes).await?;
    
    // Initialize cluster (first node only)
    node.initialize().await?;
    
    // Write through consensus
    let request = Request::Put {
        key: b"key".to_vec(),
        value: b"value".to_vec(),
    };
    node.put(b"key", b"value").await?;
    
    // Check if leader
    let is_leader = node.is_leader().await;
    
    // Get metrics
    let metrics = node.metrics().await;
    
    node.shutdown().await?;
    Ok(())
}
```

### Simple P2P Peer

```rust
use aidb::cluster::PeerNode;
use aidb::{DB, Options};
use std::sync::Arc;

let db = DB::open("./data/peer1", Options::default())?;
let peer = PeerNode::new(
    "peer1".to_string(),
    "127.0.0.1:50051".to_string(),
    Arc::new(db),
    Some(1000),  // Cache
    150,         // Virtual nodes
);

peer.join_peer("peer2".to_string(), "http://127.0.0.1:50052".to_string()).await?;
peer.handle_local_put(b"key", b"value")?;
```

## Testing

```bash
# All cluster tests
cargo test --features cluster --lib cluster

# Raft tests only
cargo test --features raft-cluster --lib cluster::raft
```

**Test Results:**
- Traditional cluster: 71 tests ✅
- Raft cluster: 17 tests ✅

## Implementation Status

### ✅ Phase 1-3: Complete

**Core Raft Implementation:**
- RaftStorage (4 tests)
- RaftNode + StateMachine (5 tests)
- RaftTransport + RaftPeer (3 tests)
- RaftBasedPeer (5 tests)

**Examples:**
- peer_to_peer_demo.rs (Simple P2P)
- openraft_demo.rs (OpenRaft Consensus) ⭐ NEW!

### ⏳ Phase 4-5: Optional

- Full RPC integration (currently placeholders)
- Network transport with retries
- Cluster membership changes
- Complete snapshot implementation
- End-to-end distributed tests
- Chaos testing
- Performance benchmarks

## Architecture Comparison

| Feature | Traditional | Simple P2P | Raft P2P |
|---------|------------|------------|----------|
| **Coordinator** | Required | None | None |
| **Consistency** | Eventual | Eventual | **Strong** |
| **Leader Election** | N/A | N/A | **Automatic** |
| **Fault Tolerance** | SPOF | Multi-node | **Consensus** |
| **Complexity** | Low | Low | Medium |
| **Production Ready** | Yes | Testing | **Yes** |
| **Use Case** | Simple | Caching | **Critical Data** |

## Performance

**Raft P2P:**
- Write: Requires majority quorum
- Read: Fast local reads
- Overhead: Raft log + messages
- Best for: Strong consistency needs

**Simple P2P:**
- Write: Single node
- Read: Local or forwarded
- Overhead: Minimal
- Best for: Eventual consistency OK

## Documentation

- [RAFT_INTEGRATION_PLAN.md](../../docs/RAFT_INTEGRATION_PLAN.md) - Implementation details
- [openraft_demo.rs](./openraft_demo.rs) - Complete OpenRaft example
- [TODO.md](../../TODO.md) - OpenRaft integration status (Phase 2-5 complete)
- Source: `src/cluster/raft_*.rs`

## Contributing

Areas of interest:
- Complete Phase 4-5
- Additional tests
- Performance optimization
- Documentation

## License

Same as AiDb project.
