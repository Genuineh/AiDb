# AiDb Multi-Raft Cluster

This directory contains the distributed Multi-Raft cluster implementation for AiDb, providing strong consistency through OpenRaft consensus.

## Architecture Overview

AiDb uses **Multi-Raft** architecture for distributed clustering:

```
                     ┌─────────────────────────────────────┐
                     │           Client Request            │
                     └─────────────────┬───────────────────┘
                                       │
                     ┌─────────────────▼───────────────────┐
                     │           Slot Router               │
                     │     CRC16(key) % 16384 → Group     │
                     └─────────────────┬───────────────────┘
                                       │
         ┌─────────────────────────────┼─────────────────────────────┐
         │                             │                             │
    ┌────▼────┐                  ┌────▼────┐                   ┌────▼────┐
    │ Group 0 │                  │ Group 1 │                   │ Group N │
    │(Raft)   │                  │(Raft)   │                   │(Raft)   │
    └────┬────┘                  └────┬────┘                   └────┬────┘
         │                             │                             │
    ┌────▼─────────────────────────────▼─────────────────────────────▼────┐
    │                         Multi-Raft Node                             │
    │                    Local LSM-Tree Storage                           │
    └─────────────────────────────────────────────────────────────────────┘
```

**Key Features:**
- ✅ Strong consistency (Raft consensus)
- ✅ Automatic leader election
- ✅ Fault tolerance (survives minority failures)
- ✅ 16384 slots (Redis Cluster compatible)
- ✅ Dynamic membership changes
- ✅ Slot migration support

## Quick Start

### Single-Group Raft Cluster

Start a basic 3-node Raft cluster:

```bash
# Build the Docker image
docker build -f deploy/Dockerfile -t aidb:cluster .

# Start the cluster
cd deploy
docker compose -f docker-compose.cluster.yml up -d

# Initialize the cluster
echo "INIT 1=http://node1:50001,2=http://node2:50002,3=http://node3:50003" | nc -q1 localhost 8001

# Test PUT/GET
echo "PUT mykey myvalue" | nc -q1 localhost 8001
echo "GET mykey" | nc -q1 localhost 8001
```

### OpenRaft Demo

Run the standalone demo:

```bash
cargo run --example openraft_demo --features raft-cluster
```

This demonstrates:
- 3-node Raft cluster formation
- Automatic leader election
- Write operations through consensus
- Adding learner nodes
- Membership changes

### Sharded Multi-Raft Demo

```bash
cargo run --example sharded_multi_raft_demo --features raft-cluster
```

### Slot Migration Demo

```bash
cargo run --example slot_migration_demo --features raft-cluster
```

## Available Examples

| Example | Description | Command |
|---------|-------------|---------|
| `openraft_demo.rs` | Basic Raft cluster | `cargo run --example openraft_demo --features raft-cluster` |
| `node_runner.rs` | Docker cluster node | Used by Docker Compose |
| `sharded_multi_raft_demo.rs` | Multi-group sharding | `cargo run --example sharded_multi_raft_demo --features raft-cluster` |
| `slot_migration_demo.rs` | Slot migration | `cargo run --example slot_migration_demo --features raft-cluster` |
| `thin_replication_demo.rs` | WAL-only replication | `cargo run --example thin_replication_demo --features raft-cluster` |
| `dynamic_member_demo.rs` | Dynamic membership | `cargo run --example dynamic_member_demo --features raft-cluster` |

## API Examples

### Basic Raft Operations

```rust
use aidb::cluster::{OpenRaftNode, RaftNodeConfig};
use aidb::{Options, DB};
use std::collections::BTreeMap;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create node configuration
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
    node.put(b"key", b"value").await?;
    
    // Read from local state machine
    let value = node.get(b"key").await?;
    
    // Check leadership status
    let is_leader = node.is_leader().await;
    
    // Get cluster metrics
    let metrics = node.metrics().await;
    
    node.shutdown().await?;
    Ok(())
}
```

### Membership Changes

```rust
// Add a new node as learner
node.add_learner(4, "127.0.0.1:50054".to_string()).await?;

// Promote to voter (change membership)
node.change_membership(&[1, 2, 3, 4]).await?;

// Remove a node
node.change_membership(&[1, 2, 4]).await?;  // Removes node 3
```

### Multi-Group Operations

```rust
use aidb::cluster::{MultiRaftNode, Router};

// Create multi-raft node
let multi_raft = MultiRaftNode::new(node_id, Arc::new(db)).await?;

// Create raft groups
multi_raft.create_group(0, vec![1, 2, 3]).await?;
multi_raft.create_group(1, vec![1, 2, 3]).await?;

// Route key to correct group
let router = Router::new();
let slot = router.calculate_slot(b"mykey");
let group_id = router.get_group_for_slot(slot);

// Write to correct group
multi_raft.put(group_id, b"mykey", b"myvalue").await?;
```

## Admin Commands

The `node_runner` binary provides admin commands via TCP:

| Command | Description | Example |
|---------|-------------|---------|
| `INIT nodes` | Initialize cluster | `INIT 1=http://node1:50001,2=http://node2:50002` |
| `PUT key value` | Write key-value | `PUT mykey myvalue` |
| `GET key` | Read value | `GET mykey` |
| `DELETE key` | Delete key | `DELETE mykey` |
| `ADD_LEARNER id=addr` | Add learner node | `ADD_LEARNER 4=http://node4:50004` |
| `CHANGE_MEMBERS ids` | Change voter set | `CHANGE_MEMBERS 1,2,3,4` |
| `LEADER` | Get current leader | `LEADER` |
| `IS_LEADER` | Check if this node is leader | `IS_LEADER` |
| `METRICS` | Get Raft metrics | `METRICS` |
| `MEMBERS` | Get membership info | `MEMBERS` |

## Testing

```bash
# Run all Raft tests
cargo test --features raft-cluster

# Run specific test suites
cargo test --features raft-cluster raft_multi_node
cargo test --features raft-cluster raft_edge_cases
cargo test --features raft-cluster raft_chaos

# Run with output
cargo test --features raft-cluster -- --nocapture
```

## Deployment Scripts

Located in `deploy/`:

| Script | Purpose |
|--------|---------|
| `verify_cluster.sh` | Verify cluster health |
| `membership_check.sh` | Test membership changes |
| `init_cluster.sh` | Initialize cluster |
| `admin_check.py` | Admin command utility |

## Documentation

- [Architecture](../../docs/ARCHITECTURE.md) - Overall architecture
- [Multi-Raft Architecture](../../docs/MULTI_RAFT_ARCHITECTURE.md) - Detailed Multi-Raft design
- [Multi-Raft Quickstart](../../docs/MULTI_RAFT_QUICKSTART.md) - Getting started guide
- [Multi-Raft API Reference](../../docs/MULTI_RAFT_API_REFERENCE.md) - Complete API docs
- [Redis Compatibility](../../docs/REDIS_CLUSTER_COMPATIBILITY.md) - Redis protocol adaptation

## Performance

| Operation | Latency | Notes |
|-----------|---------|-------|
| Write (3-node) | ~1-2ms | Requires majority quorum |
| Read (leader) | ~0.1ms | Local read |
| Read (follower) | ~0.5ms | May forward to leader |
| Leader election | ~5s | Configurable timeout |

## License

Same as AiDb project.
