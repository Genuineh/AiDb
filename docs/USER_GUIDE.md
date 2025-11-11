# AiDb 用户指南

本指南提供AiDb的完整使用说明，从安装到高级功能。

## 目录

- [安装](#安装)
- [快速开始](#快速开始)
- [基础操作](#基础操作)
- [高级功能](#高级功能)
- [配置选项](#配置选项)
- [错误处理](#错误处理)
- [性能优化](#性能优化)

---

## 安装

### 系统要求

- **操作系统**: Linux, macOS, Windows
- **Rust版本**: 1.70 或更高
- **磁盘空间**: 至少 100MB 用于编译

### 从源代码安装

```bash
# 克隆仓库
git clone https://github.com/yourusername/aidb.git
cd aidb

# 编译（Debug模式）
cargo build

# 编译（Release模式，推荐生产环境）
cargo build --release
```

### 作为依赖使用

在您的 `Cargo.toml` 中添加：

```toml
[dependencies]
aidb = "0.1"
```

---

## 快速开始

### 最简单的示例

```rust
use aidb::{DB, Options};

fn main() -> Result<(), aidb::Error> {
    // 打开数据库
    let options = Options::default();
    let db = DB::open("./data", options)?;
    
    // 写入数据
    db.put(b"hello", b"world")?;
    
    // 读取数据
    if let Some(value) = db.get(b"hello")? {
        println!("Value: {:?}", String::from_utf8_lossy(&value));
    }
    
    // 删除数据
    db.delete(b"hello")?;
    
    Ok(())
}
```

---

## 基础操作

### 打开数据库

```rust
use aidb::{DB, Options};
use std::sync::Arc;

// 使用默认配置
let options = Options::default();
let db = DB::open("./my_database", options)?;

// 包装为 Arc 以支持多线程共享
let db = Arc::new(db);
```

### 写入数据 (Put)

```rust
// 简单写入
db.put(b"key1", b"value1")?;

// 写入不同类型的数据（需要序列化）
db.put(b"user:1", b"Alice")?;
db.put(b"count", b"42")?;
```

### 读取数据 (Get)

```rust
// 读取存在的键
if let Some(value) = db.get(b"key1")? {
    println!("Found: {:?}", value);
}

// 处理不存在的键
match db.get(b"nonexistent")? {
    Some(value) => println!("Value: {:?}", value),
    None => println!("Key not found"),
}
```

### 删除数据 (Delete)

```rust
// 删除键
db.delete(b"key1")?;

// 删除后读取返回 None
assert!(db.get(b"key1")?.is_none());
```

### 批量写入 (WriteBatch)

批量操作保证原子性，要么全部成功，要么全部失败。

```rust
use aidb::WriteBatch;

let mut batch = WriteBatch::new();

// 添加多个操作
batch.put(b"key1", b"value1");
batch.put(b"key2", b"value2");
batch.delete(b"old_key");

// 原子提交
db.write(batch)?;
```

---

## 高级功能

### 快照 (Snapshot)

快照提供点时间一致性读取，即使数据库继续被修改。

```rust
// 创建快照
let snapshot = db.snapshot();

// 在快照后写入新数据
db.put(b"new_key", b"new_value")?;

// 快照看不到新数据
assert!(snapshot.get(b"new_key")?.is_none());

// 但可以看到快照时的数据
if let Some(value) = snapshot.get(b"old_key")? {
    println!("Snapshot value: {:?}", value);
}
```

**使用场景**：
- 一致性备份
- 长时间的读取事务
- 数据分析和报表生成

### 迭代器 (Iterator)

遍历数据库中的所有键值对。

```rust
// 创建迭代器
let mut iter = db.iter();

// 遍历所有数据
while iter.valid() {
    let key = iter.key();
    let value = iter.value();
    println!("{:?} => {:?}", key, value);
    iter.next();
}
```

### 范围查询 (Range Query)

查询指定范围内的键值对。

```rust
// 查询 [start_key, end_key) 范围
let mut iter = db.scan(Some(b"user:"), Some(b"user:z"))?;

while iter.valid() {
    println!("{:?} => {:?}", iter.key(), iter.value());
    iter.next();
}

// 从某个键开始到末尾
let mut iter = db.scan(Some(b"item:"), None)?;

// 从开始到某个键
let mut iter = db.scan(None, Some(b"limit"))?;
```

**使用场景**：
- 按前缀查询（如所有用户数据）
- 范围统计
- 分页查询

### 手动 Flush

将内存中的数据刷新到磁盘。

```rust
// 手动触发 flush
db.flush()?;
```

**注意**：通常不需要手动调用，以下情况会自动 flush：
- MemTable 达到配置的大小阈值
- 数据库关闭时
- 调用 `close()` 时

### 关闭数据库

```rust
// 显式关闭（会自动 flush）
db.close()?;

// 或者让 Drop trait 自动处理
drop(db);
```

---

## 配置选项

### Options 结构

```rust
use aidb::Options;

let mut options = Options::default();

// MemTable 大小 (字节)
// 达到此大小时触发 flush
options.memtable_size = 4 * 1024 * 1024; // 4MB

// SSTable 大小 (字节)
options.sstable_size = 2 * 1024 * 1024; // 2MB

// Block 大小 (字节)
options.block_size = 4096; // 4KB

// Block Cache 大小 (字节)
options.block_cache_size = 64 * 1024 * 1024; // 64MB

// 是否启用 Bloom Filter
options.enable_bloom_filter = true;

// Bloom Filter 位数（每个键）
options.bloom_filter_bits_per_key = 10;

// 是否启用压缩
options.enable_compression = true;

// 压缩类型
options.compression_type = aidb::CompressionType::Snappy;

let db = DB::open("./data", options)?;
```

### 配置说明

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `memtable_size` | 4MB | MemTable 达到此大小时 flush |
| `sstable_size` | 2MB | SSTable 文件的目标大小 |
| `block_size` | 4KB | 数据块大小，影响读取粒度 |
| `block_cache_size` | 64MB | Block Cache 大小，影响读取性能 |
| `enable_bloom_filter` | true | 是否启用 Bloom Filter |
| `bloom_filter_bits_per_key` | 10 | Bloom Filter 误判率参数 |
| `enable_compression` | true | 是否启用压缩 |
| `compression_type` | Snappy | 压缩算法（Snappy 或 LZ4） |

---

## 错误处理

### 错误类型

AiDb 使用 `Result<T, Error>` 返回操作结果。

```rust
use aidb::Error;

match db.get(b"key") {
    Ok(Some(value)) => {
        println!("Value: {:?}", value);
    }
    Ok(None) => {
        println!("Key not found");
    }
    Err(Error::IoError(e)) => {
        eprintln!("IO error: {}", e);
    }
    Err(Error::Corruption(msg)) => {
        eprintln!("Data corruption: {}", msg);
    }
    Err(e) => {
        eprintln!("Other error: {}", e);
    }
}
```

### 常见错误

| 错误类型 | 原因 | 解决方法 |
|---------|------|---------|
| `IoError` | 磁盘读写失败 | 检查磁盘空间和权限 |
| `Corruption` | 数据损坏 | 从备份恢复 |
| `InvalidArgument` | 参数错误 | 检查输入参数 |
| `NotFound` | 数据库目录不存在 | 确保目录存在或创建 |

### 最佳实践

```rust
use aidb::{DB, Options, Error};

fn safe_operation(db: &DB) -> Result<(), Error> {
    // 使用 ? 操作符传播错误
    db.put(b"key", b"value")?;
    
    // 或使用 match 处理特定错误
    match db.get(b"key") {
        Ok(Some(value)) => {
            println!("Success: {:?}", value);
            Ok(())
        }
        Ok(None) => {
            println!("Key not found, using default");
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            Err(e)
        }
    }
}
```

---

## 性能优化

### 写入性能优化

1. **使用 WriteBatch**：批量写入比单独写入快得多
   ```rust
   let mut batch = WriteBatch::new();
   for i in 0..1000 {
       batch.put(format!("key{}", i).as_bytes(), b"value");
   }
   db.write(batch)?;
   ```

2. **调整 MemTable 大小**：更大的 MemTable 减少 flush 频率
   ```rust
   options.memtable_size = 8 * 1024 * 1024; // 8MB
   ```

3. **禁用 fsync（非生产环境）**：加快写入速度，但可能丢失数据
   ```rust
   // 注意：仅用于测试环境
   options.disable_wal = true;
   ```

### 读取性能优化

1. **增大 Block Cache**：缓存更多数据块
   ```rust
   options.block_cache_size = 128 * 1024 * 1024; // 128MB
   ```

2. **启用 Bloom Filter**：减少无效的磁盘读取
   ```rust
   options.enable_bloom_filter = true;
   options.bloom_filter_bits_per_key = 10;
   ```

3. **调整 Block 大小**：根据访问模式调整
   ```rust
   // 随机读取：使用较小的 block
   options.block_size = 4096; // 4KB
   
   // 顺序扫描：使用较大的 block
   options.block_size = 16384; // 16KB
   ```

### 空间优化

1. **启用压缩**：减少磁盘使用
   ```rust
   options.enable_compression = true;
   options.compression_type = aidb::CompressionType::Snappy;
   ```

2. **定期 Compaction**：清理删除的数据和旧版本
   ```rust
   // Compaction 会自动触发，也可以手动触发
   db.compact_range(None, None)?;
   ```

### 多线程使用

```rust
use std::sync::Arc;
use std::thread;

let db = Arc::new(DB::open("./data", Options::default())?);

// 启动多个读线程
let mut handles = vec![];
for i in 0..4 {
    let db_clone = Arc::clone(&db);
    let handle = thread::spawn(move || {
        for j in 0..1000 {
            let key = format!("key{}", j);
            let _ = db_clone.get(key.as_bytes());
        }
    });
    handles.push(handle);
}

// 等待所有线程完成
for handle in handles {
    handle.join().unwrap();
}
```

---

## 示例场景

### 场景1：简单的键值存储

```rust
use aidb::{DB, Options};

fn main() -> Result<(), aidb::Error> {
    let db = DB::open("./cache", Options::default())?;
    
    // 存储缓存数据
    db.put(b"session:123", b"user_data")?;
    
    // 获取缓存
    if let Some(data) = db.get(b"session:123")? {
        println!("Cache hit: {:?}", data);
    }
    
    // 过期删除
    db.delete(b"session:123")?;
    
    Ok(())
}
```

### 场景2：用户数据存储

```rust
use aidb::{DB, Options, WriteBatch};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
    email: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = DB::open("./users", Options::default())?;
    
    // 存储用户
    let user = User {
        id: 1,
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
    };
    let key = format!("user:{}", user.id);
    let value = bincode::serialize(&user)?;
    db.put(key.as_bytes(), &value)?;
    
    // 读取用户
    if let Some(data) = db.get(key.as_bytes())? {
        let user: User = bincode::deserialize(&data)?;
        println!("User: {} <{}>", user.name, user.email);
    }
    
    Ok(())
}
```

### 场景3：时间序列数据

```rust
use aidb::{DB, Options};
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> Result<(), aidb::Error> {
    let db = DB::open("./metrics", Options::default())?;
    
    // 记录指标
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let key = format!("metric:cpu:{}", timestamp);
    db.put(key.as_bytes(), b"75.5")?;
    
    // 查询时间范围
    let start_time = timestamp - 3600; // 1小时前
    let start_key = format!("metric:cpu:{}", start_time);
    let end_key = format!("metric:cpu:{}", timestamp + 1);
    
    let mut iter = db.scan(Some(start_key.as_bytes()), Some(end_key.as_bytes()))?;
    while iter.valid() {
        println!("{:?} => {:?}", 
                 String::from_utf8_lossy(iter.key()),
                 String::from_utf8_lossy(iter.value()));
        iter.next();
    }
    
    Ok(())
}
```

---

## 故障排查

### 数据库打不开

**问题**: `Error: NotFound` 或 `Error: IoError`

**解决**:
1. 确保目录存在
2. 检查文件权限
3. 确保没有其他进程在使用数据库

### 性能下降

**问题**: 读写速度变慢

**解决**:
1. 检查磁盘空间是否充足
2. 增大 Block Cache 大小
3. 触发手动 Compaction
4. 检查 Bloom Filter 是否启用

### 数据损坏

**问题**: `Error: Corruption`

**解决**:
1. 尝试从 WAL 恢复
2. 从备份恢复数据库
3. 检查磁盘是否有硬件问题

---

## 下一步

- 阅读 [性能调优指南](PERFORMANCE_TUNING.md) 了解更多优化技巧
- 阅读 [最佳实践](BEST_PRACTICES.md) 了解生产环境部署建议
- 查看 [examples/](../examples/) 目录获取更多示例代码
- 参考 [API 文档](https://docs.rs/aidb) 了解完整 API

---

**有问题？** 欢迎在 [GitHub Issues](https://github.com/yourusername/aidb/issues) 提问！
