# AiDb Cluster Mode

This directory contains the distributed cluster implementation for AiDb, including RPC networking, consistent hashing, and cluster coordination.

## Architecture

AiDb cluster supports two architectures:

### Peer-to-Peer (P2P) Architecture (Recommended - New!)

Equal peer nodes without centralized coordination:

#### Peer Node
- All nodes are equal participants in the cluster
- Each hosts a full LSM-tree database with persistence
- Optional LRU cache for frequently accessed data
- Independently routes requests using consistent hashing
- Forwards requests to responsible peers
- No single point of failure
- Simple peer discovery via join/leave operations
- Direct peer-to-peer health monitoring

**Benefits:**
- ✅ No coordinator bottleneck
- ✅ Better fault tolerance
- ✅ Simplified architecture
- ✅ Easier to scale
- ✅ Lower latency

### Traditional Architecture (Still Supported)

Centralized coordinator with Primary-Replica shards:

#### Primary Node
- Hosts the full LSM-tree database with persistence
- Serves all read and write operations via gRPC
- Provides health check and statistics endpoints
- Single source of truth for all data in a shard

#### Replica Node
- Maintains an LRU cache of frequently accessed data
- Forwards cache misses to the Primary node
- Invalidates cache on write operations
- Significantly reduces load on Primary for read-heavy workloads

#### Coordinator
- Routes requests to appropriate shards using consistent hashing
- Manages shard registration and discovery
- Performs health checks and failure detection
- Provides load balancing across shards
- Transparent request forwarding

## Quick Start

### Option 1: Peer-to-Peer Cluster (Recommended)

```bash
cargo run --example peer_to_peer_demo --features cluster
```

This will:
- Create 3 equal peer nodes
- Form a peer-to-peer cluster
- Demonstrate consistent hashing routing
- Show health monitoring
- Display cluster statistics

### Option 2: Traditional Architecture

### 1. Start a Primary Node

```bash
cargo run --example primary_node --features cluster
```

The Primary node will:
- Open/create a database at `./data/primary`
- Start a gRPC server on `127.0.0.1:50051`
- Serve all database operations via RPC

### 2. Start a Replica Node

```bash
cargo run --example replica_node --features cluster
```

The Replica node will:
- Connect to the Primary at `http://127.0.0.1:50051`
- Warm up its cache with frequently accessed keys
- Forward requests to Primary on cache miss
- Display hit rate statistics

### 3. Use the RPC Client

```bash
cargo run --example rpc_client --features cluster
```

This demonstrates:
- Direct gRPC client usage
- GET, PUT, DELETE operations
- Streaming SCAN operation
- Error handling

### 4. Run the Coordinator Demo (New!)

```bash
cargo run --example coordinator_demo --features cluster
```

This demonstrates:
- Starting multiple primary nodes (shards)
- Coordinator-based request routing
- Consistent hashing with load balancing
- Health checking and failure detection
- Automatic request forwarding

## API Overview

### Protobuf Service Definition

```protobuf
service Storage {
  rpc Get(GetRequest) returns (GetResponse);
  rpc Put(PutRequest) returns (PutResponse);
  rpc Delete(DeleteRequest) returns (DeleteResponse);
  rpc BatchGet(BatchGetRequest) returns (BatchGetResponse);
  rpc Write(WriteRequest) returns (WriteResponse);
  rpc Scan(ScanRequest) returns (stream ScanResponse);
  rpc HealthCheck(HealthCheckRequest) returns (HealthCheckResponse);
  rpc GetStats(GetStatsRequest) returns (GetStatsResponse);
}
```

### Usage in Code

#### Primary Node

```rust
use aidb::cluster::PrimaryNode;
use aidb::{DB, Options};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create database
    let db = DB::open("./data", Options::default())?;
    let db = Arc::new(db);
    
    // Create and start Primary node
    let primary = PrimaryNode::new(db);
    let addr = "127.0.0.1:50051".parse()?;
    primary.serve(addr).await?;
    
    Ok(())
}
```

#### Replica Node

```rust
use aidb::cluster::ReplicaNode;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create Replica with 1000-entry cache
    let mut replica = ReplicaNode::new(
        "http://127.0.0.1:50051".to_string(),
        1000,
    ).await?;
    
    // Warm up cache
    let keys = vec![b"key1".to_vec(), b"key2".to_vec()];
    replica.warmup(keys).await?;
    
    // Use replica
    let value = replica.get(b"key1").await?;
    replica.put(b"key3", b"value3").await?;
    
    // Check statistics
    let stats = replica.stats();
    println!("Hit rate: {:.2}%", stats.hit_rate() * 100.0);
    
    Ok(())
}
```

#### Coordinator (New!)

