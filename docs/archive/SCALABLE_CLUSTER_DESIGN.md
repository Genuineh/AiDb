# AiDb 弹性扩展集群设计方案

## 🎯 设计目标（基于用户需求）

### 核心诉求

1. **减少数据复制开销** - 不做实时全量复制
2. **弹性扩展** - 通过增加主从组线性扩展
3. **负载分散** - 多个主从组，每组负载可控
4. **成本优化** - 本地盘性能 + 网盘备份
5. **无状态节点** - 节点可随时重启，状态可丢弃
6. **安全性** - 通过磁盘备份而非实时复制

### 架构哲学

```
传统HA思路：
数据复制 → 多副本 → 强一致性
❌ 复制成本高，扩展困难

我们的思路：
数据分片 → 缓存 + 转发 → 异步备份
✅ 低成本，易扩展
```

---

## 🏗️ 整体架构

### 架构图

```
                    ┌─────────────────────┐
                    │   Coordinator       │
                    │   (路由 + 负载均衡)  │
                    └──────────┬──────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
         ┌────▼─────┐     ┌───▼──────┐    ┌───▼──────┐
         │ Shard 1  │     │ Shard 2  │    │ Shard 3  │
         │ Group    │     │ Group    │    │ Group    │
         └────┬─────┘     └────┬─────┘    └────┬─────┘
              │                │                │
    ┌─────────┴─────────┐      │                │
    │                   │      │                │
┌───▼────┐         ┌───▼────┐  ...             ...
│Primary │         │Replica │
│(主节点) │         │(从节点) │
└───┬────┘         └───┬────┘
    │                  │
┌───▼────┐         ┌───▼────┐
│Local   │         │Cache   │
│SSD     │         │(内存)  │
│(全量)  │         │(热数据) │
└───┬────┘         └────────┘
    │
    │ 异步备份
    ▼
┌────────┐
│Network │
│Storage │
│(快照)  │
└────────┘
```

### 关键设计

**1. 分片架构**
```
数据按key分片 → 多个独立的主从组
每组：
├─ 1个Primary（写入 + 读取）
├─ N个Replica（只读 + 缓存）
└─ 数据完全隔离
```

**2. 主节点（Primary）**
```
存储：本地SSD（全量数据）
功能：
├─ 处理所有写入
├─ 处理读取（如果从节点miss）
└─ 异步备份到网盘
```

**3. 从节点（Replica）**
```
存储：内存缓存（热数据）
功能：
├─ 处理读取请求
├─ 如果缓存miss → 转发到Primary
└─ LRU淘汰冷数据
```

**4. 备份策略**
```
实时：无需实时复制
异步：
├─ 定期快照（如每小时）
├─ 增量备份（WAL归档）
└─ 存储到网盘/对象存储
```

---

## 🔧 详细设计

### 1. 分片策略

**一致性哈希分片**：

```rust
pub struct Coordinator {
    // 一致性哈希环
    hash_ring: ConsistentHashRing,
    // 主从组映射
    shard_groups: HashMap<ShardId, ShardGroup>,
}

pub struct ShardGroup {
    shard_id: ShardId,
    primary: PrimaryNode,
    replicas: Vec<ReplicaNode>,
    // key范围（用于range查询）
    key_range: Option<(Vec<u8>, Vec<u8>)>,
}

impl Coordinator {
    // 路由key到对应shard
    pub fn route(&self, key: &[u8]) -> &ShardGroup {
        let hash = hash_key(key);
        let shard_id = self.hash_ring.get_node(hash);
        &self.shard_groups[&shard_id]
    }
    
    // 添加新shard（弹性扩展）
    pub fn add_shard(&mut self, shard: ShardGroup) {
        self.hash_ring.add_node(shard.shard_id);
        self.shard_groups.insert(shard.shard_id, shard);
        // 无需数据迁移！新数据直接路由到新shard
    }
}
```

