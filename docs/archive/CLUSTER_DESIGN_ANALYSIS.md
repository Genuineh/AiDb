# 集群方案设计分析

## 🎯 用户提出的方案

### 原始方案描述

```
架构：
┌─────────────┐
│  主节点      │ ← 读写操作
│  (Primary)  │
└──────┬──────┘
       │
       ├────── 共享磁盘 ──────┐
       │                     │
┌──────┴──────┐      ┌──────┴──────┐
│  从节点1     │      │  从节点2     │
│  (Replica1) │      │  (Replica2) │
│  (只读)     │      │  (只读)     │
└─────────────┘      └─────────────┘

特点：
1. 主从共享一个磁盘
2. 从节点不写入，避免冲突
3. 多个主从组组成大集群
```

---

## ❌ 方案的严重问题

### 问题1: LSM-Tree的后台写入操作

**冲突场景**：
```
即使应用层"从节点不写"，LSM-Tree的后台操作仍会修改文件：

主节点的后台操作：
1. MemTable Flush → 创建新SSTable
2. Compaction → 删除旧SSTable，创建新SSTable
3. Manifest更新 → 记录文件变更
4. WAL轮转 → 删除旧WAL

问题：
❌ 从节点正在读取SSTable文件A
❌ 主节点Compaction删除了文件A
❌ 从节点读取失败！
```

**代码示例**：
```rust
// 主节点 - Compaction删除旧文件
fn compaction() {
    // 1. 合并文件1、2、3 -> 文件4
    merge_sstables(&[file1, file2, file3], file4);
    
    // 2. 更新Manifest
    manifest.add_file(file4);
    manifest.delete_files(&[file1, file2, file3]);
    
    // 3. 删除旧文件 ❌ 从节点可能正在读取！
    fs::remove_file(file1)?;
    fs::remove_file(file2)?;
    fs::remove_file(file3)?;
}

// 从节点 - 同时读取
fn read_from_replica() {
    // ❌ 文件可能被主节点删除了！
    let sstable = SSTable::open(file1)?; // Error: File not found
}
```

### 问题2: 元数据一致性

**Manifest文件的竞争**：
```
时间线：
T1: 主节点开始Compaction
T2: 主节点更新Manifest (version 10)
T3: 从节点读取Manifest (version 9)  ❌ 旧版本
T4: 从节点尝试打开已删除的SSTable  ❌ 失败
T5: 主节点删除旧SSTable文件

问题：
❌ 从节点的Manifest视图与实际文件系统不一致
❌ 从节点无法感知主节点的文件变更
❌ 需要复杂的同步机制
```

**代码冲突**：
```rust
// 主节点更新Manifest
fn update_manifest(edit: VersionEdit) {
    manifest.write(edit)?;  // version 10
    manifest.sync()?;
}

// 从节点读取Manifest
fn load_manifest() -> Version {
    // ❌ 可能读到旧版本或不完整的版本
    manifest.read()?  // version 9?
}
```

### 问题3: WAL的读写冲突

**场景**：
```
主节点写入流程：
1. append to WAL
2. fsync WAL
3. write to MemTable
4. MemTable full → Flush
5. delete old WAL

从节点读取流程：
1. 查MemTable ❌ 从节点的MemTable是空的！
2. 查SSTable ❌ 最新数据还在主节点的MemTable中
3. 数据不一致！

问题：
❌ 从节点看不到主节点MemTable中的数据
❌ 从节点无法读取主节点的WAL（可能正在写入）
❌ 读取延迟：必须等主节点Flush
```

### 问题4: 文件系统缓存和锁

**操作系统层面的问题**：
```
Linux文件系统：
1. 页缓存 (Page Cache)
   ❌ 主节点写入的数据可能在缓存中
   ❌ 从节点读取可能得到旧数据

2. 文件锁
   ❌ RocksDB使用flock防止多实例
   ❌ 共享磁盘需要禁用锁 → 失去保护

3. Direct I/O
   ❌ 性能优化与共享读取冲突
```

**代码问题**：
```rust
// 主节点打开数据库
fn open_db(path: &str) -> Result<DB> {
    // RocksDB会创建LOCK文件防止多实例
    let lock = acquire_lock(path)?;  // flock
    
    // ❌ 从节点无法打开同一路径！
}

// 从节点尝试打开
fn open_replica(path: &str) -> Result<DB> {
    // ❌ 锁已被主节点持有
    let lock = acquire_lock(path)?;  // Error: Resource busy
}
```

### 问题5: 共享存储的性能瓶颈

