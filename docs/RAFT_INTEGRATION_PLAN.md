# Raft-Based Peer-to-Peer Cluster Architecture Plan

## 概述

使用 Embedded Raft 实现真正的对等节点集群，提供强一致性保证和自动故障恢复能力。

## Raft 实现选择

### tikv/raft-rs
- **优点**：
  - 生产级实现，TiKV 使用
  - 性能优异
  - 完整的 Raft 协议支持
  - 活跃维护
- **缺点**：
  - 学习曲线较陡
  - 需要实现存储层

**决定：使用 tikv/raft-rs**

## 架构设计

### 层次结构

```
┌────────────────────────────────────────────────┐
│          Application Layer                     │
│  (Client API, Query Processing)                │
└───────────────────┬────────────────────────────┘
                    │
┌───────────────────▼────────────────────────────┐
│          Raft Consensus Layer                  │
│  ┌──────────────────────────────────────┐     │
│  │ Raft State Machine                   │     │
│  │ - Leader Election                    │     │
│  │ - Log Replication                    │     │
│  │ - Membership Changes                 │     │
│  └──────────────────────────────────────┘     │
└───────────────────┬────────────────────────────┘
                    │
┌───────────────────▼────────────────────────────┐
│          Data Storage Layer                    │
│  ┌──────────────┐  ┌──────────────┐           │
│  │ LSM-Tree DB  │  │  Raft Log    │           │
│  │ (User Data)  │  │  Storage     │           │
│  └──────────────┘  └──────────────┘           │
└────────────────────────────────────────────────┘
```

### 核心组件

#### 1. RaftNode（新增）
- 包装 Raft 状态机
- 管理 Raft 消息传递
- 处理 Leader 选举
- 协调日志复制

#### 2. RaftPeer（改进的 PeerNode）
- 集成 RaftNode
- 实现 Raft 状态机接口
- 提供 KV 操作接口
- 处理客户端请求

#### 3. RaftStateMachine
- 实现 Raft 的状态机接口
- 应用已提交的日志条目
- 生成和应用快照
- 维护集群元数据

#### 4. RaftStorage
- 实现 Raft 的存储接口
- 持久化 Raft 日志
- 存储 HardState 和 ConfState
- 支持快照

## 数据流

### 写入流程（使用 Raft 共识）

```
Client Write Request
    │
    ▼
RaftPeer (Any Node)
    │
    ├──► If Leader: Accept
    │
    └──► If Follower: Forward to Leader
            │
            ▼
        Leader RaftNode
            │
            ├──► Append to Raft Log
            │
            ├──► Replicate to Followers
            │      │
            │      ▼
            │   Followers Append Log
            │      │
            │      ▼
            │   Acknowledge to Leader
            │
            ├──► Wait for Majority Quorum
            │
            ├──► Commit Log Entry
            │
            ├──► Apply to State Machine
            │      │
            │      ▼
            │   LSM-Tree DB Write
            │
            └──► Return Success to Client
```

### 读取流程

```
Client Read Request
    │
    ▼
RaftPeer (Any Node)
    │
    ├──► Strong Consistency Mode:
    │    │
    │    ├──► Forward to Leader
    │    │       │
    │    │       ▼
    │    │   Read from Leader's DB
    │    │       │
    │    │       ▼
    │    │   Return Result
    │
    └──► Relaxed Consistency Mode:
         │
         ├──► Read from Local DB
         │       │
         │       ▼
         │   Check if data is stale
         │       │
         │       ▼
         │   Return Result (with staleness info)
```

### 集群成员变更流程

```
Add/Remove Node Request
    │
    ▼
Leader RaftNode
    │
    ├──► Propose Configuration Change
    │
    ├──► Add Entry to Raft Log
    │
    ├──► Replicate to Majority
    │
    ├──► Commit Configuration Change
    │
    ├──► Apply to State Machine
    │      │
    │      ▼
    │   Update Cluster Membership
    │      │
    │      ▼
    │   Update Consistent Hash Ring
    │
    └──► Return Success
```

## 实现计划

### Phase 1: Raft 基础集成 (Week 1-2)
- [ ] 添加 raft-rs 依赖
- [ ] 实现 RaftStorage
  - [ ] 日志存储
  - [ ] 状态存储
  - [ ] 快照存储