**优势**：
- ✅ 添加shard无需迁移数据
- ✅ 只影响部分key的路由
- ✅ 负载自然分散

---

### 2. 主节点（Primary）设计

**数据存储**：

```rust
pub struct PrimaryNode {
    // 本地LSM存储（我们实现的AiDb）
    db: DB,
    
    // 备份管理器
    backup_manager: BackupManager,
    
    // 网络服务（供Replica读取）
    rpc_server: RpcServer,
}

impl PrimaryNode {
    // 写操作
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        // 1. 写入本地SSD
        self.db.put(key, value)?;
        
        // 2. 立即返回（无需等待复制）✅
        Ok(())
        
        // 3. 后台异步备份
        // self.backup_manager会定期备份
    }
    
    // 读操作
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db.get(key)
    }
    
    // RPC服务（供Replica调用）
    pub async fn handle_replica_get(&self, key: &[u8]) 
        -> Result<Option<Vec<u8>>> {
        self.db.get(key)
    }
}
```

**备份策略**：

```rust
pub struct BackupManager {
    primary_db: Arc<DB>,
    backup_storage: BackupStorage, // S3/OSS等
    config: BackupConfig,
}

pub struct BackupConfig {
    // 快照间隔（例如每小时）
    snapshot_interval: Duration,
    
    // WAL归档间隔（例如每10分钟）
    wal_archive_interval: Duration,
    
    // 保留策略
    retention: RetentionPolicy,
}

impl BackupManager {
    // 全量快照
    pub async fn create_snapshot(&self) -> Result<SnapshotId> {
        // 1. 触发LSM checkpoint
        let checkpoint = self.primary_db.checkpoint()?;
        
        // 2. 上传SSTable文件到网盘
        for sstable in checkpoint.sstables {
            self.backup_storage.upload(sstable).await?;
        }
        
        // 3. 上传Manifest
        self.backup_storage.upload(checkpoint.manifest).await?;
        
        // 4. 返回快照ID
        Ok(SnapshotId::new())
    }
    
    // WAL归档（增量备份）
    pub async fn archive_wal(&self) -> Result<()> {
        // 1. 获取自上次归档以来的WAL
        let wal_files = self.primary_db.get_wal_files_since(
            self.last_archive_point
        )?;
        
        // 2. 上传到网盘
        for wal in wal_files {
            self.backup_storage.upload(wal).await?;
        }
        
        Ok(())
    }
    
    // 后台任务
    pub async fn run(&self) {
        loop {
            // 定期快照
            if elapsed > snapshot_interval {
                self.create_snapshot().await?;
            }
            
            // WAL归档
            if elapsed > wal_archive_interval {
                self.archive_wal().await?;
            }
            
            sleep(Duration::from_secs(60)).await;
        }
    }
}
```

---

### 3. 从节点（Replica）设计

**核心理念：缓存层 + 转发**

```rust
pub struct ReplicaNode {
    // 内存缓存（热数据）
    cache: Arc<RwLock<LruCache<Vec<u8>, Vec<u8>>>>,
    
    // Primary节点的RPC客户端
    primary_client: RpcClient,
    
    // 配置
    config: ReplicaConfig,
}

pub struct ReplicaConfig {
    // 缓存大小（例如1GB）
    cache_size: usize,
    
    // 预热策略
    warmup_strategy: WarmupStrategy,
}

impl ReplicaNode {
    // 读操作（只读）
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // 1. 先查缓存
        {
            let cache = self.cache.read();
            if let Some(value) = cache.get(key) {
                return Ok(Some(value.clone())); // ✅ 缓存命中
            }
        }
        
        // 2. 缓存miss → 转发到Primary
        let value = self.primary_client.get(key).await?;
        
        // 3. 更新缓存
        if let Some(ref v) = value {
            let mut cache = self.cache.write();
            cache.put(key.to_vec(), v.clone());
        }
        
        Ok(value)
    }
    
    // 批量预热（启动时）
    pub async fn warmup(&self, keys: &[Vec<u8>]) -> Result<()> {
        for key in keys {
            let value = self.primary_client.get(key).await?;
            if let Some(v) = value {
                let mut cache = self.cache.write();
                cache.put(key.clone(), v);
            }
        }
        Ok(())
    }
    
    // 写操作 → 转发到Primary
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        // 直接转发，不缓存写入
        self.primary_client.put(key, value).await?;
        
        // 可选：使缓存失效
        let mut cache = self.cache.write();
        cache.pop(key);
        
        Ok(())
    }
}
```