**网络存储的限制**：
```
NFS / EBS / Ceph：
├─ 延迟高：网络往返 1-10ms
├─ 吞吐有限：100-500 MB/s
├─ IOPS限制：10K-20K
└─ 成本高昂

本地SSD：
├─ 延迟低：< 0.1ms
├─ 吞吐高：3000+ MB/s
├─ IOPS高：500K+
└─ 但无法共享（物理限制）

问题：
❌ 共享存储成为性能瓶颈
❌ 主从节点竞争I/O资源
❌ 无法利用本地SSD的性能
```

### 问题6: 故障场景分析

**单点故障**：
```
故障1: 共享磁盘故障
└─> 主从节点全部不可用 ❌

故障2: 主节点故障
├─> 从节点只能读
├─> 无法写入
└─> 需要人工切换 ❌

故障3: 网络分区
├─> 从节点与共享磁盘断开
└─> 从节点不可用 ❌

问题：
❌ 共享磁盘是单点故障
❌ 无法自动故障转移
❌ 可用性反而降低
```

---

## ✅ 推荐的替代方案

### 方案A: 基于Raft的主从复制 ⭐ **强烈推荐**

**架构**：
```
┌──────────────┐     Write       ┌──────────────┐
│  Client      │ ──────────────> │  Leader      │
└──────────────┘                 │  (主节点)    │
                                 └──────┬───────┘
                                        │ Raft Log
                          ┌─────────────┼─────────────┐
                          │             │             │
                     ┌────▼────┐   ┌───▼─────┐  ┌───▼─────┐
                     │Follower1│   │Follower2│  │Follower3│
                     │(从节点) │   │(从节点) │  │(从节点) │
                     └─────────┘   └─────────┘  └─────────┘
                          │             │             │
                     ┌────▼────┐   ┌───▼─────┐  ┌───▼─────┐
                     │Local    │   │Local    │  │Local    │
                     │Disk     │   │Disk     │  │Disk     │
                     └─────────┘   └─────────┘  └─────────┘

特点：
✅ 每个节点独立存储
✅ 强一致性保证
✅ 自动故障转移
✅ 可横向扩展
```

**实现方案**：
```rust
// 使用raft-rs库
pub struct RaftDB {
    raft: RaftNode,           // Raft共识层
    storage: DB,              // 本地LSM存储
    state_machine: StateMachine,
}

impl RaftDB {
    // 写操作通过Raft复制
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        // 1. 构造Raft日志
        let log_entry = LogEntry::Put(key, value);
        
        // 2. 提交到Raft（会复制到多数节点）
        self.raft.propose(log_entry).await?;
        
        // 3. 等待Raft提交
        // 4. 应用到本地存储
        Ok(())
    }
    
    // 读操作可以从任意节点
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // 直接读本地存储
        self.storage.get(key)
    }
    
    // 状态机应用日志
    fn apply(&mut self, entry: LogEntry) {
        match entry {
            LogEntry::Put(k, v) => self.storage.put(k, v),
            LogEntry::Delete(k) => self.storage.delete(k),
        }
    }
}
```

**优势**：
- ✅ 强一致性（线性一致性）
- ✅ 自动Leader选举
- ✅ 多数节点可用即可服务
- ✅ 成熟的开源实现（raft-rs）

**劣势**：
- ⚠️ 写入延迟增加（需要多数节点确认）
- ⚠️ 网络开销（日志复制）
- ⚠️ 实现复杂度较高

---

### 方案B: 基于WAL流复制 ⭐ **推荐**

**架构**：
```
┌──────────────┐     Write       ┌──────────────┐
│  Client      │ ──────────────> │  Primary     │
└──────────────┘                 │  (主节点)    │
       │ Read                     └──────┬───────┘
       │                                 │ WAL Stream
       │                    ┌────────────┼────────────┐
       │                    │            │            │
       │               ┌────▼────┐  ┌───▼─────┐ ┌───▼─────┐
       └──────────────>│Replica1 │  │Replica2 │ │Replica3 │
                       │(从节点) │  │(从节点) │ │(从节点) │
                       └────┬────┘  └────┬────┘ └────┬────┘
                            │            │           │
                       ┌────▼────┐  ┌───▼─────┐ ┌──▼──────┐
                       │Local    │  │Local    │ │Local    │
                       │Disk     │  │Disk     │ │Disk     │
                       └─────────┘  └─────────┘ └─────────┘

特点：
✅ 异步复制，低延迟
✅ 实现相对简单
✅ 类似PostgreSQL/MySQL
```