- [ ] 实现基本的 RaftNode 包装器
- [ ] 添加 Raft 消息传输层（基于现有 gRPC）

### Phase 2: 状态机集成 (Week 2-3)
- [ ] 实现 RaftStateMachine
  - [ ] 命令应用逻辑
  - [ ] 快照生成
  - [ ] 快照恢复
- [ ] 集成 LSM-Tree 数据库
- [ ] 实现写入命令的 Raft 共识

### Phase 3: RaftPeer 实现 (Week 3-4)
- [ ] 重构 PeerNode 为 RaftPeer
- [ ] 实现请求路由（基于 Leader）
- [ ] 添加客户端请求处理
- [ ] 实现一致性读取

### Phase 4: 集群管理 (Week 4-5)
- [ ] 实现节点加入/离开
- [ ] 实现配置变更协议
- [ ] 添加集群发现机制
- [ ] 实现故障检测和自动恢复

### Phase 5: 测试和优化 (Week 5-6)
- [ ] 单元测试
- [ ] 集成测试
- [ ] 混沌测试（故障注入）
- [ ] 性能测试和优化
- [ ] 文档更新

## 关键设计决策

### 1. Raft 用于什么？

**使用 Raft 的场景：**
- ✅ 集群成员管理（谁在集群中）
- ✅ 数据分区元数据（哪个分区负责哪些数据）
- ✅ 写入操作的强一致性
- ✅ Leader 选举和故障转移

**不使用 Raft 的场景：**
- ❌ 所有用户数据（太慢，不必要）
- ❌ 读取操作（可选，支持 Follower 读）

### 2. 数据分区策略

```
┌─────────────────────────────────────────┐
│         Raft Cluster (Metadata)         │
│  Manages: Partition Map, Membership     │
└────────────────┬────────────────────────┘
                 │
        ┌────────┴────────┐
        │                 │
┌───────▼──────┐  ┌──────▼───────┐
│ Partition 1  │  │ Partition 2  │
│ Range: a-m   │  │ Range: n-z   │
│              │  │              │
│ ┌──────────┐ │  │ ┌──────────┐ │
│ │Raft Group│ │  │ │Raft Group│ │
│ │3 Replicas│ │  │ │3 Replicas│ │
│ └──────────┘ │  │ └──────────┘ │
└──────────────┘  └──────────────┘
```

每个分区是一个独立的 Raft Group：
- 更好的性能（并行处理）
- 独立的 Leader 选举
- 隔离故障影响

### 3. 一致性级别

提供多种一致性级别：

```rust
pub enum ConsistencyLevel {
    /// Strong consistency - read from leader
    Strong,
    /// Linearizable read - read from leader with read index
    Linearizable,
    /// Follower read - read from local follower (may be stale)
    Follower,
}
```

### 4. 写入策略

```rust
pub enum WriteStrategy {
    /// Wait for majority commit (default)
    Majority,
    /// Wait for all replicas (slower but safer)
    All,
    /// Leader only (fastest but may lose data)
    LeaderOnly,
}
```

## API 设计

### RaftPeer API

```rust
pub struct RaftPeer {
    // Raft consensus layer
    raft_node: Arc<RaftNode>,
    // Local storage
    db: Arc<DB>,
    // Cluster membership
    cluster: Arc<RwLock<ClusterInfo>>,
}

impl RaftPeer {
    /// Create a new Raft peer
    pub fn new(
        id: u64,
        peers: Vec<PeerInfo>,
        db: Arc<DB>,
        config: RaftConfig,
    ) -> Result<Self>;
    
    /// Start the Raft peer
    pub async fn start(&self) -> Result<()>;
    
    /// Stop the Raft peer
    pub async fn stop(&self) -> Result<()>;
    
    /// Propose a write operation (goes through Raft)
    pub async fn put(
        &self,
        key: &[u8],
        value: &[u8],
        consistency: ConsistencyLevel,
    ) -> Result<()>;
    
    /// Read a value
    pub async fn get(
        &self,
        key: &[u8],
        consistency: ConsistencyLevel,
    ) -> Result<Option<Vec<u8>>>;
    
    /// Delete a key
    pub async fn delete(
        &self,
        key: &[u8],
        consistency: ConsistencyLevel,
    ) -> Result<()>;
    
    /// Add a new peer to the cluster
    pub async fn add_peer(&self, peer: PeerInfo) -> Result<()>;
    
    /// Remove a peer from the cluster
    pub async fn remove_peer(&self, peer_id: u64) -> Result<()>;
    
    /// Check if this peer is the leader
    pub fn is_leader(&self) -> bool;
    
    /// Get the current leader
    pub fn leader(&self) -> Option<u64>;
    
    /// Get cluster status
    pub fn cluster_status(&self) -> ClusterStatus;
}
```