**预热策略**：

```rust
pub enum WarmupStrategy {
    // 不预热，懒加载
    Lazy,
    
    // 热key列表预热
    HotKeys(Vec<Vec<u8>>),
    
    // 范围扫描预热
    RangeScan {
        start: Vec<u8>,
        end: Vec<u8>,
        limit: usize,
    },
    
    // 从备份加载（可选）
    FromBackup {
        snapshot_id: SnapshotId,
    },
}

impl ReplicaNode {
    pub async fn apply_warmup(&self) -> Result<()> {
        match &self.config.warmup_strategy {
            WarmupStrategy::Lazy => Ok(()),
            
            WarmupStrategy::HotKeys(keys) => {
                self.warmup(keys).await
            }
            
            WarmupStrategy::RangeScan { start, end, limit } => {
                let keys = self.primary_client
                    .scan(start, end, *limit).await?;
                self.warmup(&keys).await
            }
            
            WarmupStrategy::FromBackup { snapshot_id } => {
                // 从网盘下载部分热数据
                self.load_from_backup(snapshot_id).await
            }
        }
    }
}
```

---

### 4. 协调器（Coordinator）设计

**负载均衡和路由**：

```rust
pub struct Coordinator {
    shard_groups: Arc<RwLock<HashMap<ShardId, ShardGroup>>>,
    hash_ring: Arc<RwLock<ConsistentHashRing>>,
    
    // 负载监控
    metrics: Arc<Metrics>,
}

impl Coordinator {
    // 写入（路由到Primary）
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        // 1. 找到对应shard
        let shard = self.route(key);
        
        // 2. 写入Primary
        shard.primary.put(key, value).await
    }
    
    // 读取（负载均衡到Replica）
    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        // 1. 找到对应shard
        let shard = self.route(key);
        
        // 2. 负载均衡选择节点
        let node = self.select_read_node(shard).await;
        
        // 3. 读取
        node.get(key).await
    }
    
    // 负载均衡策略
    async fn select_read_node(&self, shard: &ShardGroup) 
        -> &dyn ReadNode {
        // 策略1: 轮询
        // 策略2: 最少连接
        // 策略3: 响应时间
        
        // 简单实现：随机选择Replica，如果没有则用Primary
        if !shard.replicas.is_empty() {
            let idx = rand::random::<usize>() % shard.replicas.len();
            &shard.replicas[idx]
        } else {
            &shard.primary
        }
    }
}
```

---

## 📊 性能分析

### 读写性能

**写入路径**：
```
Client → Coordinator → Primary → Local SSD
延迟：< 1ms（本地SSD）
吞吐：50K-100K ops/s per shard
```

**读取路径（缓存命中）**：
```
Client → Coordinator → Replica → Cache → Return
延迟：< 0.1ms（内存）
吞吐：500K+ ops/s per replica
```

**读取路径（缓存miss）**：
```
Client → Coordinator → Replica → Primary → Local SSD
延迟：< 2ms（RPC + SSD）
吞吐：30K-50K ops/s per replica
```

### 扩展性分析

**线性扩展**：

```
1个Shard Group:
├─ 写入：50K ops/s
└─ 读取：50K ops/s (primary) + 500K ops/s per replica

10个Shard Groups:
├─ 写入：500K ops/s (10x)
└─ 读取：5M+ ops/s (10x primary + 100x replica)

100个Shard Groups:
├─ 写入：5M ops/s (100x)
└─ 读取：50M+ ops/s
```