```rust
use aidb::cluster::{Coordinator, HealthChecker, HealthCheckConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create coordinator with 150 virtual nodes per shard
    let coordinator = Arc::new(Coordinator::new(150));
    
    // Register shards
    coordinator.register_shard(
        "shard1".to_string(),
        "http://127.0.0.1:50051".to_string()
    ).await?;
    
    coordinator.register_shard(
        "shard2".to_string(),
        "http://127.0.0.1:50052".to_string()
    ).await?;
    
    // Start health checker
    let health_checker = HealthChecker::new(
        coordinator.clone(),
        HealthCheckConfig::default()
    );
    health_checker.start();
    
    // Use coordinator to access data
    coordinator.put(b"key", b"value").await?;
    let response = coordinator.get(b"key").await?;
    
    if response.found {
        println!("Value: {:?}", response.value);
    }
    
    Ok(())
}
```

## Features

### Primary Node Features
- ✅ Full CRUD operations via gRPC
- ✅ Batch operations support
- ✅ Streaming scan for range queries
- ✅ Health check endpoint
- ✅ Statistics tracking (requests, errors)
- ✅ Thread-safe, concurrent access

### Replica Node Features
- ✅ LRU cache implementation
- ✅ Automatic cache miss forwarding
- ✅ Write-through cache invalidation
- ✅ Cache warming strategies
- ✅ Hit rate tracking
- ✅ Connection pooling to Primary

### Coordinator Features (New!)
- ✅ Consistent hashing with virtual nodes
- ✅ Automatic shard registration/discovery
- ✅ Request routing and load balancing
- ✅ Health checking and failure detection
- ✅ Transparent request forwarding
- ✅ O(log N) routing performance

## Performance Characteristics

### Primary Node
- **Throughput**: Depends on underlying DB performance
- **Latency**: Single-digit millisecond for local operations
- **Overhead**: Minimal gRPC serialization overhead (~5-10%)

### Replica Node
- **Cache Hit Latency**: < 1ms (in-memory lookup)
- **Cache Miss Latency**: Primary latency + network RTT
- **Hit Rate**: Typically 80-90% for read-heavy workloads
- **Cache Size**: Configurable (recommend 10,000-100,000 entries)

### Network Optimization (Week 24)
- Connection pooling reduces connection overhead
- Batch operations minimize round trips
- Optional compression for large values
- Keep-alive for persistent connections

## Testing

Run all cluster tests:

```bash
cargo test --features cluster
```

Run specific test suites:

```bash
# RPC network tests
cargo test --features cluster --test cluster_rpc_tests

# Coordinator tests (Week 25-28)
cargo test --features cluster --test coordinator_tests
```

### Test Coverage
- **RPC Tests**: 7 tests covering Primary/Replica operations
- **Coordinator Tests**: 37 tests covering consistent hashing, routing, and health checking
- **Total**: 344+ tests passing across the entire project

## Configuration

### Primary Node Configuration

The Primary node uses the same `Options` as the standalone DB:

```rust
let mut options = Options::default();
options.memtable_size = 64 * 1024 * 1024;  // 64MB
options.max_level_0_files = 8;
// ... other options

let db = DB::open("./data", options)?;
let primary = PrimaryNode::new(Arc::new(db));
```

### Replica Node Configuration

```rust
let cache_capacity = 10000;  // Number of entries
let replica = ReplicaNode::new(
    "http://primary:50051".to_string(),
    cache_capacity,
).await?;
```

### Coordinator Configuration (New!)

```rust
use std::time::Duration;

// Create coordinator with virtual nodes
let coordinator = Coordinator::new(150); // 150 virtual nodes per shard

// Configure health checker
let health_config = HealthCheckConfig {
    check_interval: Duration::from_secs(10),
    timeout: Duration::from_secs(5),
    failure_threshold: 3,
    success_threshold: 2,
};
let health_checker = HealthChecker::new(coordinator.clone(), health_config);
```

## Limitations & Future Work

### Current Limitations
- Single Primary per shard (no Primary-Primary replication)
- No automatic failover for Primary nodes
- Cache invalidation is immediate (no TTL)
- Manual shard registration (no auto-discovery)

### Completed (Week 21-28)
- ✅ RPC framework with gRPC/tonic
- ✅ Primary and Replica nodes
- ✅ Connection pooling
- ✅ Consistent hashing
- ✅ Coordinator for routing
- ✅ Health checking and failure detection

### Planned Enhancements (Week 29+)
- [ ] Shard group management
- [ ] Multi-shard transactions
- [ ] Automatic rebalancing
- [ ] Backup and recovery
- [ ] Dynamic scaling
- [ ] Auto-discovery of shards

## See Also

- [Implementation Plan](../../docs/IMPLEMENTATION.md) - Full 48-week roadmap
- [TODO](../../TODO.md) - Task tracking and progress
- [Coordinator Completion Summary](../../docs/completions/COORDINATOR_COMPLETION_SUMMARY.md) - Week 25-28 details
- [Architecture Documentation](../../docs/) - System design documents
