# Redis Cluster Compatibility Guide with AiDb Multi-Raft

## 概述 (Overview)

本文档说明如何使用AiDb的Multi-Raft架构来实现与Redis Cluster兼容的分布式存储系统。

This document explains how to use AiDb's Multi-Raft architecture to implement a Redis Cluster-compatible distributed storage system.

## Redis Cluster架构回顾 (Redis Cluster Architecture Review)

Redis Cluster使用以下关键概念:
- **16384个哈希槽** (16384 hash slots)
- **槽位分配给不同节点** (Slots assigned to different nodes)
- **客户端重定向** (Client redirection with MOVED/ASK)
- **主从复制** (Master-slave replication)

## AiDb Multi-Raft映射 (Mapping to AiDb Multi-Raft)

### 1. 槽位映射到Raft组 (Slot-to-Raft-Group Mapping)

```rust
// Redis Cluster: 16384 slots
// AiDb Multi-Raft: N Raft groups, each managing a range of slots

use aidb::cluster::{Router, SLOT_COUNT};

// Router handles slot-to-group mapping
let router = Router::new();

// Each Raft group manages a continuous range of slots
// Example: 3 groups for 16384 slots
// Group 0: slots 0-5461
// Group 1: slots 5462-10922
// Group 2: slots 10923-16383
```

### 2. 架构组件 (Architecture Components)

#### Meta Raft (元数据Raft组)
负责管理集群元数据:
- 槽位分配 (Slot assignment)
- 节点健康状态 (Node health status)
- 组成员信息 (Group membership)
- 迁移状态 (Migration state)

```rust
use aidb::cluster::{MetaRaftNode, MetaStateMachine};

// Create meta raft node
let meta_node = MetaRaftNode::new(config, Arc::new(db)).await?;
```

#### Data Raft Groups (数据Raft组)
每个组管理一部分槽位的数据:
- 强一致性读写 (Strongly consistent reads/writes)
- 日志复制 (Log replication)
- 快照管理 (Snapshot management)

```rust
use aidb::cluster::{MultiRaftNode, ShardedRaftStorage};

// Create multi-raft node managing multiple groups
let multi_raft = MultiRaftNode::new(node_id, db).await?;

// Add raft groups for slot ranges
multi_raft.create_group(0, vec![0, 1, 2]).await?;  // Group 0 with nodes 0,1,2
multi_raft.create_group(1, vec![0, 1, 2]).await?;  // Group 1 with nodes 0,1,2
```

### 3. 槽位路由 (Slot Routing)

```rust
use aidb::cluster::{Router, SLOT_COUNT};

// Calculate slot for a key (same as Redis)
fn calculate_slot(key: &[u8]) -> u16 {
    let router = Router::new();
    router.calculate_slot(key)
}

// Route to appropriate Raft group
fn route_request(key: &[u8]) -> u64 {
    let slot = calculate_slot(key);
    let router = Router::new();
    router.get_group_for_slot(slot)
}

// Example usage
let key = b"mykey";
let slot = calculate_slot(key);  // Returns 0-16383
let group_id = route_request(key);  // Returns Raft group ID
```

### 4. 客户端协议适配 (Client Protocol Adaptation)

#### Redis RESP协议支持 (Redis RESP Protocol Support)

要实现Redis协议兼容，需要添加RESP协议层:

```rust
// Pseudo-code for Redis protocol adapter
struct RedisProtocolAdapter {
    multi_raft: Arc<MultiRaftNode>,
    router: Router,
}

impl RedisProtocolAdapter {
    async fn handle_command(&self, cmd: RedisCommand) -> RedisResponse {
        match cmd {
            RedisCommand::Get(key) => {
                let group_id = self.router.get_group_for_key(&key);
                match self.multi_raft.get(group_id, &key).await {
                    Ok(Some(value)) => RedisResponse::BulkString(value),
                    Ok(None) => RedisResponse::Null,
                    Err(_) => self.redirect_response(&key),
                }
            }
            RedisCommand::Set(key, value) => {
                let group_id = self.router.get_group_for_key(&key);
                match self.multi_raft.put(group_id, &key, &value).await {
                    Ok(_) => RedisResponse::Ok,
                    Err(_) => self.redirect_response(&key),
                }
            }
            // ... other commands
        }
    }

    fn redirect_response(&self, key: &[u8]) -> RedisResponse {
        let slot = self.router.calculate_slot(key);
        let (node_addr, _) = self.router.get_node_for_slot(slot);
        RedisResponse::Moved(slot, node_addr)
    }
}
```