**成本对比**：

| 方案 | 数据量 | 存储成本 | 复制开销 |
|------|-------|---------|---------|
| **全量复制** | 100GB × 3副本 | 300GB | ❌ 高 |
| **我们的方案** | 100GB + 缓存10GB×10 | 200GB | ✅ 低 |

---

## 🔄 弹性伸缩

### 1. 添加新Shard（扩展写能力）

```rust
impl Coordinator {
    pub async fn add_shard_group(&mut self, 
                                  primary: PrimaryNode) -> Result<()> {
        let shard_id = ShardId::new();
        
        // 1. 创建新的shard group
        let shard_group = ShardGroup {
            shard_id,
            primary,
            replicas: vec![],
            key_range: None,
        };
        
        // 2. 加入hash ring
        self.hash_ring.write().add_node(shard_id);
        
        // 3. 注册shard
        self.shard_groups.write()
            .insert(shard_id, shard_group);
        
        // 4. 无需数据迁移！✅
        // 新写入的key会自动路由到新shard
        
        Ok(())
    }
}
```

**优势**：
- ✅ 无需停机
- ✅ 无需数据迁移
- ✅ 即时生效

### 2. 添加Replica（扩展读能力）

```rust
impl ShardGroup {
    pub async fn add_replica(&mut self, 
                             replica: ReplicaNode) -> Result<()> {
        // 1. 可选：预热缓存
        replica.warmup_from_primary(&self.primary).await?;
        
        // 2. 加入replica列表
        self.replicas.push(replica);
        
        // 3. 立即可服务
        Ok(())
    }
}
```

**优势**：
- ✅ 秒级添加
- ✅ 预热可选（懒加载也行）
- ✅ 线性扩展读能力

### 3. 移除节点

```rust
impl Coordinator {
    // 移除Replica（无影响）
    pub async fn remove_replica(&mut self, 
                                shard_id: ShardId, 
                                replica_id: ReplicaId) -> Result<()> {
        let shard = self.shard_groups.write()
            .get_mut(&shard_id).unwrap();
        
        shard.replicas.retain(|r| r.id != replica_id);
        // ✅ 无需任何数据操作
        Ok(())
    }
    
    // 移除Shard（需要迁移）
    pub async fn remove_shard(&mut self, shard_id: ShardId) 
        -> Result<()> {
        // 1. 停止路由新请求到此shard
        self.hash_ring.write().remove_node(shard_id);
        
        // 2. 等待现有请求完成
        self.drain_shard(shard_id).await?;
        
        // 3. 可选：备份数据
        let shard = self.shard_groups.read()
            .get(&shard_id).unwrap();
        shard.primary.backup_manager.create_snapshot().await?;
        
        // 4. 删除shard
        self.shard_groups.write().remove(&shard_id);
        
        Ok(())
    }
}
```

---

## 🛡️ 高可用和恢复

### 1. Primary节点故障

**场景**：Primary崩溃或重启

```rust
impl Coordinator {
    async fn handle_primary_failure(&mut self, shard_id: ShardId) 
        -> Result<()> {
        let shard = self.shard_groups.write()
            .get_mut(&shard_id).unwrap();
        
        // 策略1: 快速恢复（推荐）
        // Primary重启，从备份恢复
        let primary = PrimaryNode::recover_from_backup(
            &shard.primary.backup_storage,
            SnapshotId::latest()
        ).await?;
        
        // 策略2: 如果有多个Primary（可选）
        // 提升一个Replica为Primary（需要从备份加载全量数据）
        
        shard.primary = primary;
        Ok(())
    }
}

impl PrimaryNode {
    // 从备份恢复
    pub async fn recover_from_backup(
        backup_storage: &BackupStorage,
        snapshot_id: SnapshotId
    ) -> Result<Self> {
        // 1. 下载最新快照
        let snapshot = backup_storage.download_snapshot(snapshot_id).await?;
        
        // 2. 下载快照后的WAL（增量）
        let wals = backup_storage.download_wals_since(snapshot_id).await?;
        
        // 3. 恢复本地DB
        let db = DB::recover(snapshot, wals)?;
        
        // 4. 创建Primary节点
        Ok(PrimaryNode::new(db))
    }
}
```

