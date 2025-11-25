# AiDb MultiRaftCluster API for Redis Cluster Protocol

**目标**: 为上层 AiKv 提供一个最薄的 Redis Cluster 协议胶水层，底层 99.9% 的工作由 AiDb 完成，上层只需将 Redis Cluster 命令 1:1 映射到 MultiRaftCluster API 调用。

**创建时间**: 2025-11-25

---

## 📋 目录

1. [背景与动机](#背景与动机)
2. [现有实现分析](#现有实现分析)
3. [API 设计方案](#api-设计方案)
4. [实现计划](#实现计划)
5. [测试策略](#测试策略)

---

## 🎯 背景与动机

### 需求

上层 AiKv 想要适配 Redis Cluster 等协议，希望：

1. **最薄胶水层**: 99.9% 的工作交给底层 AiDb
2. **语义 1:1 映射**: Redis Cluster 命令直接翻译为 MultiRaftCluster API 调用
3. **零运维负担**: 分片、迁移、副本管理全部自动化

### Redis Cluster 核心命令

以下是需要适配的 Redis Cluster 协议命令：

```
# 集群信息
CLUSTER INFO          → 集群状态摘要
CLUSTER NODES         → 节点列表和 slot 分配
CLUSTER SLOTS         → slot-节点映射
CLUSTER MYID          → 本节点 ID
CLUSTER KEYSLOT key   → key 对应的 slot

# 节点管理
CLUSTER MEET ip port  → 添加节点
CLUSTER FORGET node   → 移除节点

# Slot 管理
CLUSTER ADDSLOTS slot...       → 分配 slot 给当前节点
CLUSTER DELSLOTS slot...       → 从当前节点移除 slot
CLUSTER SETSLOT slot node      → 将 slot 分配给指定节点
CLUSTER SETSLOT slot MIGRATING → 开始迁移 slot (源节点)
CLUSTER SETSLOT slot IMPORTING → 开始导入 slot (目标节点)
CLUSTER GETKEYSINSLOT slot     → 获取 slot 中的 keys

# 成员管理
CLUSTER REPLICATE node         → 成为某节点的副本
CLUSTER FAILOVER               → 手动故障转移

# 数据迁移
MIGRATE host port key db timeout → 迁移单个 key
```

---

## 🔍 现有实现分析

### AiDb 已有的 MultiRaft 组件

| 组件 | 位置 | 功能 | 状态 |
|------|------|------|------|
| `MultiRaftNode` | `src/cluster/multi_raft_node.rs` | 管理多个 Raft Group | ✅ 完整 |
| `MetaRaftNode` | `src/cluster/meta_raft_node.rs` | 集群元数据管理 | ✅ 完整 |
| `Router` | `src/cluster/router.rs` | key→slot→group 路由 | ✅ 完整 |
| `MigrationManager` | `src/cluster/slot_migration.rs` | 在线 slot 迁移 | ✅ 完整 |
| `MembershipCoordinator` | `src/cluster/membership_coordinator.rs` | 成员变更协调 | ✅ 完整 |
| `ReplicaAllocator` | `src/cluster/replica_allocator.rs` | 副本负载均衡分配 | ✅ 完整 |
| `ClusterMeta` | `src/cluster/meta_types.rs` | 集群元数据结构 | ✅ 完整 |

### 已暴露的 API (src/cluster/mod.rs)

```rust
// 已导出
pub use multi_raft_node::MultiRaftNode;
pub use meta_raft_node::MetaRaftNode;
pub use router::{Router, SLOT_COUNT};
pub use slot_migration::{MigrationConfig, MigrationManager};
pub use membership_coordinator::MembershipCoordinator;
pub use replica_allocator::ReplicaAllocator;
pub use meta_types::{ClusterMeta, GroupMeta, NodeInfo, ...};
```

### 缺口分析

虽然底层组件完整，但缺少一个 **面向上层的统一 API 封装**，特别是：

1. **没有统一入口**: 上层需要分别操作 `MultiRaftNode`、`MetaRaftNode`、`Router` 等多个组件
2. **没有 Redis Cluster 语义映射**: 需要手动将 Redis 命令翻译为多个 API 调用
3. **缺少高层操作 API**: 如 `cluster_info()`、`cluster_nodes()`、`cluster_slots()` 等

---

## 🏗️ API 设计方案

### 方案一：创建 `MultiRaftCluster` 统一封装 (推荐)

创建一个高层封装类，提供 Redis Cluster 语义直接映射的 API：

```rust
/// 面向上层（如 AiKv）的统一 MultiRaft 集群管理 API
///
/// 提供与 Redis Cluster 命令 1:1 映射的接口，
/// 上层只需将 Redis 命令翻译为对应方法调用即可。
pub struct MultiRaftCluster {
    /// 内部 MultiRaftNode 实例
    node: Arc<MultiRaftNode>,
    
    /// MetaRaft 实例
    meta_raft: Arc<MetaRaftNode>,
    
    /// 路由器
    router: Arc<Router>,
    
    /// 迁移管理器
    migration_manager: Option<Arc<MigrationManager>>,
    
    /// 成员协调器
    membership_coordinator: Arc<MembershipCoordinator>,
}
```

### API 方法设计

#### 1. 集群信息 API (对应 CLUSTER INFO/NODES/SLOTS)

```rust
impl MultiRaftCluster {
    /// CLUSTER INFO - 返回集群状态摘要
    ///
    /// 返回格式与 Redis CLUSTER INFO 一致:
    /// - cluster_state: ok/fail
    /// - cluster_slots_assigned: 已分配的 slot 数
    /// - cluster_slots_ok: 正常的 slot 数
    /// - cluster_known_nodes: 已知节点数
    /// - cluster_size: 参与数据分片的主节点数
    pub fn cluster_info(&self) -> ClusterInfoResponse;
    
    /// CLUSTER NODES - 返回所有节点信息
    ///
    /// 返回格式:
    /// <node_id> <ip:port> <flags> <master_id> <ping_sent> <pong_recv> <config_epoch> <link_state> <slot> <slot> ...
    pub fn cluster_nodes(&self) -> Vec<ClusterNodeInfo>;
    
    /// CLUSTER SLOTS - 返回 slot 到节点的映射
    ///
    /// 返回: [(start_slot, end_slot, [master, replica1, replica2, ...])]
    pub fn cluster_slots(&self) -> Vec<SlotRange>;
    
    /// CLUSTER MYID - 返回当前节点 ID
    pub fn cluster_myid(&self) -> NodeId;
    
    /// CLUSTER KEYSLOT key - 计算 key 对应的 slot
    pub fn cluster_keyslot(&self, key: &[u8]) -> u16;
}
```

#### 2. 节点管理 API (对应 CLUSTER MEET/FORGET)

```rust
impl MultiRaftCluster {
    /// CLUSTER MEET - 添加节点到集群
    ///
    /// # Arguments
    /// * `addr` - 节点地址 "ip:port"
    ///
    /// # Returns
    /// * `Ok(node_id)` - 新加入节点的 ID
    pub async fn cluster_meet(&self, addr: &str) -> Result<NodeId>;
    
    /// CLUSTER FORGET - 从集群移除节点
    ///
    /// # Arguments
    /// * `node_id` - 要移除的节点 ID
    ///
    /// # Notes
    /// 此操作会触发副本重分配
    pub async fn cluster_forget(&self, node_id: NodeId) -> Result<()>;
}
```

#### 3. Slot 管理 API (对应 CLUSTER ADDSLOTS/DELSLOTS/SETSLOT)

```rust
impl MultiRaftCluster {
    /// CLUSTER ADDSLOTS - 将 slots 分配给当前节点
    ///
    /// # Arguments
    /// * `slots` - 要分配的 slot 列表
    pub async fn cluster_addslots(&self, slots: &[u16]) -> Result<()>;
    
    /// CLUSTER DELSLOTS - 从当前节点移除 slots
    ///
    /// # Arguments
    /// * `slots` - 要移除的 slot 列表
    pub async fn cluster_delslots(&self, slots: &[u16]) -> Result<()>;
    
    /// CLUSTER SETSLOT slot NODE node_id - 将 slot 分配给指定节点
    ///
    /// # Arguments
    /// * `slot` - slot 号
    /// * `node_id` - 目标节点 ID
    pub async fn cluster_setslot_node(&self, slot: u16, node_id: NodeId) -> Result<()>;
    
    /// CLUSTER SETSLOT slot MIGRATING node_id - 开始迁出 slot
    ///
    /// 在源节点上调用，标记 slot 正在迁移到目标节点
    pub async fn cluster_setslot_migrating(&self, slot: u16, target_node_id: NodeId) -> Result<()>;
    
    /// CLUSTER SETSLOT slot IMPORTING node_id - 开始导入 slot
    ///
    /// 在目标节点上调用，标记正在从源节点导入 slot
    pub async fn cluster_setslot_importing(&self, slot: u16, source_node_id: NodeId) -> Result<()>;
    
    /// CLUSTER GETKEYSINSLOT slot count - 获取 slot 中的 keys
    ///
    /// # Arguments
    /// * `slot` - slot 号
    /// * `count` - 最多返回多少个 key
    ///
    /// # Returns
    /// * 该 slot 中的 key 列表
    pub fn cluster_getkeysinslot(&self, slot: u16, count: usize) -> Result<Vec<Vec<u8>>>;
}
```

#### 4. 数据操作 API (带自动路由)

```rust
impl MultiRaftCluster {
    /// PUT - 写入键值对 (自动路由)
    ///
    /// 自动计算 key 所属的 slot 和 group，路由到正确的 Raft leader
    pub async fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<()>;
    
    /// GET - 读取键值 (自动路由)
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    
    /// DELETE - 删除键 (自动路由)
    pub async fn delete(&self, key: &[u8]) -> Result<()>;
    
    /// MIGRATE - 迁移单个 key 到目标节点
    ///
    /// # Arguments
    /// * `key` - 要迁移的 key
    /// * `target_addr` - 目标节点地址
    /// * `timeout_ms` - 超时时间
    pub async fn migrate_key(&self, key: &[u8], target_addr: &str, timeout_ms: u64) -> Result<()>;
}
```

#### 5. 副本管理 API (对应 CLUSTER REPLICATE/FAILOVER)

```rust
impl MultiRaftCluster {
    /// CLUSTER REPLICATE - 将当前节点设为指定节点的副本
    ///
    /// # Arguments
    /// * `master_node_id` - 主节点 ID
    pub async fn cluster_replicate(&self, master_node_id: NodeId) -> Result<()>;
    
    /// CLUSTER FAILOVER - 手动故障转移
    ///
    /// 将当前节点提升为主节点
    pub async fn cluster_failover(&self) -> Result<()>;
}
```

#### 6. 迁移控制 API

```rust
impl MultiRaftCluster {
    /// 开始批量 slot 迁移
    ///
    /// # Arguments
    /// * `slots` - 要迁移的 slot 列表
    /// * `target_group_id` - 目标 group ID
    pub async fn start_slot_migration(&self, slots: &[u16], target_group_id: GroupId) -> Result<()>;
    
    /// 获取迁移进度
    ///
    /// # Arguments
    /// * `slot` - slot 号
    ///
    /// # Returns
    /// 迁移进度信息，None 表示未在迁移
    pub fn get_migration_progress(&self, slot: u16) -> Option<MigrationProgress>;
    
    /// 取消迁移
    pub fn cancel_migration(&self, slot: u16) -> Result<()>;
    
    /// 获取所有活跃的迁移
    pub fn get_active_migrations(&self) -> Vec<MigrationProgress>;
}
```

### 方案二：直接文档化现有 API 映射

如果不想增加新的封装层，可以提供详细的映射文档：

| Redis Cluster 命令 | AiDb API 调用 |
|-------------------|--------------|
| `CLUSTER INFO` | `meta_raft.get_cluster_meta()` 后解析 |
| `CLUSTER NODES` | `meta_raft.get_cluster_meta().nodes` |
| `CLUSTER SLOTS` | `meta_raft.get_cluster_meta().slots` + `groups` |
| `CLUSTER KEYSLOT` | `Router::key_to_slot(key)` |
| `CLUSTER MEET` | `meta_raft.add_node(node_id, addr)` |
| `CLUSTER SETSLOT MIGRATING` | `migration_manager.start_migration(slot, from, to)` |
| `PUT/GET/DELETE` | `multi_raft_node.put/get/delete` |

---

## 📅 实现计划

### Phase 1: 创建 MultiRaftCluster 封装 (1-2 天)

- [ ] 创建 `src/cluster/multi_raft_cluster.rs`
- [ ] 实现 `MultiRaftCluster::new()` 构造函数
- [ ] 实现集群信息 API (`cluster_info`, `cluster_nodes`, `cluster_slots`, `cluster_myid`, `cluster_keyslot`)
- [ ] 添加单元测试

### Phase 2: 实现节点管理 API (1 天)

- [ ] 实现 `cluster_meet`
- [ ] 实现 `cluster_forget`
- [ ] 添加集成测试

### Phase 3: 实现 Slot 管理 API (1-2 天)

- [ ] 实现 `cluster_addslots`
- [ ] 实现 `cluster_delslots`
- [ ] 实现 `cluster_setslot_node`
- [ ] 实现 `cluster_setslot_migrating`
- [ ] 实现 `cluster_setslot_importing`
- [ ] 实现 `cluster_getkeysinslot`
- [ ] 添加测试

### Phase 4: 实现数据操作和副本管理 API (1 天)

- [ ] 实现带自动路由的 `put`, `get`, `delete`
- [ ] 实现 `migrate_key`
- [ ] 实现 `cluster_replicate`
- [ ] 实现 `cluster_failover`
- [ ] 添加测试

### Phase 5: 文档和示例 (1 天)

- [ ] 编写 API 文档
- [ ] 创建示例代码 `examples/cluster/redis_cluster_demo.rs`
- [ ] 更新 README

---

## 🧪 测试策略

### 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_cluster_keyslot() {
        // 验证与 Redis CRC16 一致
        assert_eq!(MultiRaftCluster::cluster_keyslot(b"user:1000"), 3572);
    }
    
    #[tokio::test]
    async fn test_cluster_info() {
        let cluster = create_test_cluster().await;
        let info = cluster.cluster_info();
        assert_eq!(info.cluster_state, "ok");
    }
    
    #[tokio::test]
    async fn test_cluster_meet() {
        let cluster = create_test_cluster().await;
        let node_id = cluster.cluster_meet("127.0.0.1:7001").await.unwrap();
        assert!(node_id > 0);
    }
}
```

### 集成测试

```rust
#[tokio::test]
async fn test_full_cluster_lifecycle() {
    // 1. 创建 3 节点集群
    // 2. 分配 slots
    // 3. 写入数据
    // 4. 添加第 4 个节点
    // 5. 迁移部分 slots 到新节点
    // 6. 验证数据完整性
    // 7. 移除节点
}
```

---

## 📝 总结

### 推荐方案

**方案一 (创建 MultiRaftCluster 封装)** 是推荐方案，因为：

1. **最薄胶水层**: 上层 AiKv 只需 `MultiRaftCluster` 一个入口
2. **语义直接映射**: API 名称与 Redis Cluster 命令一一对应
3. **隐藏复杂性**: 内部协调多个组件，上层无需关心
4. **易于扩展**: 未来添加新命令支持很容易

### 工时估计

- **总工时**: 5-7 天
- **代码量**: ~800-1000 行
- **测试**: ~500 行

### 下一步

1. 确认方案选择
2. 创建开发分支
3. 开始 Phase 1 实现

---

*文档版本: v1.0*  
*作者: AiDb Team*  
*最后更新: 2025-11-25*
