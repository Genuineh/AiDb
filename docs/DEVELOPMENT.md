# 开发指南

本文档帮助开发者快速上手AiDb开发。

## 目录

- [环境准备](#环境准备)
- [代码结构](#代码结构)
- [开发流程](#开发流程)
- [编码规范](#编码规范)
- [测试指南](#测试指南)
- [性能优化](#性能优化)

---

## 环境准备

### 安装Rust

```bash
# 安装rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 更新到最新版本
rustup update

# 确认版本(需要1.70+)
rustc --version
```

### 克隆项目

```bash
git clone https://github.com/yourusername/aidb.git
cd aidb
```

### 安装开发工具

```bash
# 代码格式化
rustup component add rustfmt

# 静态分析
rustup component add clippy

# 文档生成
cargo install cargo-doc

# 性能分析
cargo install flamegraph
cargo install cargo-criterion
```

### 编译项目

```bash
# 开发模式（快速编译）
cargo build

# 发布模式（优化）
cargo build --release

# 检查编译（不生成二进制）
cargo check
```

---

## 代码结构

```
aidb/
├── src/                    # 源代码
│   ├── lib.rs             # 库入口
│   ├── error.rs           # 错误定义
│   ├── config.rs          # 配置
│   │
│   ├── wal/               # WAL模块
│   │   ├── mod.rs
│   │   ├── writer.rs
│   │   └── reader.rs
│   │
│   ├── memtable/          # MemTable模块
│   │   ├── mod.rs
│   │   └── skiplist.rs
│   │
│   ├── sstable/           # SSTable模块
│   │   ├── mod.rs
│   │   ├── builder.rs
│   │   ├── reader.rs
│   │   └── block.rs
│   │
│   ├── compaction/        # Compaction模块
│   │   ├── mod.rs
│   │   └── picker.rs
│   │
│   └── cluster/           # 集群模块（待实现）
│       ├── mod.rs
│       ├── coordinator.rs
│       ├── primary.rs
│       └── replica.rs
│
├── tests/                 # 集成测试
│   ├── basic_test.rs
│   └── recovery_test.rs
│
├── benches/               # 性能测试
│   ├── write_bench.rs
│   └── read_bench.rs
│
├── examples/              # 示例
│   └── basic.rs
│
├── proto/                 # Protobuf定义（待添加）
│   └── aidb.proto
│
└── docs/                  # 文档
    ├── ARCHITECTURE.md
    ├── IMPLEMENTATION.md
    └── this file
```

---

## 开发流程

### 1. 选择任务

从[TODO.md](../TODO.md)中选择未完成的任务。

### 2. 创建分支

```bash
git checkout -b feature/wal-implementation
```

### 3. 实现功能

#### TDD方式（推荐）

```rust
// 1. 先写测试
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wal_append() {
        let mut wal = WAL::create("/tmp/test.log").unwrap();
        wal.append(b"key", b"value").unwrap();
        wal.sync().unwrap();
        
        // 验证
        let wal2 = WAL::open("/tmp/test.log").unwrap();
        let records = wal2.read_all().unwrap();
        assert_eq!(records.len(), 1);
    }
}

// 2. 实现功能
pub struct WAL {
    file: File,
}

impl WAL {
    pub fn append(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        // 实现...
        todo!()
    }
}

// 3. 运行测试
// cargo test test_wal_append

// 4. 修复直到通过
```

### 4. 代码检查

```bash
# 格式化
cargo fmt

# 静态分析
cargo clippy -- -D warnings

# 运行测试
cargo test

# 运行特定测试
cargo test wal

# 显示输出
cargo test -- --nocapture
```

### 5. 提交代码

```bash
git add .
git commit -m "feat: implement WAL append and recovery"
git push origin feature/wal-implementation
```

### 6. 创建Pull Request

在GitHub上创建PR，等待代码审查。

---

## 编码规范

### 命名约定

```rust
// 类型：大驼峰
struct MemTable { }
enum RecordType { }

// 函数/变量：小写下划线
fn create_sstable() { }
let file_size = 1024;

// 常量：大写下划线
const MAX_BLOCK_SIZE: usize = 4096;

// 泛型：单个大写字母或大驼峰
fn serialize<T: Serialize>(value: &T) { }
fn merge<K: Key, V: Value>() { }
```

### 代码组织

```rust
// 模块结构
pub mod wal {
    mod writer;    // 私有子模块
    mod reader;
    
    pub use writer::WALWriter;  // 重导出
    pub use reader::WALReader;
}

// 使用
use crate::wal::{WALWriter, WALReader};
```

### 错误处理

```rust
// 使用Result
pub fn open(path: &str) -> Result<DB> {
    let file = File::open(path)?;  // 使用?传播错误
    // ...
}

// 不要用unwrap/expect
// ❌ 错误
let file = File::open(path).unwrap();

// ✅ 正确
let file = File::open(path)?;
```

### 文档注释

```rust
/// Opens a database at the specified path.
///
/// # Arguments
///
/// * `path` - The directory path for the database
/// * `options` - Configuration options
///
/// # Errors
///
/// Returns an error if:
/// - The path is invalid
/// - Insufficient permissions
/// - Data corruption detected
///
/// # Example
///
/// ```
/// use aidb::{DB, Options};
///
/// let db = DB::open("./data", Options::default())?;
/// ```
pub fn open(path: &str, options: Options) -> Result<DB> {
    // 实现...
}
```

### 代码风格

```rust
// 使用Rust习惯用法

// ✅ 好
if let Some(value) = db.get(key)? {
    process(value);
}

// ❌ 差
match db.get(key)? {
    Some(value) => process(value),
    None => {}
}

// ✅ 迭代器链
let sum: u64 = values
    .iter()
    .filter(|v| v.is_valid())
    .map(|v| v.size())
    .sum();

// ✅ 早返回
fn process(data: &[u8]) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    // 处理...
}
```

---

## 测试指南

### 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_basic_operation() {
        // 使用临时目录
        let dir = tempdir().unwrap();
        let db = DB::open(dir.path(), Options::default()).unwrap();
        
        // 测试
        db.put(b"key", b"value").unwrap();
        assert_eq!(db.get(b"key").unwrap(), Some(b"value".to_vec()));
    }
    
    #[test]
    #[should_panic(expected = "key too large")]
    fn test_large_key() {
        let db = DB::open("test", Options::default()).unwrap();
        let large_key = vec![0u8; 1_000_000];
        db.put(&large_key, b"value").unwrap();
    }
}
```

### 集成测试

```rust
// tests/integration_test.rs
use aidb::{DB, Options};

#[test]
fn test_persistence() {
    let path = "/tmp/test_db";
    
    // 写入数据
    {
        let db = DB::open(path, Options::default()).unwrap();
        for i in 0..1000 {
            db.put(&format!("key{}", i).as_bytes(), b"value").unwrap();
        }
    } // db dropped
    
    // 重新打开，验证数据
    {
        let db = DB::open(path, Options::default()).unwrap();
        for i in 0..1000 {
            assert!(db.get(&format!("key{}", i).as_bytes()).unwrap().is_some());
        }
    }
}
```

### 运行测试

```bash
# 所有测试
cargo test

# 特定模块
cargo test wal

# 特定测试
cargo test test_basic_operation

# 显示输出
cargo test -- --nocapture

# 单线程运行（调试用）
cargo test -- --test-threads=1

# 只编译测试
cargo test --no-run
```

### 性能测试

```rust
// benches/write_bench.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use aidb::{DB, Options};

fn bench_write(c: &mut Criterion) {
    let db = DB::open("/tmp/bench", Options::default()).unwrap();
    
    c.bench_function("write 1KB", |b| {
        b.iter(|| {
            let key = format!("key{}", rand::random::<u64>());
            let value = vec![0u8; 1024];
            db.put(black_box(key.as_bytes()), black_box(&value)).unwrap();
        });
    });
}

criterion_group!(benches, bench_write);
criterion_main!(benches);
```

```bash
# 运行基准测试
cargo bench

# 特定测试
cargo bench write
```

---

## 性能优化

### Profiling

#### 使用flamegraph

```bash
# 安装
cargo install flamegraph

# 生成火焰图
cargo flamegraph --bin aidb

# 查看
firefox flamegraph.svg
```

#### 使用perf (Linux)

```bash
# 编译release版本
cargo build --release

# 记录
perf record -g ./target/release/aidb

# 查看
perf report
```

### 优化技巧

#### 1. 避免不必要的分配

```rust
// ❌ 差：每次都分配
fn process() {
    let mut buffer = Vec::new();
    // 使用buffer
}

// ✅ 好：复用buffer
struct Processor {
    buffer: Vec<u8>,
}

impl Processor {
    fn process(&mut self) {
        self.buffer.clear();
        // 使用buffer
    }
}
```

#### 2. 使用合适的数据结构

```rust
// HashMap vs BTreeMap
// HashMap: 查找O(1)，无序
// BTreeMap: 查找O(log n)，有序，适合范围查询

// Vec vs VecDeque
// Vec: 尾部操作O(1)
// VecDeque: 头尾操作O(1)
```

#### 3. 批量操作

```rust
// ❌ 差：逐个写入
for i in 0..1000 {
    db.put(&key[i], &value[i])?;
}

// ✅ 好：批量写入
let mut batch = WriteBatch::new();
for i in 0..1000 {
    batch.put(&key[i], &value[i]);
}
db.write(batch)?;
```

#### 4. 并发优化

```rust
// 使用Arc + RwLock
let cache = Arc::new(RwLock::new(LruCache::new(1000)));

// 读操作
{
    let cache = cache.read();
    cache.get(key)
}

// 写操作
{
    let mut cache = cache.write();
    cache.put(key, value);
}
```

---

## 调试技巧

### 日志

```rust
use log::{debug, info, warn, error};

// 使用日志而非println!
info!("Opening database at {:?}", path);
debug!("MemTable size: {} bytes", size);
warn!("Compaction is slow: {:?}", duration);
error!("Failed to write WAL: {}", err);
```

```bash
# 设置日志级别
RUST_LOG=debug cargo test
RUST_LOG=aidb=trace cargo run
```

### 断言

```rust
// 开发时的断言
debug_assert!(key.len() > 0);
debug_assert_eq!(actual, expected);

// 生产环境的检查
assert!(key.len() > 0, "Key cannot be empty");
```

---

## 常见问题

### Q: 如何运行单个测试？
```bash
cargo test test_name
```

### Q: 测试失败如何调试？
```bash
cargo test test_name -- --nocapture
```

### Q: 如何查看性能？
```bash
cargo bench
cargo flamegraph
```

### Q: 代码检查不通过？
```bash
cargo clippy --fix
cargo fmt
```

---

## 获取帮助

- 查看[架构文档](ARCHITECTURE.md)了解设计
- 查看[实施计划](IMPLEMENTATION.md)了解任务
- 提Issue或在Discussion讨论

---

**Happy Coding!** 🎉
