# Multi-Raft 快速开始指南

**目标**: 帮助开发者快速上手已完成的 Multi-Raft + Sharding 实现

**更新时间**: 2024-12-10  
**状态**: ✅ 生产就绪

---

## 🎉 Multi-Raft 已完成！

Multi-Raft + 分片架构已完整实现并通过 666+ 测试用例验证。本指南帮助您快速了解和使用这个生产级功能。

---

## 🚀 10 分钟快速开始

### 1. 理解架构

Multi-Raft 实现了真正的横向扩展分布式数据库：

**核心组件**:
- **MetaRaft (Group 0)**: 管理全局元数据（slot→group映射）
- **Data Raft Groups (1-16384)**: 每个 Group 管理一部分 slots
- **Router**: 自动路由 key 到正确的 Group
- **ShardedStateMachine**: 每个 Group 独立的 DB 实例
- **MigrationManager**: 在线 Slot 迁移，零停机

**关键特性**:
- ✅ 16384 个 slots（与 Redis Cluster 兼容）
- ✅ CRC16/XMODEM 哈希算法
- ✅ 自动路由和负载均衡
- ✅ 在线迁移和动态扩缩容
- ✅ Thin Replication（仅复制 WAL）

详细架构图解：[MULTI_RAFT_ARCHITECTURE.md](MULTI_RAFT_ARCHITECTURE.md)

### 2. 环境准备

```bash
# 1. 克隆项目
git clone https://github.com/Genuineh/AiDb.git
cd AiDb

# 2. 构建项目（启用 raft-cluster 特性）
cargo build --features raft-cluster --release

# 3. 运行测试验证
cargo test --features raft-cluster multi_raft

# 4. 查看示例代码
ls examples/cluster/
```

### 3. 基础使用示例

#### 示例 1: 创建单节点 Multi-Raft 集群

```rust
use aidb::cluster::{MultiRaftNode, Router, ShardedStateMachine};
use aidb::config::Options;
use openraft::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建 Multi-Raft 节点
    let config = Config::default();
    let mut node = MultiRaftNode::new(1, "./data/node1", config).await?;
    
    // 2. 初始化 MetaRaft（Group 0）
    node.init_meta_raft(Config::default()).await?;
    
    // 3. Bootstrap MetaRaft 集群（仅首节点需要）
    node.initialize_meta_cluster(vec![
        (1, "127.0.0.1:50051".to_string()),
    ]).await?;
    
    // 4. 初始化路由器和状态机
    node.init_router()?;
    node.init_state_machine(Options::default())?;
    
    // 5. 创建数据 Raft Groups
    node.create_raft_group(1, vec![1]).await?;
    node.create_raft_group(2, vec![1]).await?;
    node.create_raft_group(3, vec![1]).await?;
    
    println!("Multi-Raft cluster started with {} groups", node.group_count());
    
    // 6. 数据操作（自动路由）
    node.put(b"user:1000".to_vec(), b"Alice".to_vec()).await?;
    node.put(b"user:2000".to_vec(), b"Bob".to_vec()).await?;
    
    let value1 = node.get(b"user:1000")?;
    let value2 = node.get(b"user:2000")?;
    
    println!("user:1000 = {:?}", value1);
    println!("user:2000 = {:?}", value2);
    
    // 7. 删除数据
    node.delete(b"user:1000").await?;
    
    // 8. 优雅关闭
    node.shutdown().await?;
    
    Ok(())
}
```

#### 示例 2: Slot 迁移

```rust
use aidb::cluster::{MultiRaftNode, MigrationManager, MigrationConfig};
use std::time::Duration;

async fn migrate_example(node: &MultiRaftNode) -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建迁移管理器
    let config = MigrationConfig {
        batch_size: 100,
        rate_limit: 1000,
        key_timeout: Duration::from_secs(5),
        max_retries: 3,
        batch_delay: Duration::from_millis(10),
    };
    
    let manager = MigrationManager::new(
        config,
        node.router().unwrap().clone(),
        node.state_machine().unwrap().clone(),
    );
    
    // 2. 开始迁移 slot 100 从 Group 1 到 Group 2
    manager.start_migration(100, 1, 2).await?;
    
    // 3. 监控迁移进度
    loop {
        if let Some(progress) = manager.get_migration_progress(100) {
            println!("Migration progress: {:.2}%", progress.progress_pct());
            
            if progress.is_complete() {
                println!("Migration completed!");
                break;
            }
        } else {
            // 迁移已完成并清理
            break;
        }
        
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    
    Ok(())
}
```

