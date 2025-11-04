# 共享存储方案重新评估

## 🎯 用户的核心考量（重要！）

### 明确的设计目标

1. **减少数据复制开销** - 核心诉求
2. **弹性扩展** - 通过增加主从组实现
3. **节点无状态/轻状态** - 节点可随时重启
4. **异地备份容错** - 而非实时复制
5. **本地盘优先** - 网盘只做备份
6. **可接受短暂不一致** - 限流做得好，主要防意外

---

## 🔄 重新评估之前提出的"问题"

### 问题1: LSM-Tree后台写入冲突

**我之前的论述**：
```
❌ 主节点Compaction删除SSTable文件
❌ 从节点正在读取该文件
→ 读取失败
```

**重新评估**：

#### 场景A: 从节点不直接读文件（推荐）✅

```rust
// 架构调整
┌─────────────┐
│  主节点      │ ← 读写本地磁盘
│  (Primary)  │
└──────┬──────┘
       │ RPC服务
       ├────────────┐
       │            │
┌──────▼──────┐    ┌▼──────────┐
│  从节点1     │    │ 从节点2    │
│  (缓存层)   │    │ (缓存层)   │
└─────────────┘    └───────────┘

关键设计：
1. 主节点：独占本地SSD，完整LSM-Tree
2. 从节点：内存缓存 + RPC转发
3. 从节点不直接访问主节点的文件系统 ✅
```

**实现**：
```rust
// 主节点
pub struct PrimaryNode {
    db: DB,  // 本地LSM存储，独占访问
    rpc_server: RpcServer,  // 提供Get接口
}

impl PrimaryNode {
    // RPC服务
    pub async fn handle_get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db.get(key)  // 从本地DB读取
    }
}

// 从节点
pub struct ReplicaNode {
    cache: LruCache<Vec<u8>, Vec<u8>>,  // 只有缓存
    primary_client: RpcClient,          // RPC客户端
    // ✅ 不访问文件系统！
}

impl ReplicaNode {
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // 1. 查缓存
        if let Some(v) = self.cache.get(key) {
            return Ok(Some(v));
        }
        
        // 2. 转发到主节点（通过RPC）
        let value = self.primary_client.get(key).await?;
        
        // 3. 更新缓存
        if let Some(ref v) = value {
            self.cache.put(key.clone(), v.clone());
        }
        
        Ok(value)
    }
}
```

**结论**：
- ✅ **不是问题**：从节点不访问文件，无冲突
- ✅ 架构更清晰：主节点负责存储，从节点负责缓存

---

#### 场景B: 从节点只读访问共享盘（可选方案）

如果确实想让从节点直接读文件（减少RPC开销）：

```rust
// 解决方案1: 引用计数 + 延迟删除
pub struct SSTableManager {
    sstables: HashMap<FileId, Arc<SSTable>>,
    pending_delete: Vec<FileId>,
}

impl SSTableManager {
    // 主节点：标记删除而非立即删除
    pub fn mark_for_deletion(&mut self, file_id: FileId) {
        self.pending_delete.push(file_id);
        // ✅ 不立即删除文件
    }
    
    // 后台GC：确保没有引用时才删除
    pub async fn garbage_collect(&mut self) {
        for file_id in &self.pending_delete {
            let sstable = &self.sstables[file_id];
            if Arc::strong_count(sstable) == 1 {
                // 只有manager持有引用，可以安全删除
                fs::remove_file(sstable.path)?;
                self.sstables.remove(file_id);
            }
        }
    }
}

// 从节点：只读打开
impl ReplicaNode {
    pub fn open_sstable(&self, file_id: FileId) -> Result<Arc<SSTable>> {
        // 只读模式打开
        SSTable::open_readonly(&path)
    }
}
```

**解决方案2: 版本化文件名**
```rust
// 文件命名包含版本号
// 000001.sst.v1
// 000001.sst.v2
// Compaction产生新版本，不删除旧版本
// 定期GC清理无人使用的旧版本
```

**结论**：
- ⚠️ **可以解决**：通过延迟删除、版本控制
- ⚠️ 增加复杂度，但可行
- ✅ **推荐场景A**（RPC方式）更简单

