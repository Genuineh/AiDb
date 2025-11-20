# Multi-Raft 实施快速指南

**目标**: 帮助开发者快速上手 Multi-Raft + Sharding 实施

**更新时间**: 2025-11-20

---

## 🚀 10 分钟快速开始

### 1. 理解架构

在开始编码前，建议先阅读以下文档：

1. **架构图解** (5分钟): [docs/MULTI_RAFT_ARCHITECTURE.md](MULTI_RAFT_ARCHITECTURE.md)
   - 快速理解当前架构 vs 目标架构
   - 可视化组件关系

2. **完整计划** (20分钟): [docs/MULTI_RAFT_SHARDING_PLAN.md](MULTI_RAFT_SHARDING_PLAN.md)
   - 6 阶段详细实施计划
   - 关键数据结构
   - 技术决策

3. **当前代码** (10分钟):
   - `src/cluster/raft_storage.rs` - 现有 RaftStorage 实现
   - `src/cluster/raft_network.rs` - 现有 RaftNetwork 实现
   - `src/cluster/raft_node_new.rs` - 现有 OpenRaftNode 实现

### 2. 环境准备

```bash
# 1. 切换到开发分支
cd /path/to/AiDb
git checkout -b feature/multi-raft-sharding

# 2. 确保 openraft 集成正常
cargo build --features raft-cluster
cargo test --features raft-cluster

# 3. 运行现有 openraft demo
cargo run --example openraft_demo --features raft-cluster
```

### 3. 阶段 1 骨架搭建 (第一周)

**目标**: 实现 MetaRaft，手动管理集群元数据

#### 3.1 创建文件结构

```bash
# 在 src/cluster/ 下创建新文件
touch src/cluster/meta_state_machine.rs
touch src/cluster/meta_raft_node.rs
touch src/cluster/cluster_meta.rs
```

#### 3.2 定义数据结构 (cluster_meta.rs)

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
