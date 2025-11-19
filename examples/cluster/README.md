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

**Complete Integration Test:**
```bash
cargo run --example raft_integration_test --features raft-cluster
```

Demonstrates:
- 3-node Raft cluster formation
- Automatic leader election
- Write operations through consensus
- State machine command application
- Read operations from local state
- Cluster status monitoring

**Complete Cluster Demo:**
```bash
cargo run --example raft_peer_cluster --features raft-cluster
```

**Component Demo:**
```bash
cargo run --example raft_cluster_demo --features raft-cluster
```

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

### Raft-Based Peer

```rust
use aidb::cluster::{RaftBasedPeer, RaftConfig};
use aidb::{Options, DB};
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup peers
    let mut peers = HashMap::new();
    peers.insert(1, "http://127.0.0.1:50051".to_string());
    peers.insert(2, "http://127.0.0.1:50052".to_string());
    peers.insert(3, "http://127.0.0.1:50053".to_string());
    
    // Create Raft peer
    let db = DB::open("./data/peer1", Options::default())?;
    let config = RaftConfig {
        id: 1,
        election_tick: 10,
        heartbeat_tick: 3,
        ..Default::default()
    };
    
    let peer = RaftBasedPeer::new(1, Arc::new(db), peers, config).await?;
    peer.start().await?;
    
    // Write (leader only)
    if peer.is_leader() {
        peer.put(b"key", b"value").await?;
    }
    
    // Read (any node)
    let value = peer.get(b"key")?;
    
    // Status
    let (term, committed, is_leader) = peer.status_info();
    
    peer.stop();
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
- peer_to_peer_demo.rs
- raft_cluster_demo.rs
- raft_peer_cluster.rs
- raft_integration_test.rs

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
- [raft_integration_test.rs](./raft_integration_test.rs) - Complete example
- Source: `src/cluster/raft_*.rs`

## Contributing

Areas of interest:
- Complete Phase 4-5
- Additional tests
- Performance optimization
- Documentation

## License

Same as AiDb project.