---

### 问题2: 元数据一致性

**我之前的论述**：
```
❌ 从节点读旧Manifest
❌ 尝试打开已删除的SSTable
```

**重新评估**：

#### 在用户的场景下

```
用户说：节点无状态，只是缓存

这意味着：
├─ 从节点不需要完整的Manifest
├─ 从节点不需要知道所有SSTable
└─ 从节点只需要缓存热数据

因此：
✅ 元数据一致性不是核心问题！
```

**实现**：
```rust
// 主节点：完整元数据
pub struct PrimaryNode {
    db: DB,  // 包含完整Manifest
}

// 从节点：无元数据
pub struct ReplicaNode {
    cache: Cache,  // 只有缓存，没有Manifest
    // ✅ 不需要Manifest！
}
```

**如果确实需要从节点读文件**：
```rust
// 解决方案：定期同步Manifest
impl ReplicaNode {
    // 定期（如每10秒）从主节点同步Manifest
    async fn sync_manifest(&mut self) {
        let manifest = self.primary_client.get_manifest().await?;
        self.manifest = manifest;
        // 允许10秒延迟，可接受
    }
    
    // 读取时处理文件不存在
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        match self.read_from_local(key) {
            Ok(v) => Ok(v),
            Err(Error::FileNotFound) => {
                // 文件已被删除，转发到主节点
                self.primary_client.get(key).await
            }
            Err(e) => Err(e),
        }
    }
}
```

**结论**：
- ✅ **不是问题**：从节点可以不需要Manifest
- ⚠️ 如需要，定期同步 + 容错处理即可

---

### 问题3: WAL读写冲突

**我之前的论述**：
```
❌ 从节点看不到主节点MemTable的数据
❌ 最新数据只在主节点
```

**重新评估**：

```
这根本不是问题，而是设计特性！

用户的场景：
├─ 从节点是缓存层，本来就有延迟
├─ 缓存miss就转发到主节点
└─ 可接受短暂不一致

因此：
✅ 这是预期行为，不是bug！
```

**实现**：
```rust
impl ReplicaNode {
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // 1. 查缓存（可能是旧数据）
        if let Some(v) = self.cache.get(key) {
            // 缓存命中，快速返回
            // ⚠️ 可能不是最新，但用户可接受
            return Ok(Some(v));
        }
        
        // 2. 缓存miss → 转发主节点
        // ✅ 获取最新数据（包括MemTable中的）
        let value = self.primary_client.get(key).await?;
        
        // 3. 更新缓存
        if let Some(ref v) = value {
            self.cache.put(key.clone(), v.clone());
        }
        
        Ok(value)
    }
}
```

**可选优化：缓存失效通知**
```rust
// 主节点写入时，异步通知从节点失效缓存
impl PrimaryNode {
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        // 1. 写入本地
        self.db.put(key, value)?;
        
        // 2. 异步通知从节点（best effort）
        self.invalidate_replicas_cache(key).await;
        // 注意：异步非阻塞，不影响写入性能
        
        Ok(())
    }
}
```

**结论**：
- ✅ **完全不是问题**：这是缓存层的正常行为
- ✅ 最终一致性，符合用户需求

---

### 问题4: 文件系统缓存和锁

**我之前的论述**：
```
❌ RocksDB使用flock，多实例冲突
❌ 页缓存不一致
```

**重新评估**：

#### 方案A: 主节点独占DB，从节点RPC访问 ✅

```rust
// 主节点：独占DB实例
let primary = PrimaryNode {
    db: DB::open("/data/shard1")?,  // 持有文件锁
    // ...
};

// 从节点：不打开DB
let replica = ReplicaNode {
    cache: Cache::new(),
    primary_client: RpcClient::connect("primary:8080"),
    // ✅ 不访问文件，无锁冲突
};
```

**结论**：
- ✅ **不是问题**：从节点不打开DB实例

---

#### 方案B: 从节点只读打开（如果需要）

