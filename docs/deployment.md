---
name: aidb-deployment
description: AiDb 依赖嵌入、生产配置调优与运维实践指南 (How to Embed & Operate). 包含 Cargo feature 组合、多线程嵌入范式、数据目录规划、Options 调优与监控备份集成.
---

# AiDb 部署与运行

本文面向系统集成方 (如 [wiqun/AiKv](https://github.com/wiqun/AiKv)) 与运维工程师, 说明 **如何引入 AiDb 依赖、规划硬件与数据目录、进行生产配置调优, 以及落地备份恢复与可观测性**.

> **重要说明**: AiDb 是纯 Rust **嵌入式 lib crate**, 无独立守护进程, 亦不自建对外网络服务. 若需独立 Redis RESP 服务或集群运维, 请直接部署 **[wiqun/AiKv](https://github.com/wiqun/AiKv)**.

---

## 1. 系统要求与硬件规划

| 资源维度 | 生产推荐配置 | 最低运行要求 | 运维与规划说明 |
| --- | --- | --- | --- |
| **Rust 工具链** | Rust **stable** | 声明于 `rust-toolchain.toml` | 编译构建需要, 包含 `clippy` 与 `rustfmt` |
| **操作系统** | Linux x86_64 | Linux x86_64 | 正式验证平台; 其他平台仅提供 best-effort 支持 |
| **磁盘存储** | 高性能 NVMe SSD | 标准 SSD / HDD | LSM 写 WAL 与 Compaction 读写高度依赖磁盘 IOPS |
| **内存容量** | 4 GiB ~ 32 GiB+ | 512 MiB | 与 `Options` 中 MemTable、BlockCache 容量及活跃并发数正相关 |
| **Protobuf 编译器** | `protoc` (最新 stable) | 系统包管理器自带版本 | 仅编译 `cluster` feature 时需要; 缺少 protoc 时构建失败 |

---

## 2. Cargo Features 构建矩阵

定义声明见 [Cargo.toml](../Cargo.toml). 嵌入方可根据运行场景按需开启特性:

| Feature | 默认状态 | 包含模块与外部依赖 | 适用场景 |
| --- | --- | --- | --- |
| `default` | ✅ | 包含 `backup` 与核心 LSM 引擎 | 单机嵌入且需要本地全量备份 |
| (engine only) | — | 仅 `src/engine/*`, 零外部可选依赖 | 极简单机嵌入 (`--no-default-features`) |
| `backup` | ✅ | `src/backup/*`, 依赖 `ring`, `hex`, `serde_json` | 单机全量备份与数据恢复 (`BackupManager`) |
| `compression` | ❌ | SSTable 块压缩, 依赖 `snap`, `lz4` | 节省磁盘空间, 启用时 `Options` 默认使用 Snap |
| `cluster` | ❌ | `src/cluster/*`, 依赖 `openraft`, `tonic`, `tokio` | MetaRaft + MultiRaft 16384 槽位集群共识 |
| `monitoring` | ❌ | `src/metrics.rs`, 依赖 `opentelemetry` 系列 | 内部指标埋点 (`aidb_*`), 配合宿主 OTLP 导出 |

### 典型构建命令

```bash
# 1. 单机基础构建 (默认启用 backup)
cargo build --release

# 2. 最小单机构建 (仅核心存储引擎, 零额外依赖)
cargo build --release --no-default-features

# 3. 生产全功能构建 (块压缩 + 集群共识 + 指标埋点)
cargo build --release --features compression,cluster,monitoring
```

---

## 3. 作为依赖的嵌入集成范式

### 3.1 声明依赖 (`Cargo.toml`)

在宿主应用的 `Cargo.toml` 中引入:

```toml
[dependencies]
# 方式 A: 来自 GitHub main 分支
aidb = { git = "https://github.com/wiqun/AiDb.git", branch = "main", default-features = false, features = ["backup", "compression", "monitoring"] }

# 方式 B: 本地 Monorepo 路径依赖 (如 AiKv 引入)
aidb = { path = "../aidb", features = ["backup", "compression", "cluster", "monitoring"] }
```

### 3.2 多线程安全共享 (`Arc<DB>`)

`aidb::DB` 实现了 `Send + Sync`, 推荐使用 `Arc<DB>` 在多个工作线程间共享实例:

```rust
use std::sync::Arc;
use aidb::{DB, config::Options};

fn main() -> aidb::Result<()> {
    // 1. 初始化 Options 并打开数据库
    let mut options = Options::for_high_write_throughput();
    options.create_if_missing = true;

    let db = Arc::new(DB::open("/var/data/aidb", options)?);

    // 2. 多线程并发读写
    let mut handles = Vec::new();
    for i in 0..4 {
        let db_cloned = Arc::clone(&db);
        handles.push(std::thread::spawn(move || {
            let key = format!("user:{}", i).into_bytes();
            let val = format!("data_{}", i).into_bytes();
            let _ = db_cloned.put(&key, &val);
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // 3. 优雅关闭 (内部触发 Flush 并释放文件锁)
    db.close()?;
    Ok(())
}
```

### 3.3 与 Tokio 异步运行时桥接

由于 `aidb::DB` 的公共 API 为同步阻塞设计, 在 Tokio 异步服务中应通过 `spawn_blocking` 进行调度, 避免阻塞异步 Worker 线程:

```rust
let db_clone = Arc::clone(&db);
let value = tokio::task::spawn_blocking(move || {
    db_clone.get(b"target_key")
}).await.map_err(|e| aidb::Error::Internal(e.to_string()))??;
```

### 3.4 版本兼容与数据迁移

`1.x` 遵循 Cargo SemVer, 保持公开 Rust API 兼容. 此兼容承诺不包含持久化数据格式
和集群跨版本互操作:

- v1 不保证读取 v1 之前版本生成的 WAL, MANIFEST, SSTable, Raft snapshot 或
  migration checkpoint.
- v1 不支持与 v1 之前版本进行集群滚动升级.
- 从开发版本升级到 v1 时, 应使用新数据目录, 或通过应用层导入导出迁移数据.

生产构建和运行的正式验证平台为 Linux x86_64. 其他平台仅提供 best-effort 支持,
部署前应由集成方自行完成构建和数据恢复验证.

---

## 4. 数据目录结构与文件生命周期

单机 `DB::open(path, opts)` 指定的 `path` 即为数据存储根目录:

```shell
/var/data/aidb/
├── LOCK                 # 进程互斥锁 (保证单进程独占, 冲突返回 Error::Busy)
├── CURRENT              # ASCII 指针文件, 记录当前生效的 MANIFEST 文件名
├── MANIFEST-000001      # LSM 版本变更元数据日志 (原子记录 SSTable 增删)
├── 000002_L0.sst        # L0 SSTable 文件 (Key 范围可能重叠)
├── 000003_L1.sst        # L1 SSTable 文件 (同层 Key 严格排序且不重叠)
└── wal_000004.log       # 活跃预写日志文件 (数据落盘后自动清理)
```

### 核心文件生命周期

1. **`LOCK` (排他文件锁)**: `DB::open` 时通过文件锁抢占; 数据库关闭或进程崩溃时操作系统自动释放. 若非正常退出导致残留锁报错, 确认无并发进程后重新打开即可.
2. **`CURRENT` & `MANIFEST-*`**: 任何 MemTable Flush 或 Compaction 完成后, 先将变更以 `VersionEdit` 形式追加到 `MANIFEST`, 随后原子更新 `CURRENT`, 确保崩溃后元数据处于一致状态.
3. **`*.sst` (持久化数据表)**: SSTable 文件一旦生成即为**只读不可变**. Compaction 产生新层 SSTable 后, 淘汰的旧 SSTable 会在无活跃 Snapshot 引用时被安全删除.
4. **`wal_*.log` (预写日志)**: 记录未落盘的原子写入. 当对应的 MemTable 成功落盘为 L0 SSTable 后, 该 WAL 日志由后台线程自动归档删除.

---

## 5. 生产配置参数与 Presets 调优

### 5.1 核心参数调优指南 (`src/config.rs`)

| 参数项 | 默认基准 | 调优建议与适用场景 |
| --- | --- | --- |
| `create_if_missing` | `true` | 首次运行自动创建目录; 严格生产部署可前置由运维创建并校验权限 |
| `write_buffer_size` / `memtable_size` | `64 MiB` | 写密集业务建议调大至 `128 MiB ~ 256 MiB`, 提升连续写吞吐并降低 Flush 频率 |
| `block_cache_size` | `64 MiB` | 读密集业务建议调大至物理内存的 30%~50% (如 `4 GiB ~ 16 GiB`), 显著降低物理 IO |
| `use_wal` | `true` | 必须为 `true`. 仅在批量离线导入临时数据且容忍丢失时可设为 `false` |
| `sync_wal` | `false` | 默认为 OS 异步刷盘 (高性能); 金融级强一致要求可设为 `true` (每条操作 fsync, 吞吐显著下降) |
| `bloom_false_positive_rate` | `0.01` | 默认 1% 误判率 (每 Key 约 10 bit); 读密集冷热分明场景可设为 `0.001` (每 Key 约 14 bit) |
| `background_compaction` | `true` | 生产必须开启; 允许后台线程自动执行 Leveled Compaction |
| `compaction_threads` | `1` | 多核服务器且写入量大时建议设为 `2 ~ 4`, 加速大合并以压制 Write Stall |

### 5.2 开箱即用 Presets

```rust
// 1. 测试预设: 极小内存占用, 快速启动
let opts = Options::for_testing();

// 2. 高吞吐写入预设: 大 MemTable, 启用 Snap 压缩, 调优 Flush 触发线
let opts = Options::for_high_write_throughput();

// 3. 高吞吐读取预设: 大 BlockCache, 低 Bloom 误判率, 禁用压缩以降低 CPU 解压开销
let opts = Options::for_high_read_throughput();
```

### 5.3 集群配置 (`ClusterConfig`)

当启用 `cluster` feature 时, 可通过预设初始化分布式集群参数:

```rust
use aidb::cluster::ClusterConfig;

// 生产集群配置: 256 个 MultiRaft Group, 3 副本冗余
let cluster_cfg = ClusterConfig::for_production();

// 单机集成测试配置: 4 个 MultiRaft Group, 1 副本
let test_cluster_cfg = ClusterConfig::for_testing();
```

---

## 6. 备份与恢复运维实践 (`feature = "backup"`)

### 6.1 创建全量备份与保留策略

```rust
use std::sync::Arc;
use aidb::{DB, config::Options};
use aidb::backup::{BackupManager, LocalFileStorage, RetentionPolicy};

fn create_daily_backup(db: &DB) -> aidb::Result<u64> {
    // 1. 初始化备份存储目录与保留策略 (最多保留 7 份历史备份)
    let storage = Arc::new(LocalFileStorage::new("/var/backups/aidb"));
    let policy = RetentionPolicy { max_backups: 7, ..Default::default() };
    let manager = BackupManager::new(storage, policy);

    // 2. 执行 Checkpoint 快照、逐文件 SHA256 校验与元数据归档
    let backup_id = manager.create_backup(db)?;
    println!("Backup completed successfully, ID: {}", backup_id);
    Ok(backup_id)
}
```

### 6.2 数据完整性校验与原子恢复

```rust
use std::sync::Arc;
use aidb::backup::{LocalFileStorage, RecoveryManager};

fn restore_from_backup(backup_id: u64, target_dir: &str) -> aidb::Result<()> {
    let storage = Arc::new(LocalFileStorage::new("/var/backups/aidb"));
    let recovery = RecoveryManager::new(storage);

    // 1. 校验备份元数据与文件哈希
    if !recovery.verify_backup(backup_id)? {
        return Err(aidb::Error::Corruption("Backup SHA256 verification failed".into()));
    }

    // 2. 原子恢复至目标目录 (经临时目录冒烟自检后原子 rename)
    recovery.restore(backup_id, target_dir)?;
    println!("Restore completed to {}", target_dir);
    Ok(())
}
```

---

## 7. 可观测性接入运维 (`feature = "monitoring"`)

AiDb 遵循库与应用解耦原则, 内部不自建 HTTP Exporter.

### 7.1 Tracing 链路日志配置

宿主应用使用 `tracing-subscriber` 接收引擎日志. 引擎热路径埋点均为 `debug` 级别:

```rust
// 生产环境推荐配置: 过滤引擎 debug 日志, 仅在关注模块打开 trace/debug
tracing_subscriber::fmt()
    .with_env_filter("info,aidb=info,aidb::engine::compaction=debug")
    .init();
```

### 7.2 OpenTelemetry 指标导出

宿主应用 (如 AiKv) 负责初始化全局 `MeterProvider` 并注册 OTLP 导出管道:

```rust
// 宿主初始化全局 Provider 后, 调用 aidb 注册内部指标
aidb::metrics::init(); // 注册 aidb_* 指标
```

详细指标列表与 Grafana 监控大盘配置参考 [05-observability.md](modules/05-observability.md).

---

## 8. 集群运维边界说明

| 运维职责领域 | AiDb 内部处理 | 上层网络服务 (AiKv) / 外部运维处理 |
| --- | --- | --- |
| **节点间通信** | 基于 `proto/raft.proto` 的 gRPC 协议传输与分发 | 网络端口暴露、TLS 证书配置与防火墙规则 |
| **槽位路由** | CRC16 槽位计算、16384 槽分配表与迁移执行器 | 接收客户端请求、返回 `MOVED`/`ASK` 转向错误 |
| **集群拓扑命令** | MetaRaft 拓扑状态机变更 | `CLUSTER NODES`, `CLUSTER MEET`, `CLUSTER SLOTS` 等命令解析 |
| **服务生命周期** | Group 异常指数退避就地自愈 (`supervise_groups`) | 物理容器重启、Docker Compose 编排与 K8s StatefulSet 调度 |