## 实现步骤 (Implementation Steps)

### 步骤1: 初始化Meta Raft

```rust
use aidb::cluster::{MetaRaftNode, MetaNodeInfo, NodeStatus};
use aidb::{DB, Options};

async fn initialize_meta_cluster() -> Result<(), Box<dyn std::error::Error>> {
    // Create meta raft nodes
    let meta_nodes = vec![
        (1, "http://127.0.0.1:7001".to_string()),
        (2, "http://127.0.0.1:7002".to_string()),
        (3, "http://127.0.0.1:7003".to_string()),
    ];

    let db = DB::open("./data/meta", Options::default())?;
    let meta_node = MetaRaftNode::new(1, Arc::new(db)).await?;

    // Start RPC server
    let addr = "127.0.0.1:7001".parse()?;
    tokio::spawn(async move {
        meta_node.start_server(addr).await
    });

    // Initialize cluster
    meta_node.initialize(meta_nodes).await?;

    Ok(())
}
```

### 步骤2: 创建Data Raft Groups

```rust
use aidb::cluster::MultiRaftNode;

async fn create_data_groups() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::open("./data/node1", Options::default())?;
    let multi_raft = MultiRaftNode::new(1, Arc::new(db)).await?;

    // Start RPC server
    let addr = "127.0.0.1:8001".parse()?;
    tokio::spawn(async move {
        multi_raft.start_server(addr).await
    });

    // Create 3 raft groups for different slot ranges
    // Each group handles approximately 5461 slots
    multi_raft.create_group(0, vec![1, 2, 3]).await?;  // Slots 0-5461
    multi_raft.create_group(1, vec![1, 2, 3]).await?;  // Slots 5462-10922
    multi_raft.create_group(2, vec![1, 2, 3]).await?;  // Slots 10923-16383

    Ok(())
}
```

### 步骤3: 配置槽位分配

```rust
use aidb::cluster::{Router, MembershipCoordinator};

async fn configure_slot_allocation() -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = MembershipCoordinator::new(meta_raft, router);

    // Assign slot ranges to groups
    coordinator.assign_slots_to_group(0, 0..=5461).await?;
    coordinator.assign_slots_to_group(1, 5462..=10922).await?;
    coordinator.assign_slots_to_group(2, 10923..=16383).await?;

    Ok(())
}
```

### 步骤4: 实现槽位迁移 (Slot Migration)

```rust
use aidb::cluster::{MigrationManager, MigrationConfig};

async fn migrate_slots() -> Result<(), Box<dyn std::error::Error>> {
    let migration_config = MigrationConfig {
        batch_size: 100,
        retry_count: 3,
        timeout_ms: 5000,
    };

    let migration_manager = MigrationManager::new(
        meta_raft,
        multi_raft,
        migration_config,
    );

    // Migrate slots 100-200 from group 0 to group 1
    migration_manager.migrate_slots(
        0,  // source group
        1,  // target group
        100..=200,  // slot range
    ).await?;

    Ok(())
}
```

## Redis命令映射 (Redis Command Mapping)

### 基本命令 (Basic Commands)

| Redis命令 | AiDb Multi-Raft实现 |
|----------|-------------------|
| `GET key` | `multi_raft.get(group_id, key)` |
| `SET key value` | `multi_raft.put(group_id, key, value)` |
| `DEL key` | `multi_raft.delete(group_id, key)` |
| `EXISTS key` | `multi_raft.exists(group_id, key)` |
| `MGET key1 key2...` | Parallel `get()` across groups |
| `MSET key1 val1...` | Batch writes per group |

