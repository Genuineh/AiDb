# AiDb 架构设计文档

## 目录

- [1. 架构概览](#1-架构概览)
- [2. 单机版架构](#2-单机版架构)
- [3. Multi-Raft 集群架构](#3-multi-raft-集群架构)
- [4. 数据模型](#4-数据模型)
- [5. 关键设计决策](#5-关键设计决策)

---

## 1. 架构概览

AiDb采用分阶段演进的架构设计：

```
阶段A-C: 单机版LSM-Tree引擎
    ↓
阶段1-2: 添加OpenRaft共识层
    ↓
阶段3-6: 完整的Multi-Raft分布式集群
```

### 核心设计理念

**借鉴RocksDB优点，避免其复杂性**

✅ **借鉴**：
- 成熟的LSM-Tree分层架构
- Leveled Compaction策略
- Bloom Filter优化
- 经过验证的文件格式

❌ **避免**：
- 过度复杂的配置（200+ → <20）
- 臃肿的API（100+ → <30）
- C++依赖和编译问题
- 不必要的特性（Column Families等）

**创新设计**：
- 🆕 Multi-Raft实现真正的水平扩展
- 🆕 Slot-based分片（兼容Redis Cluster）
- 🆕 动态成员变更和热迁移

---

## 2. 单机版架构

### 2.1 整体架构

```
┌─────────────────────────────────────────────┐
│                  DB API                      │
├─────────────────────────────────────────────┤
│                Write Path                    │
│  Client → WAL → MemTable → Flush → SSTable  │
├─────────────────────────────────────────────┤
│                Read Path                     │
│  Client → MemTable → Imm → Cache → SSTable  │
├─────────────────────────────────────────────┤
│             Background Tasks                 │
│  • Flush (MemTable → SSTable)               │
│  • Compaction (SSTable Merge)               │
└─────────────────────────────────────────────┘
```

### 2.2 核心组件

#### WAL (Write-Ahead Log)
**职责**：确保数据持久化

```rust
// Record格式（借鉴RocksDB）
[checksum: u32]  // CRC32校验
[length: u16]    // 数据长度  
[type: u8]       // FULL/FIRST/MIDDLE/LAST
[data: bytes]    // 实际数据

// 特性
- 顺序追加写入
- fsync保证持久化
- 支持崩溃恢复
- 支持日志轮转
```

#### MemTable
**职责**：内存中的有序索引

```rust
// 数据结构
- SkipList（使用crossbeam-skiplist）
- 并发安全（多读单写）
- 大小限制（默认4MB）

// 操作
- Put: O(log n)
- Get: O(log n)  
- Delete: 墓碑标记
- Iterator: 有序遍历

// 状态转换
Mutable → (Full) → Immutable → (Flush) → Dropped
```

#### SSTable (Sorted String Table)
**职责**：磁盘上的有序不可变文件

```rust
// 文件格式
[Data Block 1]    // 4KB, KV pairs
[Data Block 2]
...
[Data Block N]
[Meta Block]      // Bloom Filter
[Index Block]     // Data Block索引
[Footer: 48B]     // 指向Index Block

// 特性
- 不可变（Immutable）
- 分Block存储（4KB）
- 二级索引（Index → Data）
- Bloom Filter加速查询
```

#### Compaction
**职责**：合并SSTable，回收空间

```rust
// Leveled Compaction策略
Level 0: 4个文件，可能重叠
Level 1: 10MB，有序不重叠
Level 2: 100MB，有序不重叠
Level N: 10^N MB

// 触发条件
- Level 0: 文件数 >= 4
- Level N: 总大小 >= 阈值

// 执行过程
1. 选择文件（Level N + Level N+1重叠）
2. 多路归并排序
3. 生成新SSTable
4. 更新Manifest
5. 删除旧文件
```

### 2.3 数据流

#### 写入路径
```
1. Client.put(key, value)
   ↓
2. WAL.append(record)
   ↓  
3. WAL.sync() (可选)
   ↓
4. MemTable.put(key, value)
   ↓
5. Check MemTable size
   ↓ (if full)
6. Trigger Flush
   ↓
7. MemTable → Immutable
   ↓
8. Create new MemTable
   ↓
9. Background: Flush Immutable → SSTable
```

#### 读取路径
```
1. Client.get(key)
   ↓
2. Check MemTable
   ↓ (if not found)
3. Check Immutable MemTables
   ↓ (if not found)
4. Check Block Cache
   ↓ (if miss)
5. For each level (0 → N):
   a. Check Bloom Filter
   b. Binary search Index Block
   c. Read Data Block
   ↓
6. Return value or None
```

### 2.4 文件组织

```
data_dir/
├── LOCK              # 进程锁
├── CURRENT           # 当前Manifest
├── MANIFEST-000001   # 元数据日志
├── 000001.log        # WAL
├── 000002.sst        # SSTable (Level 0)
├── 000003.sst        # SSTable (Level 0)
├── 000004.sst        # SSTable (Level 1)
└── 000005.sst        # SSTable (Level 1)
```

---

## 3. Multi-Raft 集群架构

### 3.1 整体架构

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
    │Slots 0-X│                  │Slots X+1│                   │  ...    │
    └────┬────┘                  └────┬────┘                   └────┬────┘
         │                             │                             │
    ┌────▼─────────────────────────────▼─────────────────────────────▼────┐
    │                         Raft Consensus Layer                        │
    │  ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐         │
    │  │ Node 1  │    │ Node 2  │    │ Node 3  │    │ Node 4  │ ...     │
    │  │Leader G0│    │Follower │    │Leader G1│    │Follower │         │
    │  │Follower │    │Leader G2│    │Follower │    │Leader G3│         │
    │  └────┬────┘    └────┬────┘    └────┬────┘    └────┬────┘         │
    └───────┼──────────────┼──────────────┼──────────────┼───────────────┘
            │              │              │              │
    ┌───────▼──────────────▼──────────────▼──────────────▼───────────────┐
    │                       Local LSM-Tree Storage                       │
    │                  WAL → MemTable → SSTable                          │
    └────────────────────────────────────────────────────────────────────┘
```

### 3.2 核心概念

#### Slot-based Sharding (槽分片)
```rust
// 16384个槽位，兼容Redis Cluster协议
const SLOT_COUNT: u16 = 16384;

// 槽计算
fn slot(key: &[u8]) -> u16 {
    crc16::checksum_x25(key) % SLOT_COUNT
}

// 槽分配给Group
// Group 0: slots 0-5460
// Group 1: slots 5461-10922
// Group 2: slots 10923-16383
```

#### Raft Group (复制组)
```rust
// 每个Group是一个独立的Raft复制组
pub struct RaftGroup {
    group_id: GroupId,
    slots: Vec<SlotRange>,    // 负责的槽位范围
    
    // Raft状态
    raft: OpenRaft<TypeConfig>,
    log_storage: RaftStorage,
    state_machine: StateMachine,
    
    // 成员信息
    leader: Option<NodeId>,
    voters: HashSet<NodeId>,
}
```

#### Node (集群节点)
```rust
// 每个节点可以参与多个Raft Group
pub struct MultiRaftNode {
    node_id: NodeId,
    address: String,
    
    // 本节点参与的所有Group
    groups: HashMap<GroupId, RaftGroup>,
    
    // 底层存储（共享）
    db: Arc<DB>,
    
    // gRPC服务
    rpc_service: RaftServiceImpl,
}
```

### 3.3 数据流

#### 写入路径
```
1. Client: PUT key value
   ↓
2. Router: slot = CRC16(key) % 16384
   ↓
3. Router: group = slot_to_group(slot)
   ↓
4. Router: leader = group.leader
   ↓
5. Leader: raft.propose(WriteOp::Put{key, value})
   ↓
6. Raft: replicate log to majority
   ↓
7. Leader: apply to state machine (DB)
   ↓
8. Leader: respond OK
```

#### 读取路径
```
1. Client: GET key
   ↓
2. Router: slot = CRC16(key) % 16384
   ↓
3. Router: group = slot_to_group(slot)
   ↓
4. Any Node: read from local DB
   ↓
5. Return value

注意：读取可以从任意节点，提供最终一致性
如需强一致性读，可路由到Leader
```

### 3.4 成员变更

#### 添加节点
```rust
// 1. 启动新节点
let new_node = OpenRaftNode::new(node_id, config).await?;

// 2. 作为Learner加入集群
leader.add_learner(new_node_id, address).await?;

// 3. 等待日志追赶
// (openraft自动处理)

// 4. 提升为Voter
leader.change_membership(&[1, 2, 3, new_node_id]).await?;
```

#### 移除节点
```rust
// 1. 更改成员配置，移除节点
leader.change_membership(&[1, 2, 4]).await?;  // 移除节点3

// 2. 停止被移除的节点
node3.shutdown().await?;
```

### 3.5 槽迁移

```rust
// 将slots 5000-5460从Group 0迁移到Group 1
let migration = MigrationManager::new(config);

// 1. 暂停对迁移槽的写入
// 2. 快照迁移数据
// 3. 追赶增量日志
// 4. 切换槽所有权
// 5. 恢复写入
migration.migrate_slots(
    source_group: 0,
    target_group: 1,
    slots: 5000..=5460
).await?;
```

### 3.6 高可用设计

#### 故障场景和处理

| 故障类型 | 影响 | 恢复方式 | 恢复时间 |
|---------|------|---------|---------|
| Follower故障 | 复制因子↓ | 自动重连/替换 | 秒级 |
| Leader故障 | Group短暂不可写 | 自动选举新Leader | ~5秒 |
| 少数节点故障 | 集群正常运行 | 无需操作 | 自愈 |
| 多数节点故障 | 集群不可用 | 需人工恢复 | 分钟级 |

#### 数据一致性

**模型**：强一致性（通过Raft保证）

```
写入流程：
1. Client → Leader
2. Leader appends to log
3. Leader replicates to Followers
4. Majority acknowledges
5. Leader commits and applies
6. Leader responds to Client

保证：
- 写入被多数节点确认后才返回成功
- 已提交的日志不会丢失
- 所有节点最终应用相同的日志序列
```

### 3.7 与Redis Cluster的兼容性

AiDb Multi-Raft架构设计考虑了Redis Cluster协议兼容性：

| 特性 | Redis Cluster | AiDb Multi-Raft |
|------|---------------|-----------------|
| 槽数量 | 16384 | 16384 |
| 槽计算 | CRC16 | CRC16 |
| MOVED响应 | ✅ | ✅ (可实现) |
| ASK响应 | ✅ | ✅ (可实现) |
| Gossip协议 | ✅ | ❌ (使用Raft) |
| 一致性 | 最终一致 | 强一致 |

详细的Redis兼容性实现指南请参考：[REDIS_CLUSTER_COMPATIBILITY.md](REDIS_CLUSTER_COMPATIBILITY.md)

---

## 4. 数据模型

### 4.1 内部键格式

```rust
// InternalKey
[user_key: bytes]    // 用户key
[sequence: u64]      // 序列号（全局递增）
[type: u8]          // Put=1, Delete=0

// 排序规则
1. user_key升序
2. sequence降序（新的在前）
3. type降序（Put在Delete前）
```

### 4.2 SSTable格式

```
┌─────────────────────────────────────┐
│         Data Block 1                │
│  ┌─────────────────────────────┐   │
│  │ KV Pair 1                   │   │
│  │ KV Pair 2                   │   │
│  │ ...                         │   │
│  │ Restart Points [4B × N]     │   │
│  │ Num Restarts [4B]           │   │
│  └─────────────────────────────┘   │
├─────────────────────────────────────┤
│         Data Block 2                │
├─────────────────────────────────────┤
│         ...                         │
├─────────────────────────────────────┤
│         Meta Block                  │
│  (Bloom Filter)                     │
├─────────────────────────────────────┤
│         Index Block                 │
│  ┌─────────────────────────────┐   │
│  │ Block 1: key, offset, size  │   │
│  │ Block 2: key, offset, size  │   │
│  │ ...                         │   │
│  └─────────────────────────────┘   │
├─────────────────────────────────────┤
│         Footer (48 bytes)           │
│  ┌─────────────────────────────┐   │
│  │ Meta Index Handle [20B]     │   │
│  │ Index Handle [20B]          │   │
│  │ Magic Number [8B]           │   │
│  └─────────────────────────────┘   │
└─────────────────────────────────────┘
```

### 4.3 Manifest格式

```rust
// Manifest记录版本变更
enum VersionEdit {
    // 新增文件
    AddFile {
        level: u32,
        file_number: u64,
        file_size: u64,
        smallest_key: Vec<u8>,
        largest_key: Vec<u8>,
    },
    
    // 删除文件
    DeleteFile {
        level: u32,
        file_number: u64,
    },
    
    // 设置序列号
    SetSequenceNumber(u64),
    
    // 设置Compaction指针
    SetCompactionPointer {
        level: u32,
        key: Vec<u8>,
    },
}
```

---

## 5. 关键设计决策

### 5.1 为什么选择Multi-Raft？

**对比其他方案**：

| 方案 | 一致性 | 扩展性 | 复杂度 | 运维成本 |
|------|--------|--------|--------|----------|
| 单Raft组 | 强一致 | 差（单Leader瓶颈） | 低 | 低 |
| Multi-Raft | 强一致 | 好（多Leader并行） | 中 | 中 |
| 无共识P2P | 最终一致 | 好 | 低 | 低 |
| Paxos | 强一致 | 中 | 高 | 高 |

**选择Multi-Raft的原因**：
- ✅ 强一致性保证，满足金融级场景
- ✅ 水平扩展能力（添加Group增加吞吐）
- ✅ OpenRaft库成熟可靠
- ✅ 与Redis Cluster槽分片模型兼容

### 5.2 为什么用16384个槽？

**兼容性考虑**：
- 与Redis Cluster完全兼容
- 成熟的槽迁移协议可复用
- 社区工具生态可复用

**技术考虑**：
- 16384槽足够细粒度进行负载均衡
- 槽信息占用内存小（16KB位图）
- CRC16计算高效

### 5.3 为什么每个节点可以参与多个Group？

**资源利用**：
```
传统方案（每Group独占节点）：
- 3节点 × 3 Group = 9节点
- 资源利用率低

Multi-Raft（节点共享）：
- 3节点，每个参与3个Group
- Leader分布在不同节点
- 资源利用率高
```

**容错性**：
```
节点1: Leader(G0), Follower(G1), Follower(G2)
节点2: Follower(G0), Leader(G1), Follower(G2)
节点3: Follower(G0), Follower(G1), Leader(G2)

任一节点故障：
- 受影响Group自动选举新Leader
- 其他Group不受影响
- 集群整体可用
```

### 5.4 为什么状态机数据使用前缀存储？

```rust
// Raft元数据：raft:vote, raft:log:1, raft:snapshot
// 状态机数据：sm:user_key1, sm:user_key2

优势：
- 命名空间隔离
- 可独立清理Raft日志
- 便于调试和监控
- 不影响用户key空间
```

### 5.5 为什么用gRPC而非自定义协议？

| 维度 | gRPC | 自定义协议 |
|------|------|-----------|
| 开发效率 | 高（proto生成） | 低 |
| 跨语言 | ✅ | 需额外工作 |
| 性能 | 优秀（HTTP/2） | 可能更好 |
| 调试 | grpcurl等工具 | 需自建 |
| 流控 | 内置 | 需自建 |

**结论**：gRPC的开发效率和生态优势大于自定义协议的性能优势。

---

## 总结

AiDb的架构设计遵循以下原则：

1. **渐进式演进**：单机 → Multi-Raft集群，每阶段独立可用
2. **强一致性**：通过Raft共识保证数据不丢失
3. **水平扩展**：Multi-Raft多Group并行，无单点瓶颈
4. **协议兼容**：16384槽设计兼容Redis Cluster协议
5. **Rust优势**：类型安全、内存安全、高性能

最终目标：构建一个**高性能、强一致、易扩展**的分布式KV存储引擎。

---

更多技术细节请参考：
- [Multi-Raft架构详解](MULTI_RAFT_ARCHITECTURE.md)
- [Multi-Raft快速入门](MULTI_RAFT_QUICKSTART.md)
- [Multi-Raft API参考](MULTI_RAFT_API_REFERENCE.md)
- [Redis兼容性指南](REDIS_CLUSTER_COMPATIBILITY.md)
- [设计决策](DESIGN_DECISIONS.md)
