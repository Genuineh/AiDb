# AiDb 示例代码

本目录包含 AiDb 的各种使用示例，从基础操作到高级功能。

## 目录

- [运行示例](#运行示例)
- [示例列表](#示例列表)
- [基础示例](#基础示例)
- [高级示例](#高级示例)

---

## 运行示例

### 前提条件

```bash
# 确保已安装 Rust 1.70+
rustc --version

# 克隆仓库
git clone https://github.com/yourusername/aidb.git
cd aidb
```

### 运行单个示例

```bash
# 基础用法
cargo run --example basic

# 数据库示例
cargo run --example db_example

# WAL 示例
cargo run --example wal_example

# MemTable 示例
cargo run --example memtable_example

# SSTable 示例
cargo run --example sstable_example

# Flush 示例
cargo run --example flush_example

# Bloom Filter 示例
cargo run --example bloom_filter_example

# 完整功能示例（Week 13-14）
cargo run --example week13_14_features
```

### 清理测试数据

示例会在当前目录创建测试数据，运行后可以清理：

```bash
# 删除测试数据
rm -rf ./example_data ./test_db ./test_wal ./test_sstable ./my_database
```

---

## 示例列表

### 基础操作

| 示例 | 文件 | 说明 |
|------|------|------|
| 基础用法 | [basic.rs](basic.rs) | 打开数据库、读写删除 |
| 数据库操作 | [db_example.rs](db_example.rs) | 完整的数据库操作示例 |

### 核心组件

| 示例 | 文件 | 说明 |
|------|------|------|
| WAL | [wal_example.rs](wal_example.rs) | Write-Ahead Log 使用 |
| MemTable | [memtable_example.rs](memtable_example.rs) | 内存表操作 |
| SSTable | [sstable_example.rs](sstable_example.rs) | 持久化表操作 |

### 高级功能

| 示例 | 文件 | 说明 |
|------|------|------|
| Flush | [flush_example.rs](flush_example.rs) | 刷新内存数据到磁盘 |
| Tombstone | [tombstone_flush_example.rs](tombstone_flush_example.rs) | 删除标记和刷新 |
| Bloom Filter | [bloom_filter_example.rs](bloom_filter_example.rs) | Bloom Filter 使用 |
| Week 13-14 功能 | [week13_14_features.rs](week13_14_features.rs) | 快照、迭代器、WriteBatch |

---

## 基础示例

### 1. 最简单的使用 (basic.rs)

演示基本的打开、读、写、删操作。

```rust
use aidb::{DB, Options};

let db = DB::open("./data", Options::default())?;
db.put(b"key", b"value")?;
let value = db.get(b"key")?;
db.delete(b"key")?;
db.close()?;
```

**学习要点**：
- 如何打开数据库
- 基本的 CRUD 操作
- 正确关闭数据库

### 2. 数据库操作 (db_example.rs)

完整的数据库使用示例，包括配置和错误处理。

```rust
let mut options = Options::default();
options.memtable_size = 4 * 1024 * 1024; // 4MB
options.enable_bloom_filter = true;

let db = DB::open("./my_db", options)?;

// 写入
db.put(b"user:1", b"Alice")?;
db.put(b"user:2", b"Bob")?;

// 读取
if let Some(value) = db.get(b"user:1")? {
    println!("User: {}", String::from_utf8_lossy(&value));
}

// 批量操作
use aidb::WriteBatch;
let mut batch = WriteBatch::new();
batch.put(b"user:3", b"Charlie");
batch.delete(b"user:2");
db.write(batch)?;
```

**学习要点**：
- 配置选项
- 批量写入
- 错误处理

---

## 高级示例

### 3. WAL 使用 (wal_example.rs)

演示 Write-Ahead Log 的使用，了解持久化机制。

**学习要点**：
- WAL 工作原理
- 崩溃恢复
- 性能权衡

### 4. Flush 机制 (flush_example.rs)

演示 MemTable 刷新到 SSTable 的过程。

```rust
// 写入大量数据
for i in 0..10000 {
    db.put(format!("key{}", i).as_bytes(), b"value")?;
}

// 手动触发 flush
db.flush()?;

// 验证数据持久化
let db2 = DB::open("./data", Options::default())?;
assert!(db2.get(b"key9999")?.is_some());
```

**学习要点**：
- 何时触发 flush
- 手动 vs 自动 flush
- Flush 对性能的影响

### 5. Bloom Filter (bloom_filter_example.rs)

演示 Bloom Filter 如何加速查询。

```rust
// 启用 Bloom Filter
let mut options = Options::default();
options.enable_bloom_filter = true;
options.bloom_filter_bits_per_key = 10;

let db = DB::open("./data", options)?;

// 写入数据
for i in 0..10000 {
    db.put(format!("key{}", i).as_bytes(), b"value")?;
}

// 查询不存在的键（会被 Bloom Filter 快速过滤）
let start = std::time::Instant::now();
for i in 10000..20000 {
    let _ = db.get(format!("key{}", i).as_bytes())?;
}
println!("Query time: {:?}", start.elapsed());
```

**学习要点**：
- Bloom Filter 原理
- 性能提升效果
- 误判率权衡

### 6. 快照和迭代器 (week13_14_features.rs)

演示高级功能：快照、迭代器、范围查询。

#### 快照 (Snapshot)

```rust
// 创建快照
let snapshot = db.snapshot();

// 在快照后写入新数据
db.put(b"new_key", b"new_value")?;

// 快照看不到新数据
assert!(snapshot.get(b"new_key")?.is_none());

// 但可以看到快照时的数据
let old_value = snapshot.get(b"old_key")?;
```

**使用场景**：
- 一致性备份
- 长时间的读取操作
- 数据分析和报表

#### 迭代器 (Iterator)

```rust
// 遍历所有数据
let mut iter = db.iter();
while iter.valid() {
    println!("{:?} => {:?}", iter.key(), iter.value());
    iter.next();
}

// 范围查询
let mut iter = db.scan(Some(b"user:"), Some(b"user:z"))?;
while iter.valid() {
    println!("User: {:?}", iter.value());
    iter.next();
}
```

**使用场景**：
- 数据导出
- 批量处理
- 前缀查询

#### WriteBatch

```rust
// 原子批量写入
let mut batch = WriteBatch::new();
batch.put(b"key1", b"value1");
batch.put(b"key2", b"value2");
batch.delete(b"old_key");
db.write(batch)?;
```

**使用场景**：
- 事务性操作
- 批量导入
- 性能优化

---

## 性能对比示例

### 单条写入 vs 批量写入

```rust
use std::time::Instant;

// 单条写入（慢）
let start = Instant::now();
for i in 0..10000 {
    db.put(format!("key{}", i).as_bytes(), b"value")?;
}
println!("Single writes: {:?}", start.elapsed());
// 预期：~5-10 秒

// 批量写入（快）
let start = Instant::now();
let mut batch = WriteBatch::new();
for i in 0..10000 {
    batch.put(format!("key{}", i).as_bytes(), b"value");
}
db.write(batch)?;
println!("Batch write: {:?}", start.elapsed());
// 预期：~0.1-0.5 秒
```

### 有无 Bloom Filter 对比

```rust
// 无 Bloom Filter
let options = Options::default();
options.enable_bloom_filter = false;
// 查询不存在的键需要读取磁盘

// 有 Bloom Filter
let mut options = Options::default();
options.enable_bloom_filter = true;
// 查询不存在的键快 10-100 倍
```

---

## 实战场景示例

### 场景 1：用户会话存储

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct Session {
    user_id: u64,
    token: String,
    expires_at: u64,
}

fn store_session(db: &DB, session: &Session) -> Result<()> {
    let key = format!("session:{}", session.token);
    let value = bincode::serialize(session)?;
    db.put(key.as_bytes(), &value)?;
    Ok(())
}

fn get_session(db: &DB, token: &str) -> Result<Option<Session>> {
    let key = format!("session:{}", token);
    if let Some(data) = db.get(key.as_bytes())? {
        let session = bincode::deserialize(&data)?;
        Ok(Some(session))
    } else {
        Ok(None)
    }
}
```

### 场景 2：时间序列数据

```rust
fn store_metric(db: &DB, metric_name: &str, value: f64) -> Result<()> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    
    let key = format!("metric:{}:{:020}", metric_name, timestamp);
    db.put(key.as_bytes(), value.to_string().as_bytes())?;
    Ok(())
}

fn query_metrics(
    db: &DB,
    metric_name: &str,
    start_time: u64,
    end_time: u64,
) -> Result<Vec<(u64, f64)>> {
    let start_key = format!("metric:{}:{:020}", metric_name, start_time);
    let end_key = format!("metric:{}:{:020}", metric_name, end_time);
    
    let mut results = Vec::new();
    let mut iter = db.scan(
        Some(start_key.as_bytes()),
        Some(end_key.as_bytes()),
    )?;
    
    while iter.valid() {
        let key = String::from_utf8_lossy(iter.key());
        let timestamp = key.split(':').last().unwrap().parse::<u64>()?;
        let value = String::from_utf8_lossy(iter.value()).parse::<f64>()?;
        results.push((timestamp, value));
        iter.next();
    }
    
    Ok(results)
}
```

### 场景 3：缓存系统

```rust
use std::time::{Duration, SystemTime};

struct CacheEntry {
    value: Vec<u8>,
    expires_at: SystemTime,
}

fn cache_put(
    db: &DB,
    key: &[u8],
    value: &[u8],
    ttl: Duration,
) -> Result<()> {
    let entry = CacheEntry {
        value: value.to_vec(),
        expires_at: SystemTime::now() + ttl,
    };
    let data = bincode::serialize(&entry)?;
    db.put(key, &data)?;
    Ok(())
}

fn cache_get(db: &DB, key: &[u8]) -> Result<Option<Vec<u8>>> {
    if let Some(data) = db.get(key)? {
        let entry: CacheEntry = bincode::deserialize(&data)?;
        
        if entry.expires_at > SystemTime::now() {
            Ok(Some(entry.value))
        } else {
            // 过期，删除
            db.delete(key)?;
            Ok(None)
        }
    } else {
        Ok(None)
    }
}
```

---

## 调试技巧

### 1. 启用日志

```bash
# 设置日志级别
RUST_LOG=aidb=debug cargo run --example basic

# 或在代码中
env_logger::Builder::from_env(
    env_logger::Env::default().default_filter_or("debug")
).init();
```

### 2. 检查数据库状态

```rust
let stats = db.get_stats()?;
println!("Stats: {:#?}", stats);
```

### 3. 性能分析

```bash
# 使用 perf
perf record cargo run --release --example db_example
perf report

# 使用 flamegraph
cargo flamegraph --example db_example
```

---

## 常见问题

### Q: 示例运行失败怎么办？

**A**: 检查以下几点：
1. Rust 版本是否 >= 1.70
2. 是否有足够的磁盘空间
3. 清理旧的测试数据
4. 查看错误信息和日志

### Q: 如何修改示例代码？

**A**: 
1. 复制示例文件到新文件
2. 在 `Cargo.toml` 添加新的 `[[example]]` 配置
3. 修改和测试

### Q: 示例的性能数据准确吗？

**A**: 示例中的性能数据仅供参考，实际性能取决于：
- 硬件配置（CPU、内存、磁盘）
- 数据规模和访问模式
- 配置参数
建议在自己的环境中进行基准测试。

---

## 贡献示例

欢迎贡献新的示例！请确保：

1. **代码清晰**：添加足够的注释
2. **文档完整**：在本 README 中添加说明
3. **可运行**：确保示例可以直接运行
4. **有意义**：展示有用的功能或模式

提交 PR 时请包含：
- 示例代码文件
- README 更新
- 示例说明

---

## 下一步

- 阅读 [用户指南](../docs/USER_GUIDE.md) 了解完整功能
- 阅读 [最佳实践](../docs/BEST_PRACTICES.md) 学习生产环境使用
- 阅读 [性能调优](../docs/PERFORMANCE_TUNING.md) 优化性能
- 查看 [API 文档](https://docs.rs/aidb)

---

**有问题？** 欢迎在 [GitHub Issues](https://github.com/yourusername/aidb/issues) 提问！