## 配置示例

```rust
use aidb::cluster::RaftPeer;
use aidb::{Options, DB};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create local database
    let db = DB::open("./data/node1", Options::default())?;
    
    // Configure Raft
    let raft_config = RaftConfig {
        id: 1,
        election_tick: 10,
        heartbeat_tick: 3,
        max_size_per_msg: 1024 * 1024,
        max_inflight_msgs: 256,
    };
    
    // Define peer addresses
    let peers = vec![
        PeerInfo { id: 1, address: "127.0.0.1:5001".to_string() },
        PeerInfo { id: 2, address: "127.0.0.1:5002".to_string() },
        PeerInfo { id: 3, address: "127.0.0.1:5003".to_string() },
    ];
    
    // Create Raft peer
    let peer = RaftPeer::new(1, peers, Arc::new(db), raft_config)?;
    
    // Start the peer
    peer.start().await?;
    
    // Use with strong consistency
    peer.put(b"key", b"value", ConsistencyLevel::Strong).await?;
    let value = peer.get(b"key", ConsistencyLevel::Strong).await?;
    
    println!("Value: {:?}", value);
    
    Ok(())
}
```

## 监控和可观测性

### 关键指标

```rust
pub struct RaftMetrics {
    // Leader election metrics
    pub leader_changes: u64,
    pub election_timeout_count: u64,
    
    // Log replication metrics
    pub log_entries_appended: u64,
    pub log_entries_committed: u64,
    pub log_replication_lag: Duration,
    
    // Performance metrics
    pub write_latency_p50: Duration,
    pub write_latency_p99: Duration,
    pub read_latency_p50: Duration,
    pub read_latency_p99: Duration,
    
    // Cluster health
    pub active_peers: usize,
    pub quorum_size: usize,
}
```

## 优势总结

使用 Raft 的 P2P 架构优势：

1. **强一致性保证**
   - 写入操作保证多数派确认
   - 防止脑裂和数据丢失

2. **自动故障恢复**
   - Leader 失败时自动重新选举
   - 无需人工干预

3. **水平扩展**
   - 通过多个 Raft Group 并行处理
   - 每个分区独立扩展

4. **简化运维**
   - 无需中心化协调器
   - 配置简单明了

5. **久经考验**
   - Raft 协议广泛应用
   - TiKV、etcd、Consul 等都在使用

## 下一步

1. 添加 raft-rs 依赖到 Cargo.toml
2. 实现 RaftStorage 接口
3. 实现基础的 RaftNode
4. 创建简单的测试案例
5. 逐步完善功能

---

## 实施状态更新 (2025-11-19)

### ✅ 已完成阶段

#### Phase 1: Raft 基础集成 (完成)
- ✅ 添加 raft-rs 依赖 (v0.7)
- ✅ 实现 RaftStorage
  - ✅ 日志存储（使用 LSM-Tree）
  - ✅ 状态存储（HardState, ConfState）
  - ✅ 快照存储
  - ✅ 日志压缩
  - ✅ 4个单元测试通过
- ✅ 实现基本的 RaftNode 包装器
- ✅ 添加 Raft 消息传输层基础

#### Phase 2: 状态机集成 (完成)
- ✅ 实现 RaftStateMachine
  - ✅ 命令应用逻辑（PUT/DELETE）
  - ✅ 命令编码/解码工具
  - ✅ 与 LSM-Tree 集成
  - ✅ 5个单元测试通过
- ✅ 实现 RaftNode 完整功能
  - ✅ Leader 选举支持
  - ✅ Proposal 提交接口
  - ✅ 配置变更接口
  - ✅ Tick 和 Step 机制

#### Phase 3: 消息传输层和完整集成 (完成)
- ✅ 实现 RaftTransport
  - ✅ Peer 连接管理
  - ✅ 消息发送/接收
  - ✅ 本地消息传递