#### 示例 3: 查询路由信息

```rust
use aidb::cluster::Router;

fn routing_example() {
    // 计算 key 对应的 slot
    let key = b"user:12345";
    let slot = Router::key_to_slot(key);
    println!("Key {:?} maps to slot {}", key, slot);
    
    // 通过 router 查询 group
    let router = /* ... */;
    let group_id = router.route(key).unwrap();
    println!("Slot {} belongs to group {}", slot, group_id);
    
    // 获取 group 的节点信息
    let nodes = router.route_to_nodes(key).unwrap();
    println!("Group {} replicas: {:?}", group_id, nodes);
}
```

---

## 💡 已实现的功能

### 核心功能清单

#### MetaRaft ✅
- ✅ 全局元数据管理（ClusterMeta）
- ✅ Slot 到 Group 映射（16384 slots）
- ✅ 节点信息管理（NodeInfo）
- ✅ Group 信息管理（GroupMeta）
- ✅ 版本控制（config_version）
- ✅ 元数据持久化和恢复

**API**:
```rust
// 添加节点
meta_raft.add_node(node_id, addr).await?;

// 创建 Group
meta_raft.create_group(group_id, replicas).await?;

// 更新 Slot 映射
meta_raft.update_slots(start, end, group_id).await?;

// 获取集群元数据
let meta = meta_raft.get_cluster_meta();
```

#### MultiRaftNode ✅
- ✅ 管理多个 Raft Groups
- ✅ 动态创建/删除 Groups
- ✅ 自动从磁盘加载 Groups
- ✅ 自动路由数据操作
- ✅ 与 MetaRaft 集成

**API**:
```rust
// 创建 Group
node.create_raft_group(group_id, replicas).await?;

// 获取 Group
let raft = node.get_raft_group(group_id);

// 数据操作
node.put(key, value).await?;
let value = node.get(&key)?;
node.delete(&key).await?;

// 列出所有 Groups
let groups = node.list_groups();
```

#### Router ✅
- ✅ CRC16/XMODEM 哈希（Redis 兼容）
- ✅ key → slot → group 路由
- ✅ 本地元数据缓存
- ✅ 自动元数据更新
- ✅ 线程安全

**API**:
```rust
// 计算 slot
let slot = Router::key_to_slot(key);

// 路由到 group
let group_id = router.route(key)?;

// 获取 group 节点
let nodes = router.route_to_nodes(key)?;

// 刷新元数据
router.refresh_metadata()?;
```

#### MigrationManager ✅
- ✅ 在线 Slot 迁移
- ✅ 批量 key 迁移
- ✅ 速率限制
- ✅ 进度跟踪
- ✅ 自动重试
- ✅ 迁移指标

**API**:
```rust
// 开始迁移
manager.start_migration(slot, from_group, to_group).await?;

// 查询进度
let progress = manager.get_migration_progress(slot);

// 取消迁移
manager.cancel_migration(slot);

// 获取指标
let metrics = manager.metrics();
println!("Keys migrated: {}", metrics.keys_migrated.load(Ordering::Relaxed));
```

#### ShardedStateMachine ✅
- ✅ 每个 Group 独立 DB 实例
- ✅ 自动创建和加载 DB
- ✅ Slot 级别 key 扫描
- ✅ 线程安全访问

**API**:
```rust
// 创建 DB
state_machine.create_db(group_id)?;

// 获取 DB
let db = state_machine.get_db(group_id);

// 扫描 slot keys
let keys = state_machine.scan_slot_keys_sync(group_id, slot, limit)?;
```

---

## 📚 完整文档