**实现方案**：
```rust
// 主节点
pub struct PrimaryDB {
    db: DB,
    wal_sender: WalSender,
}

impl PrimaryDB {
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        // 1. 写入本地WAL和MemTable
        self.db.put(key, value)?;
        
        // 2. 异步发送WAL到从节点
        self.wal_sender.send_wal_entry(key, value)?;
        
        Ok(())
    }
}

// 从节点
pub struct ReplicaDB {
    db: DB,
    wal_receiver: WalReceiver,
    apply_thread: JoinHandle<()>,
}

impl ReplicaDB {
    pub fn start(&mut self) {
        // 后台线程接收并应用WAL
        self.apply_thread = spawn(move || {
            loop {
                // 1. 接收WAL entry
                let entry = self.wal_receiver.recv()?;
                
                // 2. 应用到本地存储
                match entry {
                    WalEntry::Put(k, v) => self.db.put(k, v)?,
                    WalEntry::Delete(k) => self.db.delete(k)?,
                }
            }
        });
    }
    
    // 从节点只读
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db.get(key)
    }
}
```

**优势**：
- ✅ 写入延迟低（异步复制）
- ✅ 实现相对简单
- ✅ 可配置同步/异步
- ✅ 可横向扩展读能力

**劣势**：
- ⚠️ 最终一致性（有复制延迟）
- ⚠️ 需要手动故障转移
- ⚠️ 可能丢失少量数据（异步模式）

---

### 方案C: 分片集群 ⭐ **大规模场景**

**架构**：
```
                    ┌──────────────┐
                    │  Coordinator │
                    │  (调度器)    │
                    └──────┬───────┘
                           │
         ┌─────────────────┼─────────────────┐
         │                 │                 │
    ┌────▼────┐       ┌───▼─────┐      ┌───▼─────┐
    │ Shard 1 │       │ Shard 2 │      │ Shard 3 │
    │(0-333)  │       │(334-666)│      │(667-999)│
    └────┬────┘       └────┬────┘      └────┬────┘
         │                 │                 │
    ┌────┴────┐       ┌───┴─────┐      ┌───┴─────┐
    │Replicas │       │Replicas │      │Replicas │
    │  x3     │       │  x3     │      │  x3     │
    └─────────┘       └─────────┘      └─────────┘

特点：
✅ 水平扩展
✅ 大数据量支持
✅ 高可用（每个分片多副本）
```

**实现方案**：
```rust
pub struct ClusterDB {
    shards: Vec<ShardGroup>,
    coordinator: Coordinator,
}

pub struct ShardGroup {
    shard_id: u64,
    key_range: (Vec<u8>, Vec<u8>),
    primary: RaftDB,
    replicas: Vec<RaftDB>,
}

impl ClusterDB {
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        // 1. 根据key找到对应shard
        let shard = self.find_shard(key)?;
        
        // 2. 写入shard的primary
        shard.primary.put(key, value)
    }
    
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // 1. 找到shard
        let shard = self.find_shard(key)?;
        
        // 2. 可以从任意副本读取
        shard.read_from_any(key)
    }
    
    fn find_shard(&self, key: &[u8]) -> Result<&ShardGroup> {
        // 一致性哈希或range分片
        let hash = hash(key);
        let shard_id = hash % self.shards.len();
        Ok(&self.shards[shard_id])
    }
}
```

**优势**：
- ✅ 无限水平扩展
- ✅ 支持海量数据
- ✅ 高可用（每个分片独立）
- ✅ 负载均衡

**劣势**：
- ⚠️ 复杂度最高
- ⚠️ 跨分片查询困难
- ⚠️ 需要元数据管理
- ⚠️ 运维成本高

---

## 📊 方案对比

| 方案 | 一致性 | 可用性 | 性能 | 复杂度 | 适用场景 |
|------|-------|-------|------|--------|---------|
| **共享磁盘** | ❌ 差 | ❌ 低 | ⚠️ 中 | ⚠️ 中 | ❌ 不推荐 |
| **Raft复制** | ✅ 强 | ✅ 高 | ⚠️ 中 | ⚠️ 中 | ✅ 推荐 |
| **WAL流复制** | ⚠️ 最终 | ✅ 高 | ✅ 高 | ✅ 低 | ✅ 推荐 |
| **分片集群** | ✅ 强 | ✅ 高 | ✅ 高 | ❌ 高 | ⭐ 大规模 |

---

## 🎯 推荐实施路径

### 阶段1: 单机版（已规划）
```
Week 1-20: 完成单机版AiDb
├─ 阶段A: MVP
├─ 阶段B: 优化
└─ 阶段C: 生产就绪
```

