# AiDb Multi-Raft + 分片完整落地计划

**目标**: 将 AiDb 从"单 Raft Group + 全量复制"升级为"16384 个独立 Raft Group + 1 个 MetaRaft"的真正可横向扩展、存储成本 1/N 的强一致分布式数据库

**架构**: 保持 100% P2P 对等架构，零 Coordinator

**预计工时**: 一人全职 8~10 周，两人并行 4~6 周

**更新时间**: 2025-11-20

---

## 📋 目录

1. [整体架构](#整体架构)
2. [6 阶段实施计划](#6-阶段实施计划)
3. [关键数据结构](#关键数据结构)
4. [技术决策](#技术决策)
5. [参考项目](#参考项目)
6. [预期收益](#预期收益)
7. [风险评估](#风险评估)

---

## 🏗️ 整体架构

### 当前架构 (Phase 2 完成)

```
每个节点 (P2P 对等)
├── openraft::Raft (单一 Raft Group)
├── RaftStorage (LSM-Tree 存储)
├── RaftNetwork (gRPC 网络)
├── DB 实例 (单一数据库)
└── PeerNode (P2P 路由)
```

**限制**:
- 所有节点存储全量数据（无分片）
- 写放大 = N（N 为节点数）
- 无法横向扩展存储容量

### 目标架构 (Multi-Raft + Sharding)

```
每个节点（完全对等）
├── MetaRaft Group (Group 0)              ← 全局唯一，存集群元数据
│   ├── ClusterMeta (slot→group, group→replicas)
│   └── Config Version (版本控制)
│
├── DataRaft Groups (1~16384)             ← 每个 Group 负责若干 slot
│   ├── Group 1 → slots [0, 100)
│   ├── Group 2 → slots [100, 200)
│   └── ...
│
├── ShardedStateMachine                   ← HashMap<GroupId, AiDb>
│   ├── Group 1 → DB Instance 1
│   ├── Group 2 → DB Instance 2
│   └── ...
│
├── Router (slot → group → nodes)         ← 本地缓存 + MetaRaft watch
│   ├── SlotMap: [u64; 16384]
│   └── GroupMeta: HashMap<GroupId, GroupInfo>
│
└── gRPC Network                          ← 所有 Group 共用网络层
    ├── RaftService (per-group routing)
    └── ClientService (外部请求)
```

**优势**:
- 分片存储：每个节点仅存储部分数据
- 写放大：3~5 倍（副本数），而非节点数
- 横向扩展：添加节点线性增加容量
- 存储成本：总容量 / 副本数（1/N → 1/3）

---

## 📅 6 阶段实施计划

### 阶段 0: 当前准备 ✅ **已完成**

**目标**: 跑通现有单 Raft 集群

**状态**: ✅ 已完成（Phase 2 完成，openraft 集成）

**交付物**:
- [x] openraft 0.9 集成完成
- [x] RaftStorage 实现
- [x] RaftNetwork 实现
- [x] RaftNode 实现
- [x] 3 节点集群正常工作
- [x] 示例代码 (openraft_demo.rs)

---

### 阶段 1: MetaRaft 实现

**目标**: 实现全局元数据 Raft Group

**预计工时**: 1 周 | **难度**: ★★☆☆☆

#### 1.1 MetaRaft 数据结构 (Day 1-2)

- [ ] **定义 ClusterMeta 结构体**
  ```rust
  #[derive(Serialize, Deserialize, Default, Clone)]
  pub struct ClusterMeta {
      /// Slot to Group mapping (16384 slots)
      pub slots: [u64; 16384],
      
      /// Group metadata
      pub groups: HashMap<u64, GroupMeta>,
      
      /// Node information
      pub nodes: HashMap<NodeId, NodeInfo>,
      
      /// Configuration version (for CAS updates)
      pub config_version: u64,
  }
  
  #[derive(Serialize, Deserialize, Clone)]
  pub struct GroupMeta {
      pub group_id: u64,
      pub replicas: Vec<NodeId>,
      pub leader: Option<NodeId>,
      pub version: u64,
  }
  
  #[derive(Serialize, Deserialize, Clone)]
  pub struct NodeInfo {
      pub node_id: NodeId,
      pub addr: String,
      pub status: NodeStatus,
      pub joined_at: u64,
  }
  ```

- [ ] **定义 MetaRaft Request/Response**
  ```rust
  #[derive(Serialize, Deserialize, Clone)]
  pub enum MetaRequest {
      /// Add a new node
      AddNode { node_id: NodeId, addr: String },
      
      /// Remove a node
      RemoveNode { node_id: NodeId },
      
      /// Create a new group
      CreateGroup { group_id: u64, replicas: Vec<NodeId> },
      
      /// Update slot mapping
      UpdateSlots { start: u16, end: u16, group_id: u64 },
      
      /// Update group membership
      UpdateGroupMembers { group_id: u64, replicas: Vec<NodeId> },
  }
  
  #[derive(Serialize, Deserialize, Clone)]
  pub enum MetaResponse {
      Ok,
      Error(String),
      ClusterMeta(ClusterMeta),
  }
  ```

#### 1.2 MetaStateMachine 实现 (Day 2-3)

- [ ] **创建 src/cluster/meta_state_machine.rs**
  - [ ] 实现 StateMachine trait (基于现有 RaftStorage)
  - [ ] apply() 方法处理 MetaRequest
  - [ ] 持久化 ClusterMeta (JSON 或 bincode)
  - [ ] 提供查询接口 (get_cluster_meta)
  - [ ] 单元测试

#### 1.3 MetaRaft Node 实现 (Day 3-5)

- [ ] **创建 src/cluster/meta_raft_node.rs**
  - [ ] 基于 OpenRaftNode 创建 MetaRaftNode
  - [ ] 初始化 MetaRaft Group (Group 0)
  - [ ] 提供 MetaRaft API:
    - [ ] `add_node(node_id, addr)`
    - [ ] `remove_node(node_id)`
    - [ ] `create_group(group_id, replicas)`
    - [ ] `update_slots(start, end, group_id)`
    - [ ] `get_cluster_meta()`
  - [ ] 集成测试

#### 1.4 文档和测试 (Day 5-7)

- [ ] **单元测试**
  - [ ] MetaStateMachine 测试
  - [ ] ClusterMeta 序列化测试
  - [ ] MetaRaft API 测试

- [ ] **集成测试**
  - [ ] 3 节点 MetaRaft 集群测试
  - [ ] 元数据一致性测试
  - [ ] Leader 选举测试

- [ ] **文档**
  - [ ] MetaRaft 设计文档
  - [ ] API 使用示例

**交付物**:
- ✅ 可单独启动 MetaRaft 集群
- ✅ 手动变更元数据成功
- ✅ 测试通过率 100%

---

### 阶段 2: Multi-Raft 框架

**目标**: 支持动态创建 N 个 Raft Group

**预计工时**: 1.5 周 | **难度**: ★★★☆☆

#### 2.1 Multi-Raft 存储层 (Day 1-3)

- [ ] **创建 ShardedRaftStorage**
  ```rust
  pub struct ShardedRaftStorage {
      /// Per-group storage
      groups: Arc<RwLock<HashMap<GroupId, Arc<RaftStorage>>>>,
      
      /// Base directory
      base_dir: PathBuf,
  }
  
  impl ShardedRaftStorage {
      /// Create storage for a new group
      pub fn create_group(&self, group_id: GroupId) -> Result<()>;
      
      /// Get storage for a group
      pub fn get_group(&self, group_id: GroupId) -> Option<Arc<RaftStorage>>;
      
      /// Remove group storage
      pub fn remove_group(&self, group_id: GroupId) -> Result<()>;
  }
  ```

- [ ] **实现 src/cluster/sharded_storage.rs**
  - [ ] 目录结构: `./data/groups/{group_id}/`
  - [ ] 动态创建/删除 group storage
  - [ ] 单元测试

#### 2.2 Multi-Raft Network 层 (Day 3-5)

- [ ] **扩展 RaftNetwork 支持 Multi-Group**
  - [ ] RPC 消息添加 `group_id` 字段
  - [ ] 路由层根据 `group_id` 分发到对应 Raft 实例
  - [ ] 更新 proto/raft.proto:
    ```protobuf
    message AppendEntriesRequest {
        uint64 group_id = 1;  // 新增
        // ... 其他字段
    }
    ```

- [ ] **实现 src/cluster/multi_raft_network.rs**
  - [ ] 维护 `groups: HashMap<GroupId, Arc<Raft>>`
  - [ ] 根据 group_id 路由请求
  - [ ] 单元测试

#### 2.3 Multi-Raft Node Manager (Day 5-8)

- [ ] **创建 src/cluster/multi_raft_node.rs**
  ```rust
  pub struct MultiRaftNode {
      /// Node ID
      node_id: NodeId,
      
      /// MetaRaft instance
      meta_raft: Arc<MetaRaftNode>,
      
      /// Data Raft groups
      groups: Arc<RwLock<HashMap<GroupId, Arc<Raft>>>>,
      
      /// Sharded storage
      storage: Arc<ShardedRaftStorage>,
      
      /// Network factory
      network: Arc<MultiRaftNetwork>,
  }
  ```

- [ ] **实现核心方法**
  - [ ] `create_raft_group(group_id, replicas)` - 动态创建 Group
  - [ ] `remove_raft_group(group_id)` - 删除 Group
  - [ ] `get_raft_group(group_id)` - 获取 Group 实例
  - [ ] 单元测试

#### 2.4 集成测试 (Day 8-10)

- [ ] **测试动态创建 Raft Group**
  - [ ] 创建 100 个空 Raft Group
  - [ ] 验证选举正常
  - [ ] 验证日志复制

- [ ] **测试 Group 管理**
  - [ ] 动态添加 Group
  - [ ] 动态删除 Group
  - [ ] 并发操作测试

- [ ] **文档**
  - [ ] Multi-Raft 架构文档
  - [ ] 使用示例

**交付物**:
- ✅ 可手动创建 100 个空 Raft Group
- ✅ 选举和日志复制正常
- ✅ 测试通过率 100%

---

### 阶段 3: 分片路由 + Sharded AiDb

**目标**: 每个 Group 拥有独立 AiDb 实例，实现分片路由

**预计工时**: 2 周 | **难度**: ★★★★☆

#### 3.1 Slot 计算和路由 (Day 1-3)

- [ ] **实现 Slot 计算**
  ```rust
  pub fn key_to_slot(key: &[u8]) -> u16 {
      crc16(key) % 16384
  }
  
  pub fn slot_to_group(slot: u16, meta: &ClusterMeta) -> u64 {
      meta.slots[slot as usize]
  }
  ```

- [ ] **创建 Router**
  ```rust
  pub struct Router {
      /// Local cache of cluster metadata
      meta_cache: Arc<RwLock<ClusterMeta>>,
      
      /// MetaRaft client (for updates)
      meta_client: Arc<MetaRaftNode>,
  }
  
  impl Router {
      /// Route key to group
      pub fn route(&self, key: &[u8]) -> Result<GroupId>;
      
      /// Route key to nodes (group replicas)
      pub fn route_to_nodes(&self, key: &[u8]) -> Result<Vec<NodeId>>;
      
      /// Watch MetaRaft for updates
      pub async fn watch_metadata_changes(&self);
  }
  ```

- [ ] **实现 src/cluster/router.rs**
  - [ ] key → slot → group_id 映射
  - [ ] 本地元数据缓存
  - [ ] Watch MetaRaft 变更
  - [ ] 单元测试

#### 3.2 Sharded StateMachine (Day 3-7)

- [ ] **创建 ShardedStateMachine**
  ```rust
  pub struct ShardedStateMachine {
      /// Per-group DB instances
      dbs: Arc<RwLock<HashMap<GroupId, Arc<DB>>>>,
      
      /// Base directory
      base_dir: PathBuf,
      
      /// Router
      router: Arc<Router>,
  }
  
  impl ShardedStateMachine {
      /// Create DB for a group
      pub fn create_db(&mut self, group_id: GroupId) -> Result<()>;
      
      /// Apply entry to correct DB
      pub fn apply(&mut self, group_id: GroupId, entry: &Entry) -> Result<()>;
      
      /// Get from correct DB
      pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
  }
  ```

- [ ] **实现 src/cluster/sharded_state_machine.rs**
  - [ ] HashMap<GroupId, DB> 管理
  - [ ] apply() 路由到正确的 DB
  - [ ] 目录结构: `./data/groups/{group_id}/db/`
  - [ ] 单元测试

#### 3.3 集成 Multi-Raft + ShardedStateMachine (Day 7-12)

- [ ] **更新 MultiRaftNode**
  - [ ] 集成 ShardedStateMachine
  - [ ] 实现 `put(key, value)` - 自动路由到正确 Group
  - [ ] 实现 `get(key)` - 从正确 Group 读取
  - [ ] 实现 `delete(key)`

- [ ] **实现客户端 API**
  ```rust
  impl MultiRaftNode {
      pub async fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
          let slot = key_to_slot(&key);
          let group_id = self.router.route(&key)?;
          let raft = self.get_raft_group(group_id)?;
          
          let request = Request::Put { key, value };
          raft.client_write(request).await?;
          Ok(())
      }
      
      pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
          let group_id = self.router.route(key)?;
          self.state_machine.get(key)
      }
  }
  ```

#### 3.4 测试和验证 (Day 12-14)

- [ ] **单元测试**
  - [ ] Slot 计算测试
  - [ ] Router 测试
  - [ ] ShardedStateMachine 测试

- [ ] **集成测试**
  - [ ] 写入任意 key，验证落到正确 Group
  - [ ] 多 key 并发写入
  - [ ] 跨 Group 读写测试
  - [ ] 数据一致性验证

- [ ] **文档**
  - [ ] 分片路由设计文档
  - [ ] 使用示例

**交付物**:
- ✅ 写任意 key 自动落到对应 Group 的 AiDb
- ✅ 多 Group 并发工作正常
- ✅ 测试通过率 100%

---

### 阶段 4: 动态成员管理 + 副本分配

**目标**: 节点加入时自动分配副本，支持动态成员变更

**预计工时**: 1.5 周 | **难度**: ★★★★☆

#### 4.1 节点加入流程 (Day 1-3)

- [ ] **实现节点启动流程**
  ```rust
  impl MultiRaftNode {
      pub async fn start(&self) -> Result<()> {
          // 1. 启动 gRPC 服务器
          self.start_grpc_server().await?;
          
          // 2. 加入 MetaRaft（如果不是初始节点）
          if !self.is_bootstrap_node {
              self.join_meta_raft().await?;
          }
          
          // 3. 从 MetaRaft 获取集群元数据
          let meta = self.meta_raft.get_cluster_meta().await?;
          
          // 4. 加载应属于本节点的 Groups
          self.load_groups(&meta).await?;
          
          Ok(())
      }
  }
  ```

- [ ] **实现 join_meta_raft()**
  - [ ] 连接到现有 MetaRaft Leader
  - [ ] 添加自己为 Learner
  - [ ] 等待晋升为 Voter

#### 4.2 副本分配算法 (Day 3-6)

- [ ] **实现负载均衡算法**
  ```rust
  pub struct ReplicaAllocator {
      /// Target replication factor
      replication_factor: usize,
  }
  
  impl ReplicaAllocator {
      /// Allocate replicas for a new group
      pub fn allocate_replicas(
          &self,
          group_id: GroupId,
          available_nodes: &[NodeId],
          current_allocation: &HashMap<GroupId, Vec<NodeId>>,
      ) -> Vec<NodeId>;
      
      /// Rebalance replicas when node joins/leaves
      pub fn rebalance(
          &self,
          nodes: &[NodeId],
          current_allocation: HashMap<GroupId, Vec<NodeId>>,
      ) -> HashMap<GroupId, Vec<NodeId>>;
  }
  ```

- [ ] **实现 src/cluster/replica_allocator.rs**
  - [ ] 最小化副本不均衡
  - [ ] 考虑节点负载（已有 Group 数）
  - [ ] 单元测试

#### 4.3 动态成员变更 (Day 6-9)

- [ ] **实现 AddNode 处理**
  ```rust
  impl MetaStateMachine {
      fn handle_add_node(&mut self, node_id: NodeId, addr: String) -> Result<()> {
          // 1. 添加节点到 nodes
          self.meta.nodes.insert(node_id, NodeInfo { ... });
          
          // 2. 分配副本
          let allocator = ReplicaAllocator::new(self.replication_factor);
          let assignments = allocator.rebalance(&self.meta.nodes, &self.meta.groups);
          
          // 3. 更新 groups
          for (group_id, replicas) in assignments {
              self.meta.groups.get_mut(&group_id).unwrap().replicas = replicas;
          }
          
          // 4. 触发各 Group 的 change_membership
          self.pending_membership_changes.push(...);
          
          Ok(())
      }
  }
  ```

- [ ] **实现 Group Membership 变更**
  - [ ] MetaRaft propose membership change
  - [ ] 各节点监听 MetaRaft 变更
  - [ ] 自动调用 `raft.change_membership()`
  - [ ] 支持 Joint Consensus（零宕机）

#### 4.4 测试和验证 (Day 9-10)

- [ ] **集成测试**
  - [ ] 新节点一键加入
  - [ ] 副本自动分配
  - [ ] 成员变更正常
  - [ ] 数据自动复制

- [ ] **文档**
  - [ ] 成员管理设计文档
  - [ ] 操作手册

**交付物**:
- ✅ 新节点一键加入，数据自动平衡
- ✅ 支持 Joint Consensus（零宕机）
- ✅ 测试通过率 100%

---

### 阶段 5: 在线 Slot 迁移 (Resharding)

**目标**: 支持 CLUSTER SETSLOT MIGRATING/IMPORTING，实现在线迁移

**预计工时**: 2 周 | **难度**: ★★★★★

#### 5.1 迁移协议设计 (Day 1-3)

- [ ] **定义迁移状态**
  ```rust
  #[derive(Clone, Debug)]
  pub enum SlotMigrationState {
      Idle,
      Migrating { from_group: GroupId, to_group: GroupId },
      Importing { from_group: GroupId, to_group: GroupId },
      Complete,
  }
  
  pub struct SlotMigration {
      pub slot: u16,
      pub state: SlotMigrationState,
      pub progress: u64,  // migrated keys count
      pub total: u64,     // total keys count
      pub started_at: u64,
  }
  ```

- [ ] **设计迁移流程**
  1. MetaRaft 发起迁移: `MIGRATE slot from_group to_group`
  2. 目标 Group 进入 IMPORTING 状态
  3. 源 Group 进入 MIGRATING 状态
  4. 批量迁移 keys: `GET → MIGRATE → DEL`
  5. 迁移期间双写（源 + 目标）
  6. 迁移完成: 更新 MetaRaft slot mapping
  7. 清理源 Group 数据

#### 5.2 Key 级别迁移 (Day 3-8)

- [ ] **实现 MIGRATE 命令**
  ```rust
  impl MultiRaftNode {
      /// Migrate a single key
      pub async fn migrate_key(
          &self,
          key: Vec<u8>,
          from_group: GroupId,
          to_group: GroupId,
      ) -> Result<()> {
          // 1. Read from source
          let value = self.get_from_group(from_group, &key).await?;
          
          // 2. Write to target
          if let Some(value) = value {
              self.put_to_group(to_group, key.clone(), value).await?;
          }
          
          // 3. Delete from source (after confirmation)
          self.delete_from_group(from_group, key).await?;
          
          Ok(())
      }
      
      /// Migrate all keys in a slot
      pub async fn migrate_slot(
          &self,
          slot: u16,
          from_group: GroupId,
          to_group: GroupId,
      ) -> Result<()> {
          // Batch migrate keys
          let keys = self.scan_slot_keys(from_group, slot).await?;
          
          for key in keys {
              self.migrate_key(key, from_group, to_group).await?;
          }
          
          Ok(())
      }
  }
  ```

- [ ] **实现批量迁移**
  - [ ] 支持批量 GET/PUT/DELETE
  - [ ] 限流和进度控制
  - [ ] 错误重试

#### 5.3 双写和捉补 (Day 8-11)

- [ ] **实现双写逻辑**
  ```rust
  impl MultiRaftNode {
      pub async fn put_with_migration_aware(
          &self,
          key: Vec<u8>,
          value: Vec<u8>,
      ) -> Result<()> {
          let slot = key_to_slot(&key);
          
          // Check if slot is migrating
          if let Some(migration) = self.get_slot_migration(slot) {
              match migration.state {
                  SlotMigrationState::Migrating { from_group, to_group } => {
                      // Write to both groups
                      self.put_to_group(from_group, key.clone(), value.clone()).await?;
                      self.put_to_group(to_group, key, value).await?;
                  }
                  _ => {
                      // Normal write
                      let group_id = self.router.route(&key)?;
                      self.put_to_group(group_id, key, value).await?;
                  }
              }
          } else {
              let group_id = self.router.route(&key)?;
              self.put_to_group(group_id, key, value).await?;
          }
          
          Ok(())
      }
  }
  ```

- [ ] **实现异步捉补**
  - [ ] 后台扫描迁移差异
  - [ ] 补齐未迁移的 keys

#### 5.4 元数据更新 (Day 11-12)

- [ ] **更新 MetaRaft slot mapping**
  ```rust
  impl MetaStateMachine {
      fn handle_complete_migration(
          &mut self,
          slot: u16,
          from_group: GroupId,
          to_group: GroupId,
      ) -> Result<()> {
          // Update slot mapping
          self.meta.slots[slot as usize] = to_group;
          
          // Update config version
          self.meta.config_version += 1;
          
          Ok(())
      }
  }
  ```

#### 5.5 测试和验证 (Day 12-14)

- [ ] **集成测试**
  - [ ] 完整迁移流程
  - [ ] 迁移期间读写正确性
  - [ ] 双写验证
  - [ ] 故障恢复测试

- [ ] **压力测试**
  - [ ] 大量 keys 迁移
  - [ ] 迁移期间高并发写入
  - [ ] 迁移性能测试

- [ ] **文档**
  - [ ] 迁移设计文档
  - [ ] 操作手册
  - [ ] 故障排查指南

**交付物**:
- ✅ 支持手动/自动 rebalance
- ✅ 零停机迁移
- ✅ 测试通过率 100%

---

### 阶段 6: 优化 + 生产就绪

**目标**: 性能优化、监控、持久化、配置

**预计工时**: 1~2 周 | **难度**: ★★★☆☆

#### 6.1 存储优化 (Day 1-3)

- [ ] **Group 本地快照独立**
  - [ ] 每个 Group 独立快照
  - [ ] 快照压缩
  - [ ] 快照传输优化

- [ ] **Raft Log 清理策略**
  - [ ] 定期 purge logs
  - [ ] 配置保留策略
  - [ ] 监控 log 大小

#### 6.2 性能优化 (Day 3-6)

- [ ] **批量操作优化**
  - [ ] WriteBatch 支持
  - [ ] Batch proposal
  - [ ] Pipeline 优化

- [ ] **缓存优化**
  - [ ] 元数据缓存
  - [ ] Router 缓存
  - [ ] Group Leader 缓存

- [ ] **并发优化**
  - [ ] 减少锁竞争
  - [ ] 异步 I/O
  - [ ] 线程池优化

#### 6.3 监控和指标 (Day 6-9)

- [ ] **Prometheus 指标**
  - [ ] Per-group latency
  - [ ] Per-group QPS
  - [ ] Replication lag
  - [ ] Migration progress
  - [ ] Group count
  - [ ] Slot distribution

- [ ] **Grafana 仪表盘**
  - [ ] Multi-Raft 概览面板
  - [ ] 分片状态面板
  - [ ] 迁移监控面板

#### 6.4 配置和工具 (Day 9-12)

- [ ] **配置项**
  ```rust
  pub struct MultiRaftConfig {
      /// Number of groups (default: 16384)
      pub group_count: u16,
      
      /// Replication factor (default: 3)
      pub replication_factor: usize,
      
      /// Raft election timeout
      pub election_timeout_ms: u64,
      
      /// Raft heartbeat interval
      pub heartbeat_interval_ms: u64,
      
      /// Migration batch size
      pub migration_batch_size: usize,
      
      /// Migration rate limit (keys/sec)
      pub migration_rate_limit: u64,
  }
  ```

- [ ] **aidb-admin 命令扩展**
  - [ ] `aidb-admin cluster status` - 显示 Multi-Raft 状态
  - [ ] `aidb-admin group list` - 列出所有 Groups
  - [ ] `aidb-admin group info <id>` - Group 详情
  - [ ] `aidb-admin migrate start <slot> <to_group>` - 启动迁移
  - [ ] `aidb-admin migrate status` - 迁移进度

#### 6.5 文档和测试 (Day 12-14)

- [ ] **完整文档**
  - [ ] Multi-Raft 架构文档
  - [ ] 部署指南
  - [ ] 运维手册
  - [ ] 性能调优指南
  - [ ] FAQ

- [ ] **端到端测试**
  - [ ] 完整集群启动测试
  - [ ] 大规模数据测试（10亿+ keys）
  - [ ] 长时间运行测试（24小时+）
  - [ ] 故障注入测试

**交付物**:
- ✅ 生产可用版
- ✅ 完整监控和运维工具
- ✅ 文档齐全
- ✅ 测试通过率 100%

---

## 📊 关键数据结构

### ClusterMeta (MetaRaft StateMachine)

```rust
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ClusterMeta {
    /// Slot to Group mapping (16384 slots)
    /// slots[i] = group_id that owns slot i
    pub slots: [u64; 16384],
    
    /// Group metadata
    pub groups: HashMap<u64, GroupMeta>,
    
    /// Node information
    pub nodes: HashMap<NodeId, NodeInfo>,
    
    /// Configuration version (for CAS updates)
    pub config_version: u64,
    
    /// Ongoing migrations
    pub migrations: Vec<SlotMigration>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GroupMeta {
    pub group_id: u64,
    pub replicas: Vec<NodeId>,
    pub leader: Option<NodeId>,
    pub version: u64,
    pub slot_range: (u16, u16),  // [start, end)
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NodeInfo {
    pub node_id: NodeId,
    pub addr: String,
    pub status: NodeStatus,
    pub joined_at: u64,
    pub group_count: usize,  // Number of groups on this node
}

#[derive(Serialize, Deserialize, Clone)]
pub enum NodeStatus {
    Online,
    Offline,
    Joining,
    Leaving,
}
```

### DataGroup (Sharded DB)

```rust
pub struct DataGroup {
    /// Group ID
    pub group_id: GroupId,
    
    /// AiDb instance (独立 LSM-Tree)
    pub db: Arc<DB>,
    
    /// Data directory
    pub path: PathBuf,
    
    /// Raft instance
    pub raft: Arc<Raft<TypeConfig>>,
}
```

### ShardedStateMachine

```rust
pub struct ShardedStateMachine {
    /// MetaRaft metadata (从 MetaRaft 同步)
    pub meta: Arc<RwLock<ClusterMeta>>,
    
    /// Per-group DB instances
    pub groups: HashMap<GroupId, DataGroup>,
    
    /// Base directory
    pub base_dir: PathBuf,
}
```

### Router

```rust
pub struct Router {
    /// Local cache of cluster metadata
    meta_cache: Arc<RwLock<ClusterMeta>>,
    
    /// MetaRaft client
    meta_client: Arc<MetaRaftNode>,
    
    /// Cache version
    cached_version: AtomicU64,
}

impl Router {
    /// key → slot
    pub fn key_to_slot(key: &[u8]) -> u16 {
        crc16(key) % 16384
    }
    
    /// slot → group_id
    pub fn slot_to_group(&self, slot: u16) -> Result<GroupId> {
        let meta = self.meta_cache.read().unwrap();
        Ok(meta.slots[slot as usize])
    }
    
    /// key → group_id
    pub fn route(&self, key: &[u8]) -> Result<GroupId> {
        let slot = Self::key_to_slot(key);
        self.slot_to_group(slot)
    }
    
    /// Watch MetaRaft for updates
    pub async fn watch_metadata_changes(&self) {
        loop {
            let new_meta = self.meta_client.get_cluster_meta().await?;
            
            let mut cache = self.meta_cache.write().unwrap();
            if new_meta.config_version > cache.config_version {
                *cache = new_meta;
                self.cached_version.store(
                    cache.config_version,
                    Ordering::Release
                );
            }
            
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
```

---

## 🎯 技术决策

### 1. 为什么 16384 个 Slots？

- **Redis Cluster 兼容**: 未来 AiKv 可无缝对接
- **合理粒度**: 
  - 太少（如 1024）：迁移粒度太粗，负载不均
  - 太多（如 65536）：元数据开销大，路由慢
- **CRC16 输出**: 0~65535，取模 16384 分布均匀

### 2. Slot → Group 映射策略

**初始分配**:
```rust
// 均匀分配 16384 个 slots 到 N 个 groups
for slot in 0..16384 {
    let group_id = (slot % group_count) as u64;
    slots[slot] = group_id;
}
```

**迁移时**:
- 仅更新单个 slot 的映射
- MetaRaft propose: `UpdateSlots { start: 100, end: 101, group_id: 42 }`
- 所有节点通过 watch 获取更新

### 3. 副本数 (Replication Factor)

- **默认 3 副本**: 平衡可用性和成本
- **可配置 5 副本**: 高可用场景
- **最少 3 副本**: 保证 Raft 正常工作 (2f+1)

### 4. Group 数量选择

- **默认 16384 Groups**: 1 Group per 1 slot（最细粒度）
- **可配置更少**: 如 1024 Groups (每个 Group 负责 16 个 slots)
  - 优点: 减少 Raft 实例数，降低开销
  - 缺点: 迁移粒度变粗
- **推荐**: 根据集群规模调整
  - 小集群（< 10 节点）: 1024 Groups
  - 中等集群（10~100 节点）: 4096 Groups
  - 大集群（100+ 节点）: 16384 Groups

### 5. MetaRaft vs 每个节点独立元数据

**为什么需要 MetaRaft**:
- **强一致性**: 所有节点看到相同的 slot 映射
- **原子更新**: 成员变更、迁移等操作原子执行
- **简化同步**: 节点启动时从 MetaRaft 获取最新元数据

**MetaRaft 开销**:
- 仅存储元数据（KB 级），读写频率低
- 不影响数据读写性能（本地缓存）

### 6. 迁移策略：Push vs Pull

**选择 Push 模式** (源 Group 主动推送):
- 源 Group 扫描待迁移 keys
- 批量发送到目标 Group
- 目标 Group 确认后，源 Group 删除

**优点**:
- 源 Group 控制迁移进度
- 限流和错误处理简单
- 兼容 Redis Cluster 协议

### 7. 双写窗口

**迁移期间**:
- 客户端写入 → 同时写入源和目标 Group
- 读取 → 优先从目标 Group 读，miss 则从源 Group 读

**窗口关闭**:
- 所有 keys 迁移完成
- MetaRaft 更新 slot mapping
- 客户端更新本地缓存

### 8. 错误处理

**网络分区**:
- MetaRaft 通过 Raft 保证一致性
- 数据 Raft Groups 独立处理
- 客户端重试 + 重路由

**节点故障**:
- Raft 自动 Leader 选举
- 副本自动接管
- MetaRaft 标记节点 Offline

**迁移中断**:
- 记录迁移进度（已迁移 keys 数）
- 重启后继续迁移
- 双写确保数据不丢

---

## 📚 参考项目

| 项目 | 可参考部分 | 链接 |
|------|----------|------|
| **rdb** | 几乎整个 Multi-Raft 架构（Rust + openraft + Redis 协议） | https://github.com/MoSunDay/rdb |
| **tikv/raft-rs** | multi-raft 示例代码 | https://github.com/tikv/raft-rs/tree/master/examples/multi_raft |
| **Garnet** | 分片 + 元数据管理（C# 但思路一致） | https://github.com/microsoft/garnet |
| **DragonflyDB** | Region 迁移逻辑 | https://github.com/dragonflydb/dragonfly |
| **TiKV** | Multi-Raft 生产实践 | https://github.com/tikv/tikv |
| **CockroachDB** | Range 迁移和分裂 | https://github.com/cockroachdb/cockroach |

**直接可抄**:
1. **rdb**: Rust + openraft + Multi-Raft，架构几乎完全匹配
2. **tikv/raft-rs examples/multi_raft**: 基础框架代码
3. **TiKV PD (Placement Driver)**: MetaRaft 的最佳实践

---

## 🎁 预期收益

### 存储容量

**当前架构** (单 Raft Group):
- 10 节点 × 1TB = 1TB 可用存储（全量复制）
- 100 节点 × 1TB = 1TB 可用存储

**Multi-Raft 架构** (3 副本):
- 10 节点 × 1TB = ~3.3TB 可用存储
- 100 节点 × 1TB = ~33TB 可用存储
- **扩展比例**: 接近线性 (1/N → 1/3)

### 写放大

**当前架构**:
- 写放大 = N (所有节点写入)
- 10 节点: 10 倍写放大
- 100 节点: 100 倍写放大

**Multi-Raft 架构**:
- 写放大 = 副本数 (3~5)
- 10 节点: 3 倍写放大
- 100 节点: 3 倍写放大
- **降低**: 70~98% (N=10~100)

### 延迟

**当前架构**:
- Leader 写入 → 所有节点确认 (N/2+1)
- 延迟随节点数增加

**Multi-Raft 架构**:
- Leader 写入 → 对应 Group 的副本确认 (3/2+1 = 2)
- 延迟固定 < 1ms（局域网）
- **优化**: 与节点数无关

### 吞吐量

**当前架构**:
- 单 Leader 瓶颈
- 吞吐量 ~10K ops/s

**Multi-Raft 架构**:
- 16384 个独立 Leader
- 每个 Group ~10K ops/s
- 总吞吐量 = 10K × 16384 = **163M ops/s** (理论值)
- 实际受网络和磁盘限制，可达 **100K~1M ops/s**

### 可用性

**当前架构**:
- Leader 故障 → 整个集群不可写
- 恢复时间: 数秒

**Multi-Raft 架构**:
- 单个 Group Leader 故障 → 仅影响该 Group (1/16384 数据)
- 其他 Groups 正常服务
- **可用性**: 99.999% → 99.9999%

---

## ⚠️ 风险评估

### 高风险

1. **复杂度激增**
   - 从单 Raft → 16384 Raft
   - 调试和运维难度大幅增加
   - **缓解**: 充分测试、完善监控、详细文档

2. **迁移正确性**
   - 双写、捉补、元数据更新必须原子
   - **缓解**: 事务保证、幂等性设计、回滚机制

3. **性能退化**
   - 过多 Raft 实例可能导致资源耗尽
   - **缓解**: 合理配置 Group 数量、资源隔离、性能测试

### 中风险

4. **MetaRaft 单点**
   - MetaRaft 故障 → 无法更新元数据（但不影响数据读写）
   - **缓解**: MetaRaft 3~5 副本、监控告警

5. **Group 数量选择**
   - 太少: 迁移粒度粗，负载不均
   - 太多: 资源开销大
   - **缓解**: 可配置、根据集群规模调整

### 低风险

6. **兼容性**
   - 当前 P2P 架构需调整
   - **缓解**: 渐进式迁移、保留兼容 API

7. **文档和学习曲线**
   - Multi-Raft 概念复杂
   - **缓解**: 详细文档、示例代码、逐步教程

---

## 📈 项目时间线

```
Week 1-2:    阶段 0 (已完成) + 阶段 1 (MetaRaft)
Week 3-4:    阶段 2 (Multi-Raft 框架)
Week 5-6:    阶段 3 (分片路由 + Sharded AiDb)
Week 7-8:    阶段 4 (动态成员管理)
Week 9-10:   阶段 5 (在线迁移)
Week 11-12:  阶段 6 (优化 + 生产就绪)
```

**关键里程碑**:
- **Week 2**: MetaRaft 可运行 ✓
- **Week 4**: 100 个 Group 正常工作 ✓
- **Week 6**: 分片写入正确 ✓
- **Week 8**: 动态加入节点成功 ✓
- **Week 10**: 迁移流程完整 ✓
- **Week 12**: 生产就绪 ✓

---

## 🚀 立即行动计划

### Phase 1: 准备阶段 (本周)

1. **创建开发分支**
   ```bash
   git checkout -b feature/multi-raft-sharding
   ```

2. **设计评审**
   - 团队评审本文档
   - 确认技术方案
   - 分配任务

3. **环境准备**
   - 准备测试集群（3~5 节点）
   - 配置监控
   - 准备性能测试工具

### Phase 2: 快速验证 (Week 1)

1. **搭建 MetaRaft 骨架**
   - 3 天完成基础框架
   - 跑通 3 节点 MetaRaft

2. **Multi-Raft 原型**
   - 4 天实现动态创建 Group
   - 验证 10 个 Group 正常工作

### Phase 3: 全力开发 (Week 2-10)

- 按阶段 1~5 执行
- 每周代码评审
- 持续集成测试

### Phase 4: 测试和优化 (Week 11-12)

- 端到端测试
- 性能测试
- 故障注入
- 文档完善

---

## 📝 总结

本计划将 AiDb 从"单 Raft Group + 全量复制"升级为"Multi-Raft + Sharding"架构，实现：

- ✅ **真正的横向扩展**: 容量随节点数线性增长
- ✅ **存储成本优化**: 从 N 倍降至副本数倍 (3~5)
- ✅ **写放大降低**: 从 N 倍降至 3~5 倍
- ✅ **高可用性**: 单点故障影响最小化
- ✅ **生产可用**: 完整的监控、运维工具

**关键成功因素**:
1. 充分测试（单元、集成、端到端）
2. 详细文档（设计、API、运维）
3. 完善监控（指标、告警）
4. 渐进式迁移（兼容旧版本）

**下一步**:
1. 团队评审本文档
2. 确认开始时间
3. 创建开发分支
4. 开始阶段 1 实现

---

*文档版本: v1.0*  
*最后更新: 2025-11-20*  
*作者: AiDb Team*