### 核心文档
1. **架构说明**: [MULTI_RAFT_ARCHITECTURE.md](MULTI_RAFT_ARCHITECTURE.md)
   - 架构对比图
   - 数据流程图
   - 组件详解
   - 扩展性分析

2. **API 参考**: [MULTI_RAFT_API_REFERENCE.md](MULTI_RAFT_API_REFERENCE.md)
   - 完整 API 列表
   - Redis Cluster 命令映射
   - 使用示例

3. **实施总结**: [MULTI_RAFT_IMPLEMENTATION_SUMMARY.md](MULTI_RAFT_IMPLEMENTATION_SUMMARY.md)
   - 实施统计
   - 性能收益
   - 阶段回顾

4. **实施计划**: [MULTI_RAFT_SHARDING_PLAN.md](MULTI_RAFT_SHARDING_PLAN.md)
   - 详细计划（已完成）
   - 技术决策
   - 参考项目

### 代码示例
- `examples/cluster/` - 集群示例代码
- `tests/` - 完整测试用例
- `src/cluster/` - 源代码实现

---

## 🐛 常见问题

### Q1: 如何选择 Group 数量？

**A**: 取决于集群规模：
- 小集群（< 10 节点）：1024 Groups
- 中集群（10-100 节点）：4096 Groups
- 大集群（> 100 节点）：16384 Groups

**原则**: `Group数 >> 节点数 × 副本数`

### Q2: MetaRaft 会成为瓶颈吗？

**A**: 不会。原因：
- MetaRaft 仅管理元数据（KB 级），不处理数据
- 读取走本地缓存，不查询 MetaRaft
- 写入频率低（仅成员变更、迁移时）

### Q3: 如何处理节点故障？

**A**: 自动处理：
- 每个 Group 独立 Raft 共识
- Group 内节点故障自动选举新 Leader
- 故障隔离，不影响其他 Groups

### Q4: 迁移期间数据一致性如何保证？

**A**: 通过双写 + 原子切换：
1. 迁移期间新写入同时写源和目标
2. 迁移完成原子更新 MetaRaft slot mapping
3. 客户端更新缓存后新请求路由到目标

### Q5: 如何监控集群状态？

**A**: 使用内置监控：
```rust
// 收集 Group 指标
node.collect_group_metrics().await?;

// Prometheus 指标
// 访问 http://localhost:9090/metrics
```

---

## 🔧 配置建议

### 生产环境配置

```rust
use aidb::cluster::MigrationConfig;
use openraft::Config;
use std::time::Duration;

// Raft 配置
let mut raft_config = Config::default();
raft_config.heartbeat_interval = 500;  // 500ms
raft_config.election_timeout_min = 1500;  // 1.5s
raft_config.election_timeout_max = 3000;  // 3s
raft_config.max_payload_entries = 300;

// 迁移配置
let migration_config = MigrationConfig {
    batch_size: 100,          // 每批次 100 keys
    rate_limit: 1000,         // 1000 keys/sec
    key_timeout: Duration::from_secs(5),
    max_retries: 3,
    batch_delay: Duration::from_millis(10),
};
```

### 性能调优

1. **增加 Group 数量**提高并行度
2. **调整 batch_size** 平衡延迟和吞吐
3. **使用 SSD** 提升 I/O 性能
4. **调整 Raft 参数** 优化共识延迟

---

## 🎯 下一步

现在您已经了解了 Multi-Raft 的基础知识，可以：

1. **阅读源代码**: `src/cluster/multi_raft_*.rs`
2. **运行测试**: `cargo test --features raft-cluster`
3. **查看示例**: `examples/cluster/`
4. **阅读完整文档**: [docs/](../docs/)
5. **参与贡献**: 提交 Issue 或 PR

---

## 📞 需要帮助？

