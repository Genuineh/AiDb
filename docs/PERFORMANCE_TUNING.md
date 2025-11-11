# AiDb 性能调优指南

本指南深入讲解 AiDb 的性能调优技术，帮助您充分发挥存储引擎的性能潜力。

## 目录

- [性能基准](#性能基准)
- [写入性能优化](#写入性能优化)
- [读取性能优化](#读取性能优化)
- [内存管理](#内存管理)
- [磁盘优化](#磁盘优化)
- [Compaction 调优](#compaction-调优)
- [性能诊断](#性能诊断)
- [压测指南](#压测指南)

---

## 性能基准

### 硬件配置

推荐的硬件配置：

| 组件 | 最低配置 | 推荐配置 | 高性能配置 |
|------|---------|---------|-----------|
| CPU | 2核 | 4核 | 8核+ |
| 内存 | 4GB | 8GB | 16GB+ |
| 磁盘 | HDD | SATA SSD | NVMe SSD |
| 网络 | 100Mbps | 1Gbps | 10Gbps |

### 目标性能

单机性能目标（NVMe SSD）：

| 操作类型 | 目标性能 | RocksDB 对比 |
|---------|---------|-------------|
| 顺序写入 | 140K ops/s | ~70% |
| 随机写入 | 70K ops/s | ~70% |
| 随机读取（热数据） | 140K ops/s | ~70% |
| 随机读取（冷数据） | 20K ops/s | ~60% |
| 范围扫描 | 100K keys/s | ~70% |

**延迟目标**（P99）：
- 写入延迟：< 1ms
- 读取延迟（缓存命中）：< 0.1ms
- 读取延迟（缓存未命中）：< 5ms

---

## 写入性能优化

### 1. 使用 WriteBatch

**性能提升**：10-100x

```rust
use aidb::WriteBatch;
use std::time::Instant;

// ❌ 慢：单独写入
fn slow_write(db: &DB, count: usize) {
    let start = Instant::now();
    for i in 0..count {
        db.put(format!("key{}", i).as_bytes(), b"value").unwrap();
    }
    println!("Time: {:?}", start.elapsed());
    // 10,000 写入 ~5-10 秒
}

// ✅ 快：批量写入
fn fast_write(db: &DB, count: usize) {
    let start = Instant::now();
    let mut batch = WriteBatch::new();
    for i in 0..count {
        batch.put(format!("key{}", i).as_bytes(), b"value");
        
        // 每 1000 个提交一次（避免内存过大）
        if i % 1000 == 0 && i > 0 {
            db.write(batch).unwrap();
            batch = WriteBatch::new();
        }
    }
    if batch.len() > 0 {
        db.write(batch).unwrap();
    }
    println!("Time: {:?}", start.elapsed());
    // 10,000 写入 ~0.1-0.5 秒
}
```

**最佳批量大小**：
- 100-1000 条记录：适合大多数场景
- 1000-5000 条记录：写入密集场景
- 超过 5000 条：可能导致内存压力，建议分批

### 2. 调整 MemTable 大小

**性能影响**：减少 Flush 频率，提升写入吞吐

```rust
let mut options = Options::default();

// 场景 1：高并发写入
options.memtable_size = 16 * 1024 * 1024; // 16MB
// 优点：减少 Flush 频率
// 缺点：占用更多内存

// 场景 2：内存受限
options.memtable_size = 2 * 1024 * 1024; // 2MB
// 优点：节省内存
// 缺点：频繁 Flush

// 场景 3：均衡（推荐）
options.memtable_size = 4 * 1024 * 1024; // 4MB
```

**计算公式**：

```
MemTable 大小 = (预期写入速率 × Flush 间隔) / 压缩率

示例：
- 写入速率：10MB/s
- 期望 Flush 间隔：1秒
- 压缩率：~0.6（Snappy）
MemTable 大小 = 10MB × 1s / 0.6 ≈ 16MB
```

### 3. WAL 优化

```rust
// 场景 1：高可靠性（默认）
options.sync_wal = true;
// 每次写入都 fsync，确保持久化
// 性能：~5K-10K ops/s

// 场景 2：高性能（测试环境）
options.sync_wal = false;
// 延迟 fsync，可能丢失最后几毫秒的数据
// 性能：~50K-100K ops/s

// 场景 3：组提交（推荐）
options.sync_wal = true;
options.wal_sync_interval = Duration::from_millis(10);
// 每 10ms 批量 fsync
// 性能：~20K-40K ops/s
// 平衡可靠性和性能
```

### 4. 并发写入

```rust
use std::sync::Arc;
use std::thread;

fn parallel_write(db: Arc<DB>, threads: usize, ops_per_thread: usize) {
    let start = Instant::now();
    
    let handles: Vec<_> = (0..threads)
        .map(|t| {
            let db = Arc::clone(&db);
            thread::spawn(move || {
                for i in 0..ops_per_thread {
                    let key = format!("thread{}:key{}", t, i);
                    db.put(key.as_bytes(), b"value").unwrap();
                }
            })
        })
        .collect();
    
    for handle in handles {
        handle.join().unwrap();
    }
    
    let duration = start.elapsed();
    let total_ops = threads * ops_per_thread;
    println!("Throughput: {:.2} ops/s", 
             total_ops as f64 / duration.as_secs_f64());
}

// 测试不同线程数
parallel_write(db, 1, 10000);  // ~20K ops/s
parallel_write(db, 4, 10000);  // ~60K ops/s
parallel_write(db, 8, 10000);  // ~80K ops/s
```

**线程数建议**：
- 写入密集：CPU 核心数
- 混合负载：CPU 核心数 × 0.5-0.75
- 读取密集：CPU 核心数 × 1.5-2

---

## 读取性能优化

### 1. Block Cache 调优

**性能影响**：缓存命中率每提升 10%，读取性能提升 2-5x

```rust
let mut options = Options::default();

// 计算 Cache 大小
fn calculate_cache_size(
    working_set_size: usize,
    memory_available: usize,
) -> usize {
    // 使用可用内存的 30-50%
    let max_cache = memory_available / 2;
    
    // 至少覆盖工作集的 50%
    let min_cache = working_set_size / 2;
    
    std::cmp::min(max_cache, std::cmp::max(min_cache, 64 * 1024 * 1024))
}

// 示例：
// - 工作集：1GB
// - 可用内存：4GB
let cache_size = calculate_cache_size(
    1 * 1024 * 1024 * 1024,  // 1GB
    4 * 1024 * 1024 * 1024,  // 4GB
);
options.block_cache_size = cache_size; // ~512MB
```

**监控缓存效率**：

```rust
fn monitor_cache(db: &DB) {
    let stats = db.get_stats().unwrap();
    
    println!("Cache hit rate: {:.2}%", 
             stats.block_cache_hit_rate * 100.0);
    
    // 目标命中率
    if stats.block_cache_hit_rate < 0.8 {
        println!("Warning: Low cache hit rate!");
        println!("Consider increasing block_cache_size");
    }
}
```

**不同场景的配置**：

```rust
// 场景 1：读密集，热点数据明显
options.block_cache_size = 512 * 1024 * 1024; // 512MB
options.block_size = 4 * 1024; // 4KB（小块，精确缓存）

// 场景 2：读密集，数据分布均匀
options.block_cache_size = 1024 * 1024 * 1024; // 1GB
options.block_size = 16 * 1024; // 16KB（大块，减少索引开销）

// 场景 3：写密集，读较少
options.block_cache_size = 128 * 1024 * 1024; // 128MB
options.block_size = 8 * 1024; // 8KB
```

### 2. Bloom Filter 调优

**性能影响**：减少 90%+ 的无效磁盘读取

```rust
let mut options = Options::default();

// 启用 Bloom Filter
options.enable_bloom_filter = true;

// 调整误判率
// bits_per_key | 误判率  | 内存开销
// ------------|---------|----------
// 6           | ~5%     | 低
// 10          | ~1%     | 中（推荐）
// 15          | ~0.1%   | 高
options.bloom_filter_bits_per_key = 10;
```

**Bloom Filter 效果测试**：

```rust
fn test_bloom_filter_effect(db: &DB) {
    let start = Instant::now();
    
    // 查询不存在的键
    for i in 0..10000 {
        let key = format!("nonexistent:{}", i);
        let _ = db.get(key.as_bytes()).unwrap();
    }
    
    let duration = start.elapsed();
    println!("Without Bloom Filter: {:?}", duration);
    // 预期：~1-5 秒（需要读磁盘）
    
    // 启用 Bloom Filter 后
    // 预期：~0.1-0.5 秒（大部分被过滤）
}
```

### 3. 预读和预热

```rust
// 预读：范围查询时一次读取多个 Block
options.readahead_size = 256 * 1024; // 256KB

// 应用启动时预热
fn warmup_cache(db: &DB) -> Result<()> {
    println!("Warming up cache...");
    let start = Instant::now();
    
    // 方法 1：遍历热点前缀
    let prefixes = vec![b"user:", b"session:", b"config:"];
    for prefix in prefixes {
        let mut iter = db.scan(Some(prefix), None)?;
        let mut count = 0;
        while iter.valid() && count < 10000 {
            let _ = iter.value(); // 触发缓存加载
            iter.next();
            count += 1;
        }
    }
    
    println!("Warmup completed in {:?}", start.elapsed());
    Ok(())
}
```

### 4. 批量读取优化

```rust
// ✅ 推荐：使用范围查询
fn read_range(db: &DB, start: &[u8], end: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut results = Vec::new();
    let mut iter = db.scan(Some(start), Some(end))?;
    
    while iter.valid() {
        results.push(iter.value().to_vec());
        iter.next();
    }
    
    Ok(results)
}

// ❌ 避免：多次点查询
fn read_multiple(db: &DB, keys: &[Vec<u8>]) -> Result<Vec<Option<Vec<u8>>>> {
    let mut results = Vec::new();
    for key in keys {
        results.push(db.get(key)?);
    }
    Ok(results)
}

// 性能对比
let keys: Vec<_> = (0..1000).map(|i| format!("key{}", i).into_bytes()).collect();

// 范围查询：~1-5ms
let start = Instant::now();
read_range(&db, b"key0", b"key999")?;
println!("Range: {:?}", start.elapsed());

// 多次点查询：~10-50ms
let start = Instant::now();
read_multiple(&db, &keys)?;
println!("Multiple: {:?}", start.elapsed());
```

---

## 内存管理

### 内存分配

```
总内存需求 = MemTable + Block Cache + 系统开销

组成：
- MemTable: memtable_size × 2（mutable + immutable）
- Block Cache: block_cache_size
- 系统开销: ~20% 总内存

示例（8GB 可用内存）：
- MemTable: 16MB × 2 = 32MB
- Block Cache: 512MB
- 系统开销: ~100MB
- 总计: ~644MB
- 剩余: ~7.4GB（供应用使用）
```

### 内存配置策略

```rust
fn configure_memory(
    total_memory: usize,
    workload_type: WorkloadType,
) -> Options {
    let mut options = Options::default();
    
    match workload_type {
        WorkloadType::ReadHeavy => {
            // 读密集：大 Cache，小 MemTable
            options.block_cache_size = total_memory / 2;
            options.memtable_size = 4 * 1024 * 1024;
        }
        WorkloadType::WriteHeavy => {
            // 写密集：大 MemTable，中等 Cache
            options.memtable_size = 16 * 1024 * 1024;
            options.block_cache_size = total_memory / 4;
        }
        WorkloadType::Balanced => {
            // 均衡：平衡分配
            options.memtable_size = 8 * 1024 * 1024;
            options.block_cache_size = total_memory / 3;
        }
    }
    
    options
}
```

### 监控内存使用

```rust
fn monitor_memory(db: &DB) {
    let stats = db.get_stats().unwrap();
    
    println!("Memory Usage:");
    println!("  MemTable: {} MB", stats.memtable_size / 1024 / 1024);
    println!("  Block Cache: {} MB", stats.block_cache_size / 1024 / 1024);
    println!("  Total: {} MB", 
             (stats.memtable_size + stats.block_cache_size) / 1024 / 1024);
}
```

---

## 磁盘优化

### 1. 选择合适的存储

| 存储类型 | 读取IOPS | 写入IOPS | 延迟 | 适用场景 |
|---------|---------|---------|------|---------|
| HDD | ~100 | ~100 | 10ms | 冷数据归档 |
| SATA SSD | ~10K | ~10K | 1ms | 一般应用 |
| NVMe SSD | ~100K | ~50K | 0.1ms | 高性能应用 |

### 2. 文件系统优化

```bash
# 使用 XFS（推荐）
mkfs.xfs /dev/nvme0n1
mount -o noatime,nodiratime /dev/nvme0n1 /data

# 或使用 ext4
mkfs.ext4 /dev/nvme0n1
mount -o noatime,data=writeback /dev/nvme0n1 /data

# 启用 TRIM（SSD）
fstrim -v /data

# 禁用 swap（可选，提高性能）
swapoff -a
```

### 3. IO 调度器

```bash
# 查看当前调度器
cat /sys/block/nvme0n1/queue/scheduler

# 对于 SSD/NVMe，使用 none 或 noop
echo none > /sys/block/nvme0n1/queue/scheduler

# 对于 HDD，使用 deadline
echo deadline > /sys/block/sda/queue/scheduler
```

### 4. 压缩优化

```rust
let mut options = Options::default();

// 选择压缩算法
match workload {
    Workload::CompressionHeavy => {
        // 数据可压缩性高（如文本、JSON）
        options.enable_compression = true;
        options.compression_type = CompressionType::Snappy;
        // 压缩率：~50-70%
        // CPU 开销：低
    }
    Workload::AlreadyCompressed => {
        // 数据已压缩（如图片、视频）
        options.enable_compression = false;
        // 避免浪费 CPU
    }
    Workload::CPULimited => {
        // CPU 受限
        options.enable_compression = false;
        // 或使用 LZ4（更快）
        options.compression_type = CompressionType::LZ4;
    }
}
```

---

## Compaction 调优

### 理解 Compaction

```
Level 0: 100MB (10 files)  ──┐
                              ├─→ Compact ──→ Level 1: 200MB (5 files)
Level 1: 150MB (8 files)   ──┘

触发条件：
- Level 0 文件数 > 4
- Level N 总大小 > 阈值
- 手动触发
```

### 配置 Compaction

```rust
let mut options = Options::default();

// Level 0 触发阈值
options.level0_file_num_compaction_trigger = 4;
// Level 0 文件数达到 4 个时触发 compaction

// Level 0 减速阈值
options.level0_slowdown_writes_trigger = 8;
// Level 0 文件数达到 8 个时减速写入

// Level 0 停止阈值
options.level0_stop_writes_trigger = 12;
// Level 0 文件数达到 12 个时停止写入

// 各层大小倍数
options.level_size_multiplier = 10;
// Level N+1 = Level N × 10
```

### 手动 Compaction

```rust
// 场景 1：低峰期手动 Compaction
fn scheduled_compaction(db: &DB) -> Result<()> {
    // 检查是否需要 Compaction
    let stats = db.get_stats()?;
    if stats.num_sstables > 50 {
        println!("Starting manual compaction...");
        let start = Instant::now();
        
        db.compact_range(None, None)?;
        
        println!("Compaction completed in {:?}", start.elapsed());
    }
    Ok(())
}

// 场景 2：特定范围 Compaction
fn compact_user_data(db: &DB) -> Result<()> {
    db.compact_range(Some(b"user:"), Some(b"user:z"))?;
    Ok(())
}
```

### 监控 Compaction

```rust
fn monitor_compaction(db: &DB) {
    let stats = db.get_stats().unwrap();
    
    println!("Compaction Stats:");
    println!("  SSTables: {}", stats.num_sstables);
    println!("  Level 0 files: {}", stats.level0_files);
    
    // 告警
    if stats.level0_files > 6 {
        println!("Warning: Too many Level 0 files!");
        println!("Consider manual compaction or increasing write capacity");
    }
}
```

---

## 性能诊断

### 性能分析工具

```rust
use std::time::Instant;

// 1. 操作延迟分析
fn measure_latency(db: &DB) {
    let mut write_latencies = Vec::new();
    let mut read_latencies = Vec::new();
    
    // 测试写入延迟
    for i in 0..1000 {
        let start = Instant::now();
        db.put(format!("key{}", i).as_bytes(), b"value").unwrap();
        write_latencies.push(start.elapsed());
    }
    
    // 测试读取延迟
    for i in 0..1000 {
        let start = Instant::now();
        db.get(format!("key{}", i).as_bytes()).unwrap();
        read_latencies.push(start.elapsed());
    }
    
    // 计算统计
    print_percentiles("Write", &write_latencies);
    print_percentiles("Read", &read_latencies);
}

fn print_percentiles(name: &str, latencies: &[Duration]) {
    let mut sorted = latencies.to_vec();
    sorted.sort();
    
    let p50 = sorted[sorted.len() * 50 / 100];
    let p95 = sorted[sorted.len() * 95 / 100];
    let p99 = sorted[sorted.len() * 99 / 100];
    
    println!("{} Latency:", name);
    println!("  P50: {:?}", p50);
    println!("  P95: {:?}", p95);
    println!("  P99: {:?}", p99);
}
```

### 性能瓶颈定位

```rust
fn diagnose_performance(db: &DB) -> Result<()> {
    let stats = db.get_stats()?;
    
    println!("=== Performance Diagnosis ===\n");
    
    // 1. 检查缓存命中率
    if stats.block_cache_hit_rate < 0.7 {
        println!("❌ Problem: Low cache hit rate ({:.2}%)", 
                 stats.block_cache_hit_rate * 100.0);
        println!("✅ Solution: Increase block_cache_size");
        println!();
    }
    
    // 2. 检查 Level 0 文件数
    if stats.level0_files > 8 {
        println!("❌ Problem: Too many Level 0 files ({})", 
                 stats.level0_files);
        println!("✅ Solution: Trigger manual compaction or reduce write rate");
        println!();
    }
    
    // 3. 检查总 SSTable 数
    if stats.num_sstables > 100 {
        println!("❌ Problem: Too many SSTables ({})", 
                 stats.num_sstables);
        println!("✅ Solution: Compact database");
        println!();
    }
    
    // 4. 检查磁盘使用
    let disk_usage = get_disk_usage(&db)?;
    if disk_usage > 0.8 {
        println!("❌ Problem: Low disk space ({:.2}% used)", 
                 disk_usage * 100.0);
        println!("✅ Solution: Clean up old data or add storage");
        println!();
    }
    
    println!("=== End Diagnosis ===");
    Ok(())
}
```

---

## 压测指南

### 基准测试套件

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_writes(c: &mut Criterion) {
    let db = DB::open("./bench_db", Options::default()).unwrap();
    
    c.bench_function("write_single", |b| {
        let mut i = 0;
        b.iter(|| {
            db.put(
                format!("key{}", i).as_bytes(),
                black_box(b"value")
            ).unwrap();
            i += 1;
        });
    });
    
    c.bench_function("write_batch", |b| {
        b.iter(|| {
            let mut batch = WriteBatch::new();
            for i in 0..1000 {
                batch.put(
                    format!("key{}", i).as_bytes(),
                    b"value"
                );
            }
            db.write(black_box(batch)).unwrap();
        });
    });
}

criterion_group!(benches, benchmark_writes);
criterion_main!(benches);
```

### 压力测试

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn stress_test(
    db: Arc<DB>,
    duration: Duration,
    num_threads: usize,
) -> TestResults {
    let start = Instant::now();
    let write_count = Arc::new(AtomicU64::new(0));
    let read_count = Arc::new(AtomicU64::new(0));
    let error_count = Arc::new(AtomicU64::new(0));
    
    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            let db = Arc::clone(&db);
            let writes = Arc::clone(&write_count);
            let reads = Arc::clone(&read_count);
            let errors = Arc::clone(&error_count);
            let start = start.clone();
            
            thread::spawn(move || {
                let mut rng = rand::thread_rng();
                while start.elapsed() < duration {
                    let op: f64 = rng.gen();
                    
                    if op < 0.7 {
                        // 70% 写入
                        let key = format!("key:{}", rng.gen::<u64>());
                        match db.put(key.as_bytes(), b"value") {
                            Ok(_) => writes.fetch_add(1, Ordering::Relaxed),
                            Err(_) => errors.fetch_add(1, Ordering::Relaxed),
                        };
                    } else {
                        // 30% 读取
                        let key = format!("key:{}", rng.gen::<u64>());
                        match db.get(key.as_bytes()) {
                            Ok(_) => reads.fetch_add(1, Ordering::Relaxed),
                            Err(_) => errors.fetch_add(1, Ordering::Relaxed),
                        };
                    }
                }
            })
        })
        .collect();
    
    for handle in handles {
        handle.join().unwrap();
    }
    
    let elapsed = start.elapsed();
    let writes = write_count.load(Ordering::Relaxed);
    let reads = read_count.load(Ordering::Relaxed);
    let errors = error_count.load(Ordering::Relaxed);
    
    TestResults {
        duration: elapsed,
        writes,
        reads,
        errors,
        write_throughput: writes as f64 / elapsed.as_secs_f64(),
        read_throughput: reads as f64 / elapsed.as_secs_f64(),
    }
}

// 运行压测
fn main() {
    let db = Arc::new(DB::open("./test_db", Options::default()).unwrap());
    
    println!("Starting stress test...");
    let results = stress_test(
        db,
        Duration::from_secs(60),
        8, // 8 threads
    );
    
    println!("\n=== Results ===");
    println!("Duration: {:?}", results.duration);
    println!("Writes: {} ({:.2} ops/s)", 
             results.writes, results.write_throughput);
    println!("Reads: {} ({:.2} ops/s)", 
             results.reads, results.read_throughput);
    println!("Errors: {}", results.errors);
}
```

---

## 性能检查清单

### 写入性能

- [ ] 使用 WriteBatch 批量写入
- [ ] 调整 MemTable 大小（4-16MB）
- [ ] 考虑 WAL 同步策略
- [ ] 监控 Level 0 文件数
- [ ] 并发写入（CPU 核心数线程）

### 读取性能

- [ ] 配置足够的 Block Cache（内存的 30-50%）
- [ ] 启用 Bloom Filter
- [ ] 监控缓存命中率（目标 >80%）
- [ ] 使用范围查询代替多次点查询
- [ ] 预热热点数据

### 系统配置

- [ ] 使用 SSD（推荐 NVMe）
- [ ] 配置文件系统（noatime, nodiratime）
- [ ] 调整 IO 调度器
- [ ] 增加文件描述符限制
- [ ] 禁用 swap（可选）

### 监控指标

- [ ] 操作延迟（P99 < 5ms）
- [ ] 缓存命中率（> 80%）
- [ ] SSTable 数量（< 100）
- [ ] Level 0 文件数（< 8）
- [ ] 磁盘使用率（< 80%）

---

## 总结

### 性能优化优先级

1. **使用 WriteBatch**：最高 ROI
2. **启用 Bloom Filter**：显著减少磁盘读取
3. **调整 Block Cache**：提升缓存命中率
4. **优化键设计**：提高 Compaction 效率
5. **使用 SSD**：硬件升级立即见效

### 进一步学习

- [用户指南](USER_GUIDE.md) - 基础功能使用
- [最佳实践](BEST_PRACTICES.md) - 生产环境建议
- [架构设计](ARCHITECTURE.md) - 了解内部原理

---

**性能问题？** 欢迎在 [GitHub Issues](https://github.com/yourusername/aidb/issues) 提问！