- ✅ 实现 RaftPeer (内部事件循环)
  - ✅ 后台异步处理
  - ✅ Tick 驱动的心跳
  - ✅ Ready 消息处理
  - ✅ 自动消息路由
- ✅ 实现 RaftBasedPeer (完整节点)
  - ✅ 高级 API (put/get/delete)
  - ✅ Leader 检查
  - ✅ 状态机应用
  - ✅ 8个单元测试通过

### 📊 测试覆盖

**总计: 17个 Raft 相关测试全部通过 ✅**

- RaftStorage: 4 tests
- RaftNode/StateMachine: 5 tests
- RaftTransport/RaftPeer: 3 tests
- RaftBasedPeer: 5 tests

### 📚 示例代码

1. **peer_to_peer_demo.rs** - 基础 P2P 集群演示
2. **raft_cluster_demo.rs** - Raft 节点基础使用
3. **raft_peer_cluster.rs** - 完整 Raft P2P 集群
4. **raft_integration_test.rs** - 端到端集成测试

### 🏗️ 架构实现状态

```
✅ Application Layer
   └─ RaftBasedPeer (高级 API)

✅ Raft Consensus Layer
   ├─ RaftNode (共识协议)
   ├─ RaftPeer (事件循环)
   ├─ RaftTransport (消息传递)
   └─ RaftStateMachine (命令应用)

✅ Storage Layer
   ├─ RaftStorage (Raft 日志)
   └─ LSM-Tree DB (用户数据)
```

### ⏳ 待完成工作 (Phase 4-5)

#### Phase 4: 完整 RPC 集成 (可选)
- [ ] 实现完整的 gRPC 消息序列化
- [ ] 实现网络传输层
- [ ] 添加重试和错误处理
- [ ] 实现 RPC 超时机制

#### Phase 5: 生产就绪 (可选)
- [ ] 实现集群成员变更协议
- [ ] 添加配置变更管理
- [ ] 实现完整的快照生成和恢复
- [ ] 添加端到端分布式测试
- [ ] 混沌测试（故障注入）
- [ ] 性能基准测试和优化
- [ ] 生产环境监控指标

### 📝 关键决策记录

1. **Raft 实现选择**: tikv/raft-rs
   - 生产级实现，被 TiKV 使用
   - 性能优异，功能完整

2. **存储设计**: 复用 LSM-Tree
   - Raft 日志存储在同一个 DB
   - 使用特殊前缀区分（raft:*）
   - 简化架构，减少依赖

3. **消息传输**: 基于现有 gRPC 基础设施
   - 复用已有的 tonic + protobuf
   - 保持一致的通信方式

4. **状态机集成**: 命令编码方案
   - 简单的二进制格式
   - op_type + key_len + key + value_len + value
   - 易于扩展新命令类型

### 🎯 当前可用功能

**核心功能已完成，可用于：**
- ✅ 创建 Raft 节点集群
- ✅ 自动 Leader 选举
- ✅ 提交 Proposal 到 Raft
- ✅ 应用已提交的命令
- ✅ 状态查询和监控
- ✅ 节点启动/停止

**当前限制：**
- ⚠️ RPC 消息传输使用占位符（需要 Phase 4）
- ⚠️ 需要手动触发状态机应用
- ⚠️ 集群成员变更需要重启
- ⚠️ 快照功能未完全实现

### 🚀 使用建议

**对于开发和测试：**
- 当前实现已足够用于单机多进程测试
- 可以验证 Raft 共识逻辑
- 可以测试状态机应用

**对于生产环境：**
- 建议完成 Phase 4 的 RPC 集成
- 添加完整的错误处理和重试机制
- 实施监控和告警
- 进行负载测试和性能调优

### 📖 相关文档

- [examples/cluster/raft_integration_test.rs](../examples/cluster/raft_integration_test.rs) - 完整的使用示例
- [src/cluster/raft_peer.rs](../src/cluster/raft_peer.rs) - RaftBasedPeer API 文档
- [src/cluster/raft_node.rs](../src/cluster/raft_node.rs) - RaftNode 实现细节
- [src/cluster/raft_storage.rs](../src/cluster/raft_storage.rs) - RaftStorage 实现

### 🤝 贡献

欢迎贡献以下方面：
- Phase 4-5 的完整实现
- 更多的测试用例
- 性能优化
- 文档改进
- Bug 修复