```rust
// 修改DB的open方法支持只读模式
impl DB {
    pub fn open_readonly(path: &str) -> Result<DB> {
        // 1. 只读模式打开，不获取写锁
        let lock = try_shared_lock(path)?;  // 共享锁而非独占锁
        
        // 2. 禁用写入操作
        let db = DB {
            wal: None,  // 不创建WAL
            memtable: None,  // 不创建MemTable
            sstables: load_sstables_readonly(path)?,
            readonly: true,
        };
        
        Ok(db)
    }
}
```

**页缓存问题**：
```rust
// 使用Direct I/O或显式同步
impl SSTable {
    pub fn open_with_direct_io(&self, path: &str) -> Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECT)  // 绕过页缓存
            .open(path)
    }
}

// 或者：从节点定期invalidate缓存
impl ReplicaNode {
    pub fn sync_cache(&self) {
        // posix_fadvise(POSIX_FADV_DONTNEED)
        // 清空页缓存
    }
}
```

**结论**：
- ✅ **可以解决**：只读模式 + Direct I/O
- ✅ 但方案A（RPC）更简单

---

### 问题5: 共享存储性能瓶颈

**我之前的论述**：
```
❌ 网络存储延迟高、IOPS低
```

**重新评估**：

```
用户明确说了：
"优先使用本地盘，网盘只作为系统级备份"

所以：
✅ 根本不用共享网络存储！
✅ 每个主节点用本地SSD
✅ 网盘只做异步备份
```

**架构澄清**：
```
我之前理解错了：

❌ 我理解的：主从共享一个网络盘
┌──────┐
│Primary│─┐
└──────┘ │
         ├─→ [NFS/EBS] ← 性能瓶颈
┌──────┐ │
│Replica│─┘
└──────┘

✅ 用户实际想要的：
┌──────┐
│Primary│→ [Local SSD] ← 高性能
└──┬───┘       │
   │ RPC       │ 异步备份
   │           ▼
┌──▼───┐   [Network Storage]
│Replica│   (只做备份)
└──────┘
```

**结论**：
- ✅ **完全不是问题**：主节点用本地SSD，高性能
- ✅ 网盘只做异步备份，不在热路径上

---

### 问题6: 单点故障

**我之前的论述**：
```
❌ 共享磁盘故障 → 全部不可用
```

**重新评估**：

用户的风险模型：
```
用户说：
1. 限流措施做得好，大多是意外故障
2. 可通过磁盘异地备份恢复
3. 节点随时可重启，无状态

因此：
├─ 可接受短暂不可用（分钟级恢复）
├─ 通过备份恢复，而非实时高可用
└─ 多shard分散，单shard故障不影响全局
```

**风险评估**：

| 故障 | 影响范围 | 恢复时间 | 是否可接受 |
|------|---------|---------|-----------|
| Primary节点故障 | 单shard写入暂停 | 1-5分钟（重启） | ✅ 可接受 |
| Primary磁盘损坏 | 单shard | 10-30分钟（备份恢复） | ✅ 可接受 |
| Replica故障 | 读能力稍降 | 秒级（启动新节点） | ✅ 可接受 |
| 网络分区 | 部分不可达 | 自愈 | ✅ 可接受 |

**多shard隔离**：
```
100个shard:
├─ 1个shard故障 → 只影响1%数据
├─ 其他99个shard正常服务
└─ 整体可用性 = 1 - (0.01)^100 ≈ 100%
```

**结论**：
- ✅ **风险可接受**：单shard不是单点，有备份可恢复
- ✅ 多shard降低整体风险

---

## 🎯 重新评估结论

### 之前的6个"严重问题"

| 问题 | 实际严重性 | 解决方法 |
|------|-----------|---------|
| 1. 后台写冲突 | ⚠️ 中 | 从节点用RPC，不直接访问文件 ✅ |
| 2. 元数据一致性 | ⚠️ 低 | 从节点不需要Manifest ✅ |
| 3. WAL冲突 | ✅ 不是问题 | 缓存层的正常行为 ✅ |
| 4. 文件锁 | ⚠️ 低 | 只有主节点打开DB ✅ |
| 5. 存储性能 | ✅ 不是问题 | 用本地SSD，网盘只备份 ✅ |
| 6. 单点故障 | ⚠️ 中 | 多shard + 备份，可接受 ✅ |

### 推荐架构（融合方案）