**恢复时间**：
- 小数据集（<10GB）：1-5分钟
- 中等数据集（10-100GB）：5-30分钟
- 大数据集（>100GB）：按需分片，每个shard独立恢复

**影响范围**：
- 只影响该shard的写入
- 读取可从其他shard的replica继续服务
- 其他shard完全不受影响 ✅

### 2. Replica节点故障

```rust
impl ShardGroup {
    async fn handle_replica_failure(&mut self, replica_id: ReplicaId) {
        // 1. 从列表中移除
        self.replicas.retain(|r| r.id != replica_id);
        
        // 2. 读取自动路由到其他replica或primary
        // 无需任何恢复操作 ✅
        
        // 3. 可选：启动新replica
        if self.replicas.len() < MIN_REPLICAS {
            let new_replica = ReplicaNode::new();
            self.add_replica(new_replica).await;
        }
    }
}
```

**恢复时间**：秒级（启动新容器/进程）

**影响**：几乎无影响，只是读取能力稍降

### 3. 数据丢失恢复

**场景**：Primary的本地SSD故障，数据完全丢失

```rust
// 灾难恢复流程
pub async fn disaster_recovery(
    shard_id: ShardId,
    backup_storage: &BackupStorage
) -> Result<PrimaryNode> {
    // 1. 从网盘下载最新备份
    let snapshot = backup_storage.latest_snapshot().await?;
    let wals = backup_storage.wals_since(snapshot.id).await?;
    
    // 2. 在新机器上恢复
    let db = DB::recover(snapshot, wals)?;
    
    // 3. 启动新Primary
    let primary = PrimaryNode::new(db);
    
    // 数据丢失：仅备份间隔内的数据（如1小时）
    // 通过更频繁备份可减少丢失窗口
    
    Ok(primary)
}
```

**数据丢失窗口**：
- 快照间隔：1小时 → 最多丢失1小时数据
- WAL归档：10分钟 → 最多丢失10分钟数据
- 实时WAL备份：< 1分钟 → 几乎无丢失

---

## 💰 成本分析

### 存储成本

**假设**：1TB数据，3个replica（传统方案）

| 方案 | 本地SSD | 网盘 | 总成本 |
|------|---------|------|--------|
| **传统复制** | 3TB × $0.5/GB = $1500 | 0 | **$1500** |
| **我们的方案** | 1TB × $0.5/GB = $500 | 1TB × $0.1/GB = $100 | **$600** ✅ |

**节省**：60%

### 网络成本

**写入**：
- 传统：主→从1 + 主→从2 = 2倍网络
- 我们：主→网盘（异步，压缩）= 0.1倍网络 ✅

**读取**：
- 传统：客户端→任意节点（0成本）
- 我们：replica→primary（miss时）= 小额成本

**总体**：网络成本降低80%+

---

## 🔧 实施计划

### 阶段1: 单机版（已规划，Week 1-20）

```
完成基础AiDb引擎
├─ WAL + MemTable + SSTable
├─ Compaction
└─ 完整功能
```

### 阶段2: 分片基础（Week 21-26）⭐

```
Week 21-22: Coordinator
├─ 一致性哈希
├─ 路由逻辑
└─ RPC框架（tonic/gRPC）

Week 23-24: Primary节点网络层
├─ RPC服务端
├─ Get/Put接口
└─ 健康检查

Week 25-26: 基础测试
├─ 单shard测试
├─ 多shard测试
└─ 性能基准
```

### 阶段3: Replica和缓存（Week 27-32）