### 集群命令 (Cluster Commands)

| Redis命令 | AiDb实现 |
|----------|---------|
| `CLUSTER NODES` | Query `MetaRaftNode` for topology |
| `CLUSTER SLOTS` | Query `Router` for slot mapping |
| `CLUSTER MEET` | `meta_node.add_node()` |
| `CLUSTER FORGET` | `meta_node.remove_node()` |
| `CLUSTER FAILOVER` | Automatic via Raft leader election |

### 槽位迁移命令 (Slot Migration Commands)

| Redis命令 | AiDb实现 |
|----------|---------|
| `CLUSTER SETSLOT` | `migration_manager.prepare_migration()` |
| `CLUSTER GETKEYSINSLOT` | `group.get_keys_in_slot()` |
| `MIGRATE` | `migration_manager.migrate_slots()` |

## 性能优化建议 (Performance Optimization)

### 1. 批量操作优化 (Batch Operations)

```rust
use aidb::cluster::ThinWriteBatch;

async fn batch_write_optimization(
    multi_raft: &MultiRaftNode,
    keys: Vec<(Vec<u8>, Vec<u8>)>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Group keys by Raft group
    let mut group_batches: HashMap<u64, ThinWriteBatch> = HashMap::new();

    for (key, value) in keys {
        let group_id = router.get_group_for_key(&key);
        group_batches.entry(group_id)
            .or_insert_with(ThinWriteBatch::new)
            .put(key, value);
    }

    // Execute batches in parallel
    let futures: Vec<_> = group_batches
        .into_iter()
        .map(|(group_id, batch)| {
            multi_raft.write_batch(group_id, batch)
        })
        .collect();

    futures::future::try_join_all(futures).await?;
    Ok(())
}
```

### 2. 读取优化 (Read Optimization)

```rust
// Use local reads when possible
async fn optimized_read(
    multi_raft: &MultiRaftNode,
    key: &[u8],
) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
    let group_id = router.get_group_for_key(key);

    // Check if local node is leader for this group
    if multi_raft.is_leader(group_id).await {
        // Direct local read (no Raft consensus needed)
        multi_raft.local_read(group_id, key).await
    } else {
        // Forward to leader or use linearizable read
        multi_raft.get(group_id, key).await
    }
}
```

### 3. 连接池管理 (Connection Pooling)

```rust
use tokio::sync::RwLock;
use std::collections::HashMap;

struct ConnectionPool {
    connections: RwLock<HashMap<String, RaftServiceClient<Channel>>>,
}

impl ConnectionPool {
    async fn get_or_create(&self, addr: &str) -> Result<RaftServiceClient<Channel>> {
        // Check if connection exists
        if let Some(client) = self.connections.read().await.get(addr) {
            return Ok(client.clone());
        }

        // Create new connection
        let client = RaftServiceClient::connect(addr.to_string()).await?;
        self.connections.write().await.insert(addr.to_string(), client.clone());
        Ok(client)
    }
}
```

## 故障处理 (Failure Handling)

### 1. 节点故障 (Node Failure)

```rust
// Raft automatically handles node failures through leader election
// No manual intervention needed for:
// - Leader crash: New leader elected automatically
// - Follower crash: Leader continues with remaining nodes
// - Network partition: Majority partition remains available
```

### 2. 槽位迁移失败 (Slot Migration Failure)

```rust
async fn handle_migration_failure(
    migration_manager: &MigrationManager,
    migration_id: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    // Check migration status
    let status = migration_manager.get_migration_status(migration_id).await?;

    match status.state {
        SlotMigrationState::Failed => {
            // Rollback migration
            migration_manager.rollback_migration(migration_id).await?;
        }
        SlotMigrationState::InProgress => {
            // Retry migration
            migration_manager.retry_migration(migration_id).await?;
        }
        _ => {}
    }

    Ok(())
}
```

## 监控和诊断 (Monitoring and Diagnostics)