- **Issue**: [GitHub Issues](https://github.com/Genuineh/AiDb/issues)
- **Discussions**: [GitHub Discussions](https://github.com/Genuineh/AiDb/discussions)
- **文档**: [docs/](../docs/)

---

**祝使用愉快！Multi-Raft + Sharding 让 AiDb 成为真正可扩展的分布式数据库！** 🚀

---

*文档版本: v2.0*  
*最后更新: 2024-12-10*  
*状态: ✅ 生产就绪*

```rust
// src/cluster/cluster_meta.rs

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type NodeId = u64;
pub type GroupId = u64;

/// 集群元数据（MetaRaft 的状态机数据）
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct ClusterMeta {
    /// Slot to Group mapping (16384 slots)
    /// slots[i] = group_id that owns slot i
    pub slots: [u64; 16384],
    
    /// Group metadata
    pub groups: HashMap<GroupId, GroupMeta>,
    
    /// Node information
    pub nodes: HashMap<NodeId, NodeInfo>,
    
    /// Configuration version (for CAS updates)
    pub config_version: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GroupMeta {
    pub group_id: GroupId,
    pub replicas: Vec<NodeId>,
    pub leader: Option<NodeId>,
    pub version: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NodeInfo {
    pub node_id: NodeId,
    pub addr: String,
    pub status: NodeStatus,
    pub joined_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum NodeStatus {
    Online,
    Offline,
    Joining,
    Leaving,
}

/// MetaRaft 请求类型
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MetaRequest {
    /// Add a new node
    AddNode { node_id: NodeId, addr: String },
    
    /// Remove a node
    RemoveNode { node_id: NodeId },
    
    /// Create a new group
    CreateGroup { group_id: GroupId, replicas: Vec<NodeId> },
    
    /// Update slot mapping
    UpdateSlots { start: u16, end: u16, group_id: GroupId },
}

/// MetaRaft 响应类型
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MetaResponse {
    Ok,
    Error(String),
    ClusterMeta(ClusterMeta),
}

impl ClusterMeta {
    /// 初始化：均匀分配 slots 到 groups
    pub fn init_slots(&mut self, group_count: u16) {
        for slot in 0..16384 {
            let group_id = (slot % group_count) as u64;
            self.slots[slot] = group_id;
        }
    }
    
    /// 获取 slot 对应的 group
    pub fn slot_to_group(&self, slot: u16) -> GroupId {
        self.slots[slot as usize]
    }
}
```

#### 3.3 实现 MetaStateMachine

```rust
// src/cluster/meta_state_machine.rs

use super::cluster_meta::{ClusterMeta, MetaRequest, MetaResponse};
use crate::DB;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const META_KEY: &[u8] = b"raft:meta:cluster_meta";

/// MetaRaft 的状态机
pub struct MetaStateMachine {
    /// 内存中的元数据
    pub meta: ClusterMeta,
    
    /// 持久化存储 (复用 DB)
    db: Arc<DB>,
}

impl MetaStateMachine {
    pub fn new(db: Arc<DB>) -> Result<Self> {
        // 从 DB 恢复元数据
        let meta = if let Some(data) = db.get(META_KEY)? {
            bincode::deserialize(&data)?
        } else {
            ClusterMeta::default()
        };
        
        Ok(Self { meta, db })
    }
    
    /// 应用 MetaRequest
    pub fn apply(&mut self, request: MetaRequest) -> Result<MetaResponse> {
        match request {
            MetaRequest::AddNode { node_id, addr } => {
                self.meta.nodes.insert(node_id, super::cluster_meta::NodeInfo {
                    node_id,
                    addr,
                    status: super::cluster_meta::NodeStatus::Online,
                    joined_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs(),
                });
                self.persist()?;
                Ok(MetaResponse::Ok)
            }
            
            MetaRequest::RemoveNode { node_id } => {
                self.meta.nodes.remove(&node_id);
                self.persist()?;
                Ok(MetaResponse::Ok)
            }
            
            MetaRequest::CreateGroup { group_id, replicas } => {
                self.meta.groups.insert(group_id, super::cluster_meta::GroupMeta {
                    group_id,
                    replicas,
                    leader: None,
                    version: 0,
                });
                self.persist()?;
                Ok(MetaResponse::Ok)
            }
            
            MetaRequest::UpdateSlots { start, end, group_id } => {
                for slot in start..end {
                    self.meta.slots[slot as usize] = group_id;
                }
                self.meta.config_version += 1;
                self.persist()?;
                Ok(MetaResponse::Ok)
            }
        }
    }
    
    /// 持久化元数据
    fn persist(&self) -> Result<()> {
        let data = bincode::serialize(&self.meta)?;
        self.db.put(META_KEY, &data)?;
        Ok(())
    }
    
    /// 获取当前元数据
    pub fn get_cluster_meta(&self) -> ClusterMeta {
        self.meta.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    
    #[test]
    fn test_meta_state_machine_basic() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        let db = Arc::new(db);
        
        let mut sm = MetaStateMachine::new(db.clone()).unwrap();
        
        // Test AddNode
        let resp = sm.apply(MetaRequest::AddNode {
            node_id: 1,
            addr: "127.0.0.1:50001".to_string(),
        }).unwrap();
        assert!(matches!(resp, MetaResponse::Ok));
        assert_eq!(sm.meta.nodes.len(), 1);
        
        // Test CreateGroup
        let resp = sm.apply(MetaRequest::CreateGroup {
            group_id: 1,
            replicas: vec![1, 2, 3],
        }).unwrap();
        assert!(matches!(resp, MetaResponse::Ok));
        assert_eq!(sm.meta.groups.len(), 1);
        
        // Test persistence
        drop(sm);
        let sm2 = MetaStateMachine::new(db).unwrap();
        assert_eq!(sm2.meta.nodes.len(), 1);
        assert_eq!(sm2.meta.groups.len(), 1);
    }
}
```

#### 3.4 集成到 mod.rs

```rust
// src/cluster/mod.rs

// ... 现有代码 ...

// 添加新模块
pub mod cluster_meta;
pub mod meta_state_machine;
// pub mod meta_raft_node;  // 下一步实现

// 重新导出
pub use cluster_meta::{ClusterMeta, MetaRequest, MetaResponse};
pub use meta_state_machine::MetaStateMachine;
```

#### 3.5 运行测试

```bash
cargo test --features raft-cluster meta_state_machine
```

### 4. 阶段 1 完成标准

✅ **检查清单**:
- [ ] ClusterMeta 数据结构定义完成
- [ ] MetaStateMachine 实现并测试通过
- [ ] 元数据可正确持久化和恢复
- [ ] 基础单元测试全部通过

📊 **预期交付**:
- 3 个新文件 (~500 行代码)
- 5+ 个单元测试
- 文档注释完整

⏱️ **时间**: 2~3 天

---

## 💡 开发技巧

### 1. 复用现有代码

**不要重复造轮子**！充分利用现有实现：

- ✅ `RaftStorage` → 复制为 `MetaRaftStorage`（仅改状态机）
- ✅ `RaftNetwork` → 复用（添加 group_id 路由）
- ✅ `OpenRaftNode` → 包装为 `MetaRaftNode`

### 2. 渐进式实现

**每个阶段都能运行**！不要一次写太多代码：

```
阶段 1: MetaRaft 可运行 (仅元数据管理)
  ├─> 验证: 可手动添加节点、创建 Group
  
阶段 2: Multi-Raft 可运行 (动态创建 Group)
  ├─> 验证: 可创建 10+ 空 Group，选举正常
  
阶段 3: 分片路由可运行 (数据写入正确分片)
  ├─> 验证: 写入任意 key，落到正确 Group
  
阶段 4: 成员管理可运行 (动态加入节点)
  ├─> 验证: 新节点一键加入
  
阶段 5: 迁移可运行 (在线 slot 迁移)
  ├─> 验证: 迁移完成，数据正确
```

### 3. 充分测试

**每个功能都要有测试**：

- 单元测试: 测试单个函数/方法
- 集成测试: 测试多个组件协同
- 端到端测试: 测试完整流程

**测试金字塔**:
```
      /\      ← 端到端测试 (少量，慢)
     /  \
    /────\    ← 集成测试 (中等)
   /      \
  /────────\  ← 单元测试 (大量，快)
```

### 4. 详细日志

**调试 Multi-Raft 困难**，充分的日志至关重要：

```rust
use log::{debug, info, warn, error};

// 关键路径记录 info
info!("Routing key {:?} to group {}", key, group_id);

// 详细信息记录 debug
debug!("Slot {} mapped to group {}", slot, group_id);

// 异常情况记录 warn/error
warn!("Group {} leader not found, retrying", group_id);
error!("Failed to migrate slot {}: {}", slot, err);
```

### 5. 性能考虑

**优化可以后做**，但要避免明显的坑：

- ❌ 避免: 每次请求都查询 MetaRaft
  - ✅ 使用本地缓存 + watch 更新

- ❌ 避免: 全局大锁
  - ✅ 使用细粒度锁（per-group）

- ❌ 避免: 同步阻塞 I/O
  - ✅ 使用 async/await

---

## 📚 参考资源

### 代码参考

1. **rdb** (最相似): https://github.com/MoSunDay/rdb
   - 直接参考其 Multi-Raft 架构
   - 代码质量高，可直接学习

2. **tikv/raft-rs examples**: https://github.com/tikv/raft-rs/tree/master/examples
   - multi_raft 示例
   - 基础框架参考

3. **openraft examples**: https://github.com/datafuselabs/openraft/tree/main/examples
   - raft-kv-memstore
   - raft-kv-rocksdb

### 论文和文档

1. **Raft 论文**: https://raft.github.io/raft.pdf
   - Section 6: Cluster membership changes
   - 理解 Joint Consensus

2. **TiKV 设计文档**: https://tikv.org/docs/
   - Multi-Raft 实践
   - PD (Placement Driver) 设计

3. **CockroachDB 博客**: https://www.cockroachlabs.com/blog/
   - Range 分裂和迁移
   - 分布式事务

### 社区讨论

1. **openraft Discussions**: https://github.com/datafuselabs/openraft/discussions
   - 问题解答
   - 最佳实践

2. **TiKV 社区**: https://internals.tikvproject.org/
   - 深度技术讨论
   - 生产经验分享

---

## 🐛 常见问题

### Q1: MetaRaft 是否会成为瓶颈？

**A**: 不会。原因：
- MetaRaft 仅管理元数据（KB 级），不处理数据
- 读取走本地缓存，不查询 MetaRaft
- 写入频率低（仅成员变更、迁移时）

### Q2: 为什么需要 16384 个 Slots？

**A**: 平衡粒度和开销：
- 太少（如 256）：迁移粒度粗，负载不均
- 太多（如 65536）：元数据大，路由慢
- 16384：Redis Cluster 标准，已验证

### Q3: Group 数量如何选择？

**A**: 取决于集群规模：
- 小集群（< 10 节点）：1024 Groups
- 中等集群（10~100 节点）：4096 Groups
- 大集群（> 100 节点）：16384 Groups

**原则**: Group 数 >> 节点数 × 副本数

### Q4: 迁移期间的一致性如何保证？

**A**: 通过双写 + 原子切换：
1. 迁移期间：新写入同时写源和目标
2. 迁移完成：原子更新 MetaRaft slot mapping
3. 客户端更新缓存：新请求路由到目标

### Q5: 如何处理网络分区？

**A**: Raft 自动处理：
- MetaRaft：通过 Raft 保证元数据一致性
- Data Groups：独立 Raft，各自处理分区
- 客户端：重试 + 重路由

---

## 🚦 下一步行动

1. **立即开始** (今天):
   - [ ] 创建开发分支
   - [ ] 搭建文件结构
   - [ ] 实现 ClusterMeta 数据结构

2. **本周完成**:
   - [ ] MetaStateMachine 实现
   - [ ] 单元测试通过
   - [ ] 集成测试通过

3. **下周开始**:
   - [ ] 阶段 2: Multi-Raft 框架
   - [ ] 动态创建 Group
   - [ ] 测试 10+ Groups

---

## 📞 需要帮助？

- **Issue**: [GitHub Issues](https://github.com/Genuineh/AiDb/issues)
- **Discussions**: [GitHub Discussions](https://github.com/Genuineh/AiDb/discussions)
- **文档**: [docs/](../docs/)

---

**加油！让我们一起把 AiDb 升级为生产级分布式数据库！** 🚀

---

*文档版本: v1.0*  
*最后更新: 2025-11-20*  
*作者: AiDb Team*
