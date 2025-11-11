# AiDb 最佳实践

本文档总结了使用 AiDb 的最佳实践，帮助您构建高性能、可靠的应用。

## 目录

- [设计模式](#设计模式)
- [性能最佳实践](#性能最佳实践)
- [可靠性最佳实践](#可靠性最佳实践)
- [运维最佳实践](#运维最佳实践)
- [常见陷阱](#常见陷阱)
- [生产环境部署](#生产环境部署)

---

## 设计模式

### 键设计

#### ✅ 推荐的键设计

```rust
// 使用前缀组织不同类型的数据
db.put(b"user:1001", b"alice")?;
db.put(b"user:1002", b"bob")?;
db.put(b"order:5001", b"order_data")?;
db.put(b"session:xyz", b"session_data")?;

// 使用分隔符便于范围查询
db.put(b"metric:cpu:20231115:120000", b"75.5")?;
db.put(b"metric:mem:20231115:120000", b"60.2")?;

// 对于时间序列，使用固定长度的时间戳
let timestamp = format!("{:020}", epoch_seconds); // 固定20位
let key = format!("log:{}:{}", timestamp, log_id);
```

#### ❌ 避免的键设计

```rust
// 不要使用随机或无序的键（影响 Compaction 效率）
db.put(&uuid::Uuid::new_v4().as_bytes(), b"value")?; // 不推荐

// 不要使用过长的键（浪费空间和性能）
let key = "very_long_key_" * 100; // 不推荐

// 不要在键中包含不必要的信息
db.put(b"user:1001:name:alice:age:25", b"data")?; // 不推荐
// 应该：db.put(b"user:1001", b"serialized_user_data")?; // 推荐
```

#### 键设计原则

1. **使用前缀分组**：便于范围查询和组织
2. **保持简短**：键越短，性能越好
3. **固定长度更好**：对于数字ID，使用固定长度编码
4. **考虑排序**：设计键时考虑排序需求
5. **可读性**：在合理范围内保持键的可读性

### 值设计

#### ✅ 推荐的值设计

```rust
use serde::{Serialize, Deserialize};

// 使用二进制序列化（bincode 或 MessagePack）
#[derive(Serialize, Deserialize)]
struct User {
    name: String,
    email: String,
    created_at: u64,
}

let user = User { /* ... */ };
let value = bincode::serialize(&user)?;
db.put(b"user:1001", &value)?;

// 对于大对象，考虑分块存储
fn store_large_object(db: &DB, id: &str, data: &[u8]) -> Result<()> {
    const CHUNK_SIZE: usize = 1024 * 1024; // 1MB chunks
    
    for (i, chunk) in data.chunks(CHUNK_SIZE).enumerate() {
        let key = format!("object:{}:chunk:{}", id, i);
        db.put(key.as_bytes(), chunk)?;
    }
    
    // 存储元数据
    let meta = format!("chunks:{}", (data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE);
    db.put(format!("object:{}:meta", id).as_bytes(), meta.as_bytes())?;
    Ok(())
}
```

#### ❌ 避免的值设计

```rust
// 不要存储超大值（> 10MB）在单个键中
db.put(b"large_file", &huge_data)?; // 不推荐

// 不要使用 JSON（除非需要人类可读性）
let json = serde_json::to_string(&user)?; // 比 bincode 慢且大
db.put(b"user:1001", json.as_bytes())?; // 不推荐
```

### 事务模式

#### 使用 WriteBatch 实现原子操作

```rust
use aidb::WriteBatch;

// 转账操作：原子性保证
fn transfer(db: &DB, from: &str, to: &str, amount: u64) -> Result<()> {
    // 读取余额
    let from_key = format!("balance:{}", from);
    let to_key = format!("balance:{}", to);
    
    let from_balance: u64 = get_balance(db, &from_key)?;
    let to_balance: u64 = get_balance(db, &to_key)?;
    
    // 检查余额
    if from_balance < amount {
        return Err(Error::InvalidArgument("Insufficient balance".into()));
    }
    
    // 使用 WriteBatch 原子更新
    let mut batch = WriteBatch::new();
    batch.put(from_key.as_bytes(), &(from_balance - amount).to_le_bytes());
    batch.put(to_key.as_bytes(), &(to_balance + amount).to_le_bytes());
    
    db.write(batch)?;
    Ok(())
}
```

#### 使用 Snapshot 实现一致性读取

```rust
// 生成报表：确保数据一致性
fn generate_report(db: &DB) -> Result<Report> {
    let snapshot = db.snapshot();
    
    // 所有读取都基于同一时间点
    let total_users = count_prefix(&snapshot, b"user:")?;
    let total_orders = count_prefix(&snapshot, b"order:")?;
    let revenue = calculate_revenue(&snapshot)?;
    
    Ok(Report {
        users: total_users,
        orders: total_orders,
        revenue,
    })
}
```

---

## 性能最佳实践

### 写入优化

#### 1. 批量写入

```rust
// ✅ 推荐：使用 WriteBatch
let mut batch = WriteBatch::new();
for i in 0..10000 {
    batch.put(format!("key{}", i).as_bytes(), b"value");
}
db.write(batch)?;

// ❌ 避免：逐个写入
for i in 0..10000 {
    db.put(format!("key{}", i).as_bytes(), b"value")?; // 慢！
}
```

**提升幅度**：10-100x

#### 2. 合理设置 MemTable 大小

```rust
let mut options = Options::default();

// 写入密集型：增大 MemTable
options.memtable_size = 16 * 1024 * 1024; // 16MB

// 读取密集型：使用默认值
options.memtable_size = 4 * 1024 * 1024; // 4MB
```

#### 3. 延迟 Compaction（高写入负载时）

```rust
// 在高峰期暂停后台 compaction
// 注意：这会累积更多的 SSTable 文件

// 在低峰期手动触发 compaction
db.compact_range(None, None)?;
```

### 读取优化

#### 1. 启用 Block Cache

```rust
let mut options = Options::default();

// 根据工作集大小设置
options.block_cache_size = 256 * 1024 * 1024; // 256MB

// 监控缓存命中率
let stats = db.get_stats()?;
println!("Cache hit rate: {:.2}%", 
         stats.block_cache_hit_rate * 100.0);
```

#### 2. 使用 Bloom Filter

```rust
let mut options = Options::default();

// 启用 Bloom Filter（推荐）
options.enable_bloom_filter = true;

// 调整误判率（bits per key 越大，误判率越低）
options.bloom_filter_bits_per_key = 10; // ~1% 误判率
options.bloom_filter_bits_per_key = 15; // ~0.1% 误判率
```

**效果**：对于不存在的键，可减少 90%+ 的磁盘读取

#### 3. 预热缓存

```rust
// 应用启动时预热热点数据
fn warmup_cache(db: &DB) -> Result<()> {
    // 读取最常访问的数据
    let hot_keys = vec![b"config", b"user:1", b"user:2"];
    for key in hot_keys {
        let _ = db.get(key)?;
    }
    Ok(())
}
```

#### 4. 使用范围查询替代多次点查询

```rust
// ✅ 推荐：范围查询
let mut iter = db.scan(Some(b"user:1000"), Some(b"user:2000"))?;
while iter.valid() {
    // 处理数据
    iter.next();
}

// ❌ 避免：多次点查询
for i in 1000..2000 {
    let key = format!("user:{}", i);
    db.get(key.as_bytes())?;
}
```

### 压缩优化

#### 选择合适的压缩算法

```rust
let mut options = Options::default();

// Snappy：平衡压缩率和速度（推荐）
options.compression_type = CompressionType::Snappy;

// LZ4：更快，但压缩率稍低
options.compression_type = CompressionType::LZ4;

// 不压缩：适合已压缩的数据（如图片、视频）
options.enable_compression = false;
```

---

## 可靠性最佳实践

### 错误处理

#### 正确处理错误

```rust
use aidb::Error;

fn safe_write(db: &DB, key: &[u8], value: &[u8]) -> Result<(), Error> {
    match db.put(key, value) {
        Ok(_) => Ok(()),
        Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::NoSpace => {
            // 磁盘空间不足，触发清理
            cleanup_old_data(db)?;
            // 重试
            db.put(key, value)
        }
        Err(e) => {
            log::error!("Write failed: {}", e);
            Err(e)
        }
    }
}
```

#### 实现重试逻辑

```rust
use std::thread;
use std::time::Duration;

fn retry_write(db: &DB, key: &[u8], value: &[u8], max_retries: u32) -> Result<()> {
    let mut attempt = 0;
    loop {
        match db.put(key, value) {
            Ok(_) => return Ok(()),
            Err(e) if attempt < max_retries => {
                attempt += 1;
                log::warn!("Write failed (attempt {}): {}", attempt, e);
                thread::sleep(Duration::from_millis(100 * attempt as u64));
            }
            Err(e) => return Err(e),
        }
    }
}
```

### 数据备份

#### 使用快照进行备份

```rust
use std::fs;
use std::path::Path;

fn backup_database(db: &DB, backup_path: &Path) -> Result<()> {
    // 创建快照
    let snapshot = db.snapshot();
    
    // 创建备份目录
    fs::create_dir_all(backup_path)?;
    
    // 遍历所有数据并备份
    let mut iter = snapshot.iter();
    let backup_file = backup_path.join("backup.db");
    let backup_db = DB::open(&backup_file, Options::default())?;
    
    while iter.valid() {
        backup_db.put(iter.key(), iter.value())?;
        iter.next();
    }
    
    backup_db.flush()?;
    Ok(())
}
```

#### 定期备份策略

```rust
use std::time::{SystemTime, UNIX_EPOCH};

fn automated_backup() {
    loop {
        thread::sleep(Duration::from_secs(3600)); // 每小时
        
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let backup_path = format!("/backups/db_{}", timestamp);
        
        match backup_database(&db, Path::new(&backup_path)) {
            Ok(_) => log::info!("Backup successful: {}", backup_path),
            Err(e) => log::error!("Backup failed: {}", e),
        }
        
        // 清理旧备份
        cleanup_old_backups("/backups", 7)?; // 保留7天
    }
}
```

### 监控和日志

#### 添加关键操作日志

```rust
use log::{info, warn, error};

fn logged_operation(db: &DB) -> Result<()> {
    info!("Starting operation");
    
    let start = std::time::Instant::now();
    let result = db.put(b"key", b"value");
    let duration = start.elapsed();
    
    match result {
        Ok(_) => {
            info!("Operation completed in {:?}", duration);
            Ok(())
        }
        Err(e) => {
            error!("Operation failed after {:?}: {}", duration, e);
            Err(e)
        }
    }
}
```

#### 监控数据库健康状态

```rust
fn monitor_db_health(db: &DB) {
    loop {
        thread::sleep(Duration::from_secs(60));
        
        // 检查统计信息
        let stats = db.get_stats().unwrap();
        
        if stats.block_cache_hit_rate < 0.8 {
            warn!("Low cache hit rate: {:.2}%", 
                  stats.block_cache_hit_rate * 100.0);
        }
        
        if stats.num_sstables > 100 {
            warn!("Too many SSTables: {}, consider compaction", 
                  stats.num_sstables);
        }
        
        info!("DB Stats: {} SSTables, {:.2}% cache hit", 
              stats.num_sstables, 
              stats.block_cache_hit_rate * 100.0);
    }
}
```

---

## 运维最佳实践

### 容量规划

#### 估算存储需求

```
总存储 = 原始数据 × (1 + WAL开销 + 压缩系数 + 空间放大)

其中：
- WAL开销：~10-20%
- 压缩系数：Snappy约 0.5-0.7（根据数据类型）
- 空间放大：LSM-Tree约 1.1-1.3（Compaction 期间）

示例：100GB 原始数据
总存储 ≈ 100GB × (1 + 0.15 + 0.6 + 0.2) = 195GB
建议预留：250GB（含增长空间）
```

#### 监控磁盘使用

```rust
use std::fs;

fn check_disk_space(db_path: &Path) -> Result<()> {
    let metadata = fs::metadata(db_path)?;
    let size = get_dir_size(db_path)?;
    
    let available = get_available_space(db_path)?;
    
    if available < size * 2 {
        warn!("Low disk space: {} available, {} used", 
              available, size);
    }
    
    Ok(())
}
```

### 性能调优

#### 基准测试

```rust
use std::time::Instant;

fn benchmark_operations(db: &DB) {
    // 写入基准
    let start = Instant::now();
    for i in 0..10000 {
        db.put(format!("bench:write:{}", i).as_bytes(), b"value").unwrap();
    }
    let write_duration = start.elapsed();
    println!("Write: {:.2} ops/s", 10000.0 / write_duration.as_secs_f64());
    
    // 读取基准
    let start = Instant::now();
    for i in 0..10000 {
        db.get(format!("bench:write:{}", i).as_bytes()).unwrap();
    }
    let read_duration = start.elapsed();
    println!("Read: {:.2} ops/s", 10000.0 / read_duration.as_secs_f64());
}
```

### 故障恢复

#### 从 WAL 恢复

```rust
// AiDb 自动从 WAL 恢复，但可以验证
fn verify_recovery(db_path: &Path) -> Result<()> {
    // 打开数据库会自动应用 WAL
    let db = DB::open(db_path, Options::default())?;
    
    // 验证关键数据
    if db.get(b"critical_key")?.is_none() {
        return Err(Error::Corruption("Critical data missing".into()));
    }
    
    Ok(())
}
```

---

## 常见陷阱

### 1. 忘记关闭数据库

```rust
// ❌ 错误：可能丢失数据
{
    let db = DB::open("./data", Options::default())?;
    db.put(b"key", b"value")?;
    // db 在这里被 drop，但可能未 flush
}

// ✅ 正确：显式关闭
{
    let db = DB::open("./data", Options::default())?;
    db.put(b"key", b"value")?;
    db.close()?; // 确保 flush
}
```

### 2. 在迭代时修改数据

```rust
// ❌ 错误：可能导致不一致
let mut iter = db.iter();
while iter.valid() {
    db.delete(iter.key())?; // 危险！
    iter.next();
}

// ✅ 正确：先收集键，再删除
let mut keys_to_delete = Vec::new();
let mut iter = db.iter();
while iter.valid() {
    keys_to_delete.push(iter.key().to_vec());
    iter.next();
}

for key in keys_to_delete {
    db.delete(&key)?;
}
```

### 3. 不处理 Snapshot 生命周期

```rust
// ❌ 错误：Snapshot 阻止 Compaction
fn bad_snapshot_usage(db: &DB) {
    let snapshot = db.snapshot();
    
    // 长时间持有 snapshot
    thread::sleep(Duration::from_secs(3600));
    
    // 这会阻止旧版本被清理
}

// ✅ 正确：及时释放 Snapshot
fn good_snapshot_usage(db: &DB) -> Result<Vec<u8>> {
    let snapshot = db.snapshot();
    let data = snapshot.get(b"key")?.unwrap();
    drop(snapshot); // 显式释放
    Ok(data)
}
```

### 4. 过小的 MemTable

```rust
// ❌ 错误：频繁 flush 影响性能
options.memtable_size = 1024 * 1024; // 1MB 太小

// ✅ 正确：合理大小
options.memtable_size = 4 * 1024 * 1024; // 4MB
// 或对于写密集场景
options.memtable_size = 16 * 1024 * 1024; // 16MB
```

---

## 生产环境部署

### 系统配置

#### 文件描述符限制

```bash
# 增加文件描述符限制
# /etc/security/limits.conf
* soft nofile 65536
* hard nofile 65536

# 验证
ulimit -n
```

#### 磁盘配置

```bash
# 使用 XFS 或 ext4 文件系统
# 禁用 atime（提升性能）
mount -o noatime,nodiratime /dev/sda1 /data

# 使用 SSD 并启用 TRIM
fstrim -v /data
```

### 应用配置

#### 生产环境推荐配置

```rust
let mut options = Options::default();

// MemTable 大小：根据内存调整
options.memtable_size = 16 * 1024 * 1024; // 16MB

// Block Cache：使用可用内存的 30-50%
options.block_cache_size = 512 * 1024 * 1024; // 512MB

// 启用压缩
options.enable_compression = true;
options.compression_type = CompressionType::Snappy;

// 启用 Bloom Filter
options.enable_bloom_filter = true;
options.bloom_filter_bits_per_key = 10;

// 调整 SSTable 大小
options.sstable_size = 64 * 1024 * 1024; // 64MB
```

### 监控指标

#### 关键指标

```rust
struct DBMetrics {
    // 操作延迟
    write_latency_p99: f64,
    read_latency_p99: f64,
    
    // 吞吐量
    writes_per_sec: u64,
    reads_per_sec: u64,
    
    // 缓存
    block_cache_hit_rate: f64,
    
    // 存储
    num_sstables: usize,
    disk_usage: u64,
    
    // 错误率
    error_rate: f64,
}

fn collect_metrics(db: &DB) -> DBMetrics {
    // 实现指标收集
    // ...
}
```

### 告警规则

```yaml
# 示例告警配置
alerts:
  - name: HighWriteLatency
    condition: write_latency_p99 > 100ms
    severity: warning
    
  - name: LowCacheHitRate
    condition: block_cache_hit_rate < 0.7
    severity: warning
    
  - name: TooManySSTables
    condition: num_sstables > 100
    severity: warning
    
  - name: DiskSpaceLow
    condition: disk_usage > 0.8 * disk_capacity
    severity: critical
```

---

## 总结

### 核心原则

1. **批量操作**：使用 WriteBatch 提高写入性能
2. **合理缓存**：根据工作负载调整 Block Cache
3. **键值设计**：前缀组织、保持简短
4. **错误处理**：实现重试和降级策略
5. **监控告警**：实时监控关键指标
6. **定期备份**：保护数据安全
7. **性能测试**：生产前充分压测

### 进一步学习

- [性能调优指南](PERFORMANCE_TUNING.md) - 详细的性能优化技巧
- [用户指南](USER_GUIDE.md) - 完整的功能使用说明
- [架构设计](ARCHITECTURE.md) - 深入了解内部实现
- [examples/](../examples/) - 更多实战示例

---

**有疑问？** 欢迎在 [GitHub Discussions](https://github.com/yourusername/aidb/discussions) 讨论！