```
┌─────────────────────────────────────────┐
│  Coordinator (路由 + 负载均衡)           │
└────────────┬────────────────────────────┘
             │
     ┌───────┼───────┐
     │       │       │
┌────▼───┐ ┌▼────┐ ┌▼────┐
│Shard 1 │ │Shard2│ │Shard│  ... (多个shard)
└────┬───┘ └─────┘ └─────┘
     │
     ├─ Primary (主节点)
     │  ├─ 本地SSD存储（完整LSM-Tree）
     │  ├─ RPC服务（供Replica调用）
     │  └─ 异步备份到网盘
     │
     └─ Replicas (从节点) x N
        ├─ 内存缓存（热数据）
        ├─ 缓存miss → RPC转发Primary
        └─ 无状态，秒级启动

关键设计：
✅ 主节点独占本地SSD（高性能）
✅ 从节点缓存 + RPC（无文件冲突）
✅ 网盘异步备份（低成本，容灾）
✅ 多shard分散（弹性扩展）
✅ 节点无状态（快速恢复）
```

---

## 📊 方案对比

### 原始共享磁盘方案 vs 我推荐的方案 vs 优化后的方案

| 维度 | 原始共享磁盘 | 我的Raft方案 | 优化融合方案 |
|------|------------|-------------|------------|
| **数据复制** | ✅ 无 | ❌ 全量 | ✅ 无（只备份） |
| **文件冲突** | ❌ 有 | ✅ 无 | ✅ 无（RPC） |
| **扩展性** | ⚠️ 中 | ⚠️ 中 | ✅ 高 |
| **复杂度** | ⚠️ 中 | ❌ 高 | ✅ 中 |
| **性能** | ⚠️ 中 | ⚠️ 中 | ✅ 高 |
| **成本** | ✅ 低 | ❌ 高 | ✅ 低 |
| **一致性** | ⚠️ 最终 | ✅ 强 | ⚠️ 最终 |
| **恢复时间** | ⚠️ 中 | ✅ 快 | ⚠️ 中 |

### 核心差异

**优化融合方案 = 原方案的思想 + 规避文件冲突**

```
借鉴原方案：
✅ 不做全量数据复制
✅ 从节点轻量级（缓存）
✅ 异步备份容灾
✅ 多shard弹性扩展

规避文件冲突：
✅ 从节点不直接访问主节点文件
✅ 通过RPC转发，而非共享磁盘
✅ 主节点独占DB实例
```

---

## ✅ 最终推荐

### 推荐方案：优化融合架构

**核心思想**（完全符合你的需求）：

1. **减少复制开销** ✅
   - 无实时数据复制
   - 从节点只缓存热数据
   - 异步备份到网盘

2. **弹性扩展** ✅
   - 添加shard增加容量
   - 添加replica增加读能力
   - 秒级启动新节点

3. **节点无状态** ✅
   - 从节点只有缓存
   - 随时重启
   - 快速恢复

4. **本地盘 + 网盘** ✅
   - 主节点本地SSD（高性能）
   - 网盘异步备份（低成本）
   - 不在热路径

5. **可接受风险** ✅
   - 最终一致性
   - 短暂数据丢失可接受
   - 多shard降低影响

### 与原方案的唯一差异

```
原方案：从节点直接读主节点的磁盘文件
         ↓
优化方案：从节点通过RPC从主节点读取

理由：
├─ 规避LSM后台操作的文件冲突
├─ 不需要处理文件锁问题
├─ 实现更简单
└─ 性能差异很小（RPC vs 文件IO都是微秒级）
```

---

## 🚀 下一步

### 需要你确认

1. **是否接受"从节点通过RPC而非直接文件访问"**？
   - 优点：无文件冲突，实现简单
   - 缺点：增加一次RPC开销（但很小）

2. **是否确认以下设计**？
   - ✅ 主节点：本地SSD + 完整LSM
   - ✅ 从节点：内存缓存 + RPC转发
   - ✅ 备份：异步到网盘
   - ✅ 扩展：多shard分片

3. **如果确认，我将制定详细实施计划**

---

*这个方案保留了你所有的核心诉求，只是规避了共享文件系统的技术陷阱*