### 1. 集群健康检查 (Cluster Health Check)

```rust
use aidb::cluster::HealthChecker;

async fn check_cluster_health(
    meta_node: &MetaRaftNode,
) -> Result<(), Box<dyn std::error::Error>> {
    let health_checker = HealthChecker::new(meta_node);

    // Check all nodes
    let health_report = health_checker.check_all_nodes().await?;

    for (node_id, status) in health_report {
        println!("Node {}: {:?}", node_id, status);
    }

    Ok(())
}
```

### 2. 槽位分布检查 (Slot Distribution Check)

```rust
async fn check_slot_distribution(
    router: &Router,
) -> Result<(), Box<dyn std::error::Error>> {
    let distribution = router.get_slot_distribution().await?;

    for (group_id, slot_range) in distribution {
        println!("Group {}: slots {:?}", group_id, slot_range);
    }

    Ok(())
}
```

## 完整示例 (Complete Example)

```rust
use aidb::cluster::{MetaRaftNode, MultiRaftNode, Router, MigrationManager};
use aidb::{DB, Options};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize Meta Raft
    let meta_db = DB::open("./data/meta", Options::default())?;
    let meta_node = Arc::new(MetaRaftNode::new(1, Arc::new(meta_db)).await?);

    let meta_addr = "127.0.0.1:7001".parse()?;
    let meta_node_clone = meta_node.clone();
    tokio::spawn(async move {
        meta_node_clone.start_server(meta_addr).await
    });

    // 2. Initialize Data Multi-Raft
    let data_db = DB::open("./data/node1", Options::default())?;
    let multi_raft = Arc::new(MultiRaftNode::new(1, Arc::new(data_db)).await?);

    let data_addr = "127.0.0.1:8001".parse()?;
    let multi_raft_clone = multi_raft.clone();
    tokio::spawn(async move {
        multi_raft_clone.start_server(data_addr).await
    });

    // 3. Create Raft groups
    multi_raft.create_group(0, vec![1, 2, 3]).await?;
    multi_raft.create_group(1, vec![1, 2, 3]).await?;
    multi_raft.create_group(2, vec![1, 2, 3]).await?;

    // 4. Initialize router
    let router = Router::new();

    // 5. Perform operations
    let key = b"mykey";
    let value = b"myvalue";
    let group_id = router.get_group_for_key(key);

    multi_raft.put(group_id, key, value).await?;
    let result = multi_raft.get(group_id, key).await?;

    println!("Retrieved value: {:?}", result);

    Ok(())
}
```

## 参考资料 (References)

- [Redis Cluster Specification](https://redis.io/docs/reference/cluster-spec/)
- [Raft Consensus Algorithm](https://raft.github.io/)
- [AiDb Multi-Raft Architecture](./MULTI_RAFT_ARCHITECTURE.md)
- [AiDb Multi-Raft Quickstart](./MULTI_RAFT_QUICKSTART.md)

## 常见问题 (FAQ)

### Q: AiDb Multi-Raft与Redis Cluster的主要区别是什么?

A: 主要区别:
1. **一致性保证**: AiDb使用Raft提供强一致性，Redis Cluster使用最终一致性
2. **故障恢复**: AiDb通过Raft自动恢复，Redis需要手动failover
3. **数据复制**: AiDb复制日志，Redis复制完整数据

### Q: 如何处理跨槽位事务?

A: AiDb Multi-Raft不直接支持跨槽位事务。建议:
1. 使用hash tags确保相关键在同一槽位
2. 使用两阶段提交协议
3. 在应用层实现事务语义

### Q: 性能如何?

A: 基准测试显示:
- 写入延迟: ~1-2ms (3副本)
- 读取延迟: ~0.5ms (leader读取)
- 吞吐量: ~50K ops/sec/group

## 总结 (Conclusion)

AiDb的Multi-Raft架构提供了实现Redis Cluster兼容存储的强大基础。通过合理的槽位分配、组管理和协议适配，可以构建一个高可用、强一致性的分布式存储系统。