### 阶段2: 主从复制（+6-8周）⭐ **推荐先做**
```
选择：方案B (WAL流复制)

Week 1-2: WAL流复制
├─ WAL Sender (主节点)
├─ WAL Receiver (从节点)
└─ 网络传输协议

Week 3-4: 状态同步
├─ 全量同步（Snapshot）
├─ 增量同步（WAL replay）
└─ 断线重连

Week 5-6: 监控和测试
├─ 复制延迟监控
├─ 故障转移测试
└─ 性能测试

理由：
✅ 实现简单
✅ 性能好
✅ 满足大部分需求
```

### 阶段3: Raft共识（+8-12周）**可选**
```
如果需要强一致性，升级到Raft

Week 1-4: 集成raft-rs
├─ Raft日志管理
├─ 快照机制
└─ 成员变更

Week 5-8: 状态机
├─ 日志应用
├─ 事务支持
└─ 线性一致性读

Week 9-12: 测试和优化
├─ Jepsen测试
├─ 性能调优
└─ 故障注入测试
```

### 阶段4: 分片集群（+12-16周）**长期规划**
```
大规模场景需要时再做

Week 1-4: 分片策略
├─ 一致性哈希
├─ Range分片
└─ 动态分片

Week 5-8: 元数据管理
├─ Coordinator
├─ 路由表
└─ 负载均衡

Week 9-12: 数据迁移
├─ Shard分裂
├─ Shard合并
└─ 在线迁移

Week 13-16: 完善和测试
```

---

## 💡 关键问题解答

### Q1: 为什么不推荐共享磁盘方案？

**A**: 核心问题是LSM-Tree的写入特性：
1. 后台Compaction会修改文件
2. 元数据（Manifest）频繁变更
3. 文件系统级别的缓存和锁问题
4. 共享存储成为性能瓶颈
5. 单点故障风险

### Q2: 如果一定要用共享存储怎么办？

**A**: 需要大量额外工作：
```
1. 实现文件版本管理
   - 延迟删除旧文件
   - 引用计数
   - 垃圾回收

2. 元数据同步机制
   - 从节点定期拉取Manifest
   - 监听文件变更
   - 处理不一致

3. 读缓存失效
   - 主节点通知从节点
   - 从节点刷新缓存

4. 禁用优化特性
   - Direct I/O
   - mmap
   - 某些并发优化

成本 > 收益，不如直接用Raft复制
```

### Q3: WAL流复制 vs Raft，如何选择？

| 场景 | 推荐方案 |
|------|---------|
| 读多写少 | WAL流复制 ✅ |
| 强一致性需求 | Raft ✅ |
| 低延迟要求 | WAL流复制 ✅ |
| 自动故障转移 | Raft ✅ |
| 实现简单 | WAL流复制 ✅ |
| 金融等关键场景 | Raft ✅ |

**建议**：先实现WAL流复制，有需要再升级到Raft

### Q4: 如何保证从节点数据不丢？

**WAL流复制**：
```rust
// 半同步复制
pub enum SyncMode {
    Async,   // 主节点写完立即返回（可能丢数据）
    Semi,    // 至少1个从节点确认（平衡）
    Full,    // 所有从节点确认（强一致，慢）
}

impl PrimaryDB {
    pub fn put_with_sync(&self, key: &[u8], value: &[u8], 
                         mode: SyncMode) -> Result<()> {
        // 写本地
        self.db.put(key, value)?;
        
        // 等待从节点
        match mode {
            Async => Ok(()),
            Semi => self.wait_one_replica()?,
            Full => self.wait_all_replicas()?,
        }
    }
}
```

---

## 总结

### ❌ 原方案（共享磁盘）不可行

**主要问题**：
1. LSM-Tree后台操作冲突
2. 元数据一致性难保证
3. 文件系统缓存和锁
4. 性能瓶颈
5. 单点故障

### ✅ 推荐方案

**短期（阶段2）**：
- 实现WAL流复制
- 异步复制，低延迟
- 满足大部分需求

**中期（阶段3，可选）**：
- 升级到Raft共识
- 强一致性
- 自动故障转移

**长期（阶段4）**：
- 分片集群
- 水平扩展
- 海量数据

### 🚀 立即行动

1. **确认方案选择**
2. **完成单机版**（当前阶段A-C）
3. **实现主从复制**（WAL流复制）
4. **根据需求决定是否升级Raft**

---

*等待你的反馈，确认技术方案后继续制定详细计划*