```
Week 27-28: Replica节点
├─ LRU缓存实现
├─ RPC客户端
└─ 转发逻辑

Week 29-30: 预热策略
├─ 热key识别
├─ 批量预热
└─ 懒加载

Week 31-32: 负载均衡
├─ 读请求路由
├─ 监控指标
└─ 性能测试
```

### 阶段4: 备份和恢复（Week 33-38）

```
Week 33-34: 备份管理器
├─ 快照创建
├─ WAL归档
└─ 对象存储集成（S3/OSS）

Week 35-36: 恢复机制
├─ 快照恢复
├─ WAL回放
└─ 增量恢复

Week 37-38: 自动化
├─ 定时备份
├─ 自动清理
└─ 监控告警
```

### 阶段5: 弹性伸缩（Week 39-44）

```
Week 39-40: 动态扩展
├─ 添加shard
├─ 添加replica
└─ 移除节点

Week 41-42: 负载监控
├─ 指标收集
├─ 自动伸缩（可选）
└─ Dashboard

Week 43-44: 完整测试
├─ 压力测试
├─ 故障注入
└─ 长期稳定性测试
```

---

## 📊 关键指标

### 性能目标

| 指标 | 单shard | 10 shards | 100 shards |
|------|---------|-----------|------------|
| 写入 | 50K/s | 500K/s | 5M/s |
| 读取(cache) | 500K/s per replica | 5M/s | 50M/s |
| 读取(miss) | 50K/s | 500K/s | 5M/s |
| 延迟(写) | <1ms | <2ms | <3ms |
| 延迟(读cache) | <0.1ms | <0.2ms | <0.5ms |

### 成本目标

- 存储成本：降低50-60%
- 网络成本：降低80%+
- 整体TCO：降低40-50%

### 可用性目标

- 单shard可用性：99.9%（快速恢复）
- 整体可用性：99.99%（多shard隔离）
- 数据丢失窗口：<10分钟（WAL归档）

---

## ✅ 方案优势总结

### vs 传统复制方案

| 维度 | 传统复制 | 我们的方案 | 优势 |
|------|---------|-----------|------|
| 数据复制 | 全量实时 | 无需复制 | ✅ 成本低 |
| 存储成本 | 3x | 1.2x | ✅ 降低60% |
| 网络成本 | 高 | 低 | ✅ 降低80% |
| 扩展性 | 复制瓶颈 | 线性扩展 | ✅ 更好 |
| 添加节点 | 慢（需复制） | 快（秒级） | ✅ 更快 |
| 一致性 | 强 | 最终 | ⚠️ 权衡 |

### vs 共享磁盘方案

| 维度 | 共享磁盘 | 我们的方案 | 优势 |
|------|---------|-----------|------|
| 文件冲突 | ❌ 有 | ✅ 无 | ✅ 无冲突 |
| 性能 | ⚠️ 中 | ✅ 高 | ✅ 本地SSD |
| 单点故障 | ❌ 是 | ✅ 否 | ✅ 更可靠 |
| 扩展性 | ⚠️ 差 | ✅ 好 | ✅ 更好 |

---

## 总结

### 核心思想

```
不是"复制数据"，而是"分散数据"
不是"强一致"，而是"弹性可恢复"
不是"实时备份"，而是"异步快照"
不是"重状态"，而是"轻状态缓存"
```

### 适用场景

✅ **适合**：
- 读多写少的场景
- 可接受短时间数据丢失（分钟级）
- 需要大规模扩展
- 成本敏感

⚠️ **不适合**：
- 金融交易等需要强一致性
- 不能接受任何数据丢失
- 单机性能足够的场景

### 下一步

1. **确认方案**：是否符合你的需求？
2. **调整细节**：有需要修改的地方？
3. **开始实施**：从哪个阶段开始？

---

*这个方案完全基于你的需求设计，避免了数据复制成本，实现了真正的弹性扩展！*
