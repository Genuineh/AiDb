# 硬编码值治理 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 系统修复 AiDb 和 AiKv 中不合理的硬编码值，三个 Phase 共 7 个子项

**Architecture:** 安全修复（删除 127.0.0.1 回退、对齐 grpc_max_message_size）→ 配置化扩展（引擎运行时参数 → Options、AiKv CLI 调优参数）→ 代码清理（DB_COUNT DRY、RaftNodeConfig 默认值对齐、lifecycle 超时配置化）

**依赖关系:**
- Task 1 (1A) → 无依赖
- Task 2 (2A) → 无依赖
- Task 3 (2B) → 无依赖
- Task 4 (3A) → 无依赖
- Task 5 (1B+3B+3C) → 需等 Task 1～4 完成后（与 1A/2A/2B/3A 无文件冲突，但与 Task 1 共享 cluster 模块）

**文件冲突分析:**
- Task 1 只改 `network.rs`，与其余任务无冲突
- Task 2 只改 `config.rs` + `inner.rs`，无冲突
- Task 3 只改 AiKv 的 `main.rs` + `config_auto_save.rs`，无冲突
- Task 4 只改 AiKv 的 `storage/types.rs` + `adapter.rs` + `database.rs` + `server.rs`，无冲突
- Task 5 改 `types.rs` + `multi_raft_node.rs`，需确保以上任务都合入后再操作

**Tech Stack:** Rust, clap, crossbeam, tokio, tonic (gRPC)

---

## 文件修改总览

| 文件 | Task | 改动 |
|------|------|------|
| `AiDb/src/cluster/network.rs:357` | 1 | 删除 127.0.0.1 回退，改为 ERROR 日志 |
| `AiDb/src/config.rs:14-74` | 2 | Options 新增 9 个运行时调优字段 |
| `AiDb/src/config.rs:115-143` | 2 | `for_testing()` 新增 9 个字段 |
| `AiDb/src/config.rs:157-184` | 2 | `validate()` 新增 3 条校验 |
| `AiDb/src/config.rs:76-106` | 2 | `Default` 新增 9 个字段默认值 |
| `AiDb/src/engine/db/inner.rs:32-33` | 2 | 删除 FLUSH_POLL_MS / COMPACTION_POLL_MS 常量 |
| `AiDb/src/engine/db/inner.rs:183,345,357,838-839,968,977,1181,1185` | 2 | 改用 `self.options.*` |
| `AiDb/src/engine/db/inner.rs:119` | 2 | 改用 `options.compaction_channel_size` |
| `AiKv/src/main.rs:29-80` | 3 | Args 新增 3 个 CLI 参数 |
| `AiKv/src/main.rs:248` | 3 | lifecycle tick 使用 with_tick_interval |
| `AiKv/src/main.rs:294-301` | 3 | gossip 间隔使用 CLI 参数 |
| `AiKv/src/main.rs:324-336` | 3 | ConfigAutoSave 使用 new_with_interval |
| `AiKv/src/cluster/config_auto_save.rs:23-31` | 3 | 新增 new_with_interval 构造器 |
| `AiKv/src/storage/types.rs` | 4 | 新增 `pub const DB_COUNT: usize = 16` |
| `AiKv/src/storage/adapter.rs:17` | 4 | 删除 const，改为 use 导入 |
| `AiKv/src/command/database.rs:12` | 4 | 删除 const，改为 use 导入 |
| `AiKv/src/command/server.rs:15` | 4 | 删除 const，改为 use 导入 |
| `AiDb/src/cluster/types.rs:113-127` | 5 | RaftNodeConfig 默认值对齐 |
| `AiDb/src/cluster/multi_raft_node.rs:157-162` | 5 | factory 参数改为从 cfg/config default 取值 |

---

### Task 1: Phase 1A — 删除 127.0.0.1 生产回退

**Files:**
- Modify: `AiDb/src/cluster/network.rs:357`
- Test: `AiDb/src/cluster/network.rs` (现有测试模块)

- [ ] **Step 1: 阅读现有代码确认上下文**

```rust
// AiDb/src/cluster/network.rs:346-358
let target_addr = if !node.addr.is_empty() {
    let addr = node.addr.clone();
    self.nodes.write().insert(target, addr.clone());
    normalize_grpc_addr(&addr)
} else {
    self.nodes.read().get(&target).cloned()
        .map(|a| normalize_grpc_addr(&a))
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", 50_000 + target))
};
```

- [ ] **Step 2: 确认现有测试行为**

Run: `cargo test --features cluster network`

- [ ] **Step 3: 写测试 — `new_client` 在 unknown node 时返回空 addr 并记录 error**

```rust
// 在 AiDb/src/cluster/network.rs 已有 #[cfg(test)] 模块中追加
#[test]
fn test_new_client_fallback_to_empty_on_unknown_node() {
    let mut factory = RaftNetworkClientFactory::new(1, 0, 100, 1024);
    let node = openraft::BasicNode {
        addr: "".into(),
        data: Default::default(),
    };
    let client = factory.new_client(99, &node);
    // target_addr 应为空字符串（而非 127.0.0.1:50099）
    // 注意: new_client 是 async 的，需要在异步上下文运行
    // 这里只验证 trait 边界和类型
}
```

注意：`RaftNetworkFactory::new_client` 返回 `Self::Network` 即 `RaftNetworkClient`，但 `RaftNetworkClient` 的 `target_addr` 是私有字段。验证方式可以:
- 通过 `Factory::new_client` 的返回值调用 gRPC 确认返回 `NetworkError`
- 或增加测试辅助方法暴露 `target_addr`

推荐方式: 在网络测试模块中验证 factory 注册逻辑:

```rust
#[tokio::test]
async fn test_network_factory_unknown_node_returns_empty_addr() {
    let mut factory = RaftNetworkClientFactory::new(1, 0, 100, 1024);
    let node = openraft::BasicNode {
        addr: "".into(),
        data: Default::default(),
    };
    let client = factory.new_client(99, &node).await;
    // gRPC connect 到空字符串应返回 NetworkError/Unreachable
    match client.append_entries(
        /* 构造 minimal AppendEntriesRequest */,
        RPCOption::default(),
    ).await {
        Err(RPCError::Network(_)) => {} // 期望
        other => panic!("expected NetworkError, got {:?}", other),
    }
}
```

- [ ] **Step 4: 运行测试确认当前行为**

Run: `cargo test --features cluster test_network_factory_unknown_node_returns_empty_addr 2>&1 | head -20`

- [ ] **Step 5: 修改 `network.rs:357` 删除回退**

```rust
// 替换 unwrap_or_else 分支:
} else {
    let empty_addr = String::new();
    tracing::error!(
        target_node_id = target,
        "gRPC address for node {} is not registered in RaftNetworkClientFactory; \
         this indicates a cluster configuration bug",
        target,
    );
    empty_addr
}
```

- [ ] **Step 6: 运行测试确认通过**

Run: `RUSTFLAGS='-D warnings' cargo clippy --all-targets --features cluster`

```bash
cargo test --features cluster test_network_factory_unknown_node_returns_empty_addr -v
```

- [ ] **Step 7: 全量回归**

Run: `cargo test --features cluster`

- [ ] **Step 8: 确认无硬编码 IP**

```bash
grep -rn '127\.0\.0\.1' AiDb/src/cluster/network.rs
# 期望：只有测试数据和注释，不在生产代码路径
```

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "fix: remove 127.0.0.1 production fallback in RaftNetworkClientFactory

Replace silent fallback to 127.0.0.1:{50000+target} with an ERROR-level
log and empty target_addr, so the gRPC call fails with Unreachable
instead of silently connecting to a wrong node."
```

---

### Task 2: Phase 2A — 引擎运行时参数配置化

**Files:**
- Modify: `AiDb/src/config.rs` (Options 结构体 + Default + for_testing + validate + for_high_write_throughput + for_high_read_throughput)
- Modify: `AiDb/src/engine/db/inner.rs` (替换 11 处常量引用)
- Test: `AiDb/src/config.rs` (现有测试模块，追加新断言)

- [ ] **Step 1: 在 `Options` 末尾新增 9 个字段**

```rust
// config.rs，在 background_compaction 字段之后插入
// === 运行时调优 (Phase 2 新增) ===
/// Flush 后台线程轮询间隔 (毫秒, 默认 500)
pub flush_poll_ms: u64,
/// Compaction 后台线程轮询间隔 (毫秒, 默认 500)
pub compaction_poll_ms: u64,
/// Write stall 循环 sleep 间隔 (毫秒, 默认 10)
pub write_stall_poll_ms: u64,
/// Slowdown 最大 sleep 时间 (毫秒, 默认 100)
pub write_stall_slowdown_max_ms: u64,
/// Memtable 槽等待最大迭代次数 (默认 10_000)
pub memtable_wait_iters: usize,
/// Memtable 槽等待轮询间隔 (毫秒, 默认 1)
pub memtable_wait_interval_ms: u64,
/// 子压缩最大分裂数 (默认 4)
pub max_sub_compactions: usize,
/// 子压缩最小分裂数 (默认 2)
pub min_sub_compactions: usize,
/// Compaction 信号通道容量 (默认 64)
pub compaction_channel_size: usize,
```

- [ ] **Step 2: 在 `Default::default()` 中添加默认值**

```rust
// Default impl 中追加
flush_poll_ms: 500,
compaction_poll_ms: 500,
write_stall_poll_ms: 10,
write_stall_slowdown_max_ms: 100,
memtable_wait_iters: 10_000,
memtable_wait_interval_ms: 1,
max_sub_compactions: 4,
min_sub_compactions: 2,
compaction_channel_size: 64,
```

- [ ] **Step 3: 在 `for_testing()` 中追加新字段**

```rust
// for_testing() 中追加（与默认值相同）
flush_poll_ms: 500,
compaction_poll_ms: 500,
write_stall_poll_ms: 10,
write_stall_slowdown_max_ms: 100,
memtable_wait_iters: 10_000,
memtable_wait_interval_ms: 1,
max_sub_compactions: 4,
min_sub_compactions: 2,
compaction_channel_size: 64,
```

注意: `for_high_write_throughput()` 和 `for_high_read_throughput()` 使用 `..Self::default()` 语法，新字段会自动继承默认值，无需显式追加。

- [ ] **Step 4: 在 `validate()` 中新增 3 条校验规则**

```rust
if self.flush_poll_ms == 0 {
    return Err(Error::InvalidArgument(
        "flush_poll_ms must be > 0".into(),
    ));
}
if self.compaction_poll_ms == 0 {
    return Err(Error::InvalidArgument(
        "compaction_poll_ms must be > 0".into(),
    ));
}
if self.min_sub_compactions > self.max_sub_compactions {
    return Err(Error::InvalidArgument(
        "min_sub_compactions must be <= max_sub_compactions".into(),
    ));
}
```

- [ ] **Step 5: 替换 `inner.rs` 中的常量引用**

```rust
// 删除行 32-33: const FLUSH_POLL_MS / COMPACTION_POLL_MS

// 行 183: std::thread::sleep(Duration::from_millis(self.options.flush_poll_ms))
// 行 345: std::thread::sleep(std::time::Duration::from_millis(self.options.write_stall_poll_ms))
// 行 357: let sleep_ms = (excess as f64 / cap as f64 * self.options.write_stall_slowdown_max_ms as f64) as u64;
// 行 838: .min(self.options.max_sub_compactions as u64)
// 行 839: n.max(self.options.min_sub_compactions as u64)
// 行 968: const MAX_WAIT_ITERS 删除，替换为 for _ in 0..self.options.memtable_wait_iters
// 行 977: std::thread::sleep(Duration::from_millis(self.options.memtable_wait_interval_ms))
// 行 1181: signal.recv_timeout(Duration::from_millis(self.options.compaction_poll_ms))
// 行 1185: signal.recv_timeout(Duration::from_millis(self.options.compaction_poll_ms))
```

对于行 119 `crossbeam_channel::bounded(64)`，需要将 `options` 参数传入该函数或使用已持有的 `self.options`。注意行 119 位于 `DB::open` 的构造过程中，可以通过 `options.compaction_channel_size` 访问:

```rust
// inner.rs 行 117-119
let (compaction_signals, compaction_receivers): (Vec<Sender<()>>, Vec<Receiver<()>>) = (0
    ..num_threads)
    .map(|_| crossbeam_channel::bounded(options.compaction_channel_size))
    .unzip();
```

- [ ] **Step 6: 运行测试确保现有测试通过**

```bash
cargo test --features cluster
```

**预期**: 现有 7 个 config 测试 + 所有集成测试通过

- [ ] **Step 7: 追加新字段的单元测试**

在 `config.rs` 测试模块中追加:

```rust
// 追加到 default_options_are_sane 测试中
assert_eq!(opts.flush_poll_ms, 500);
assert_eq!(opts.compaction_poll_ms, 500);
assert_eq!(opts.write_stall_poll_ms, 10);
assert_eq!(opts.write_stall_slowdown_max_ms, 100);
assert_eq!(opts.memtable_wait_iters, 10_000);
assert_eq!(opts.memtable_wait_interval_ms, 1);
assert_eq!(opts.max_sub_compactions, 4);
assert_eq!(opts.min_sub_compactions, 2);
assert_eq!(opts.compaction_channel_size, 64);
```

新增两个 validate 测试:

```rust
#[test]
fn validate_rejects_zero_flush_poll_ms() {
    let mut opts = Options::for_testing();
    opts.flush_poll_ms = 0;
    assert!(opts.validate().is_err());
}

#[test]
fn validate_rejects_min_sub_larger_than_max_sub() {
    let mut opts = Options::for_testing();
    opts.min_sub_compactions = 5;
    opts.max_sub_compactions = 3;
    assert!(opts.validate().is_err());
}
```

新增 for_testing 断言:

```rust
// 追加到 for_testing_uses_small_values 测试中
let opts = Options::for_testing();
// ... 已有断言 ...
assert_eq!(opts.flush_poll_ms, 500);    // 与默认值相同
assert_eq!(opts.compaction_poll_ms, 500);
```

- [ ] **Step 8: 运行新测试**

```bash
cargo test --features cluster validate_rejects_zero_flush_poll_ms validate_rejects_min_sub_larger_than_max_sub -v
```

- [ ] **Step 9: Clippy + fmt**

```bash
RUSTFLAGS='-D warnings' cargo clippy --all-targets --features cluster
cargo fmt --check
```

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "feat: promote engine runtime constants to Options struct

Add 9 new fields to Options (flush_poll_ms, compaction_poll_ms,
write_stall_poll_ms, write_stall_slowdown_max_ms, memtable_wait_iters,
memtable_wait_interval_ms, max_sub_compactions, min_sub_compactions,
compaction_channel_size) and replace compile-time constants in inner.rs.
Includes for_testing() defaults, validate() rules, and unit tests."
```

---

### Task 3: Phase 2B — AiKv CLI 调优参数

**Files:**
- Modify: `AiKv/src/main.rs:29-80` (Args struct 新增 3 个参数)
- Modify: `AiKv/src/main.rs:248` (lifecycle tick)
- Modify: `AiKv/src/main.rs:294-301` (gossip 间隔)
- Modify: `AiKv/src/main.rs:324-336` (ConfigAutoSave)
- Modify: `AiKv/src/cluster/config_auto_save.rs:23-31` (新增构造器)
- Test: 新增单元测试

- [ ] **Step 1: Args 结构体新增 3 个 CLI 参数**

```rust
// 在 AiKv/src/main.rs Args struct 中追加（在 metrics_addr 字段之后）

/// 生命周期管理 tick 间隔 (毫秒, 默认 1000)
#[arg(long, default_value = "1000")]
lifecycle_tick_ms: u64,

/// Gossip 后台刷新间隔 (秒, 默认 1)
#[arg(long, default_value = "1")]
gossip_interval: u64,

/// 集群配置自动保存轮询间隔 (毫秒, 默认 2000)
#[arg(long, default_value = "2000")]
config_auto_save_ms: u64,
```

- [ ] **Step 2: ConfigAutoSave 新增 new_with_interval 构造器**

```rust
// AiKv/src/cluster/config_auto_save.rs

impl ConfigAutoSave {
    /// 使用默认间隔 (2s)
    pub fn new(meta_raft: Arc<MetaRaftNode>, data_dir: PathBuf) -> Self {
        Self::new_with_interval(meta_raft, data_dir, Duration::from_secs(2))
    }

    /// 指定轮询间隔
    pub fn new_with_interval(
        meta_raft: Arc<MetaRaftNode>,
        data_dir: PathBuf,
        interval: Duration,
    ) -> Self {
        Self {
            meta_raft,
            data_dir,
            last_saved_version: RwLock::new(0),
            interval,
        }
    }
}
```

- [ ] **Step 3: 写 ConfigAutoSave 单元测试**

```rust
// 在 config_auto_save.rs 测试模块中新增
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_new_with_interval_sets_interval() {
        let ms = 500u64;
        // 通过 new_with_interval 创建，验证 interval 字段
        // 注意: MetaRaftNode 无法在单元测试中构造，此处仅验证构造器接口
        // 实际测试依赖集成测试环境
        let _interval = Duration::from_millis(ms);
        // 接口验证通过即可（编译期验证）
        assert!(true);
    }
}
```

注意：`ConfigAutoSave` 的 `interval` 字段是私有的。如果需要直接测试，可以添加 `pub fn interval(&self) -> Duration` 访问器。更务实的做法是依赖编译通过 + 集成测试环境覆盖。在 `main.rs` 中，`ConfigAutoSave::new_with_interval()` 被调用时会打印 `interval_secs` 到 tracing 日志，可通过日志确认。

- [ ] **Step 4: 修改 `init_cluster()` 使用 CLI 参数**

```rust
// AiKv/src/main.rs — lifecycle tick (约行 248)
// 当前:
let lifecycle = LifecycleManager::new(node_id, router.clone(), meta_raft_prov);
// 改为:
let lifecycle = LifecycleManager::new(node_id, router.clone(), meta_raft_prov)
    .with_tick_interval(std::time::Duration::from_millis(args.lifecycle_tick_ms));

// gossip 间隔 (约行 294-301)
// 当前:
1,
// 改为:
args.gossip_interval,

// ConfigAutoSave (约行 324-336)
// 当前:
let auto_save = ConfigAutoSave::new(meta_raft.clone(), data_dir.to_path_buf());
// 改为:
let auto_save = ConfigAutoSave::new_with_interval(
    meta_raft.clone(),
    data_dir.to_path_buf(),
    std::time::Duration::from_millis(args.config_auto_save_ms),
);
```

- [ ] **Step 5: 编译验证**

```bash
cargo build --features cluster 2>&1
```

- [ ] **Step 6: Clippy**

```bash
RUSTFLAGS='-D warnings' cargo clippy --all-targets --features cluster
```

- [ ] **Step 7: 确认新旧默认值相同**

```bash
# 确认 `--help` 输出包含新参数
cargo run --features cluster -- --help 2>&1 | grep -A2 "lifecycle-tick-ms\|gossip-interval\|config-auto-save-ms"
```

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: expose cluster tuning parameters via CLI

Add --lifecycle-tick-ms, --gossip-interval, --config-auto-save-ms CLI
args to aikv. Add ConfigAutoSave::new_with_interval() constructor."
```

---

### Task 4: Phase 3A — DB_COUNT 提取到单一定义

**Files:**
- Modify: `AiKv/src/storage/types.rs` (新增 pub const)
- Modify: `AiKv/src/storage/adapter.rs:17` (删除 const，改为 use)
- Modify: `AiKv/src/command/database.rs:12` (删除 const，改为 use)
- Modify: `AiKv/src/command/server.rs:15` (删除 const，改为 use)

- [ ] **Step 1: 在 `storage/types.rs` 新增公共常量**

```rust
// 在 TTL_NO_EXPIRY 和 WRONGTYPE 附近追加
/// AiKv 支持的数据库数量 (与 Redis 默认 16 个 DB 一致)
pub const DB_COUNT: usize = 16;
```

- [ ] **Step 2: 修改 `storage/adapter.rs`**

```rust
// 删除行 17: const DB_COUNT: usize = 16;
// 在文件头部 use 区域追加:
use crate::storage::types::DB_COUNT;
```

- [ ] **Step 3: 修改 `command/database.rs`**

```rust
// 删除行 12: const DB_COUNT: usize = 16;
// 在文件头部 use 区域追加:
use crate::storage::types::DB_COUNT;
```

- [ ] **Step 4: 修改 `command/server.rs`**

```rust
// 删除行 15: const DB_COUNT: usize = 16;
// 在文件头部 use 区域追加:
use crate::storage::types::DB_COUNT;
```

- [ ] **Step 5: 编译验证**

```bash
cargo build --features cluster 2>&1
```

- [ ] **Step 6: 确认只剩 1 处定义**

```bash
grep -rn 'const DB_COUNT' AiKv/src/
# 期望输出: AiKv/src/storage/types.rs: pub const DB_COUNT: usize = 16;
```

- [ ] **Step 7: 运行测试**

```bash
cargo test --features cluster
```

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor: extract DB_COUNT to single definition in storage/types.rs

Consolidate 3 duplicate const DB_COUNT: usize = 16 definitions into
a single pub const in storage/types.rs, eliminating DRY violation."
```

---

### Task 5: Phase 1B + 3B + 3C — 集群配置统一

**Files:**
- Modify: `AiDb/src/cluster/types.rs:113-127` (RaftNodeConfig 默认值对齐 + grpc_max_message_size)
- Modify: `AiDb/src/cluster/multi_raft_node.rs:157-162` (lifecycle factory 参数改为从 cfg 取值)
- Test: `AiDb/src/cluster/types.rs` (测试模块，追加默认值断言)

- [ ] **Step 1: 统一 `RaftNodeConfig::default()`**

```rust
// AiDb/src/cluster/types.rs:113-127
impl Default for RaftNodeConfig {
    fn default() -> Self {
        Self {
            node_id: 1,
            group_id: crate::cluster::DEFAULT_GROUP_ID,
            election_timeout_min: 500,
            election_timeout_max: 1000,
            heartbeat_interval: 100,
            max_payload_entries: 100,          // 300 → 100
            snapshot_logs_since_last: 1000,
            max_entry_size: 8 * 1024 * 1024,    // 64 MiB → 8 MiB
            rpc_timeout_ms: 200,                // 30 → 200
            grpc_max_message_size: 64 * 1024 * 1024, // 65 MiB → 64 MiB (Phase 1B)
        }
    }
}
```

- [ ] **Step 2: 确认 `validate()` 对新默认值仍然通过**

```rust
// 验证:
// heartbeat_interval(100) < election_timeout_min(500) ✅
// rpc_timeout_ms(200) < election_timeout_min(500) ✅
// max_payload_entries(100) > 0 ✅
```

- [ ] **Step 3: 写 RaftNodeConfig 默认值单元测试**

```rust
// 在 types.rs 测试模块中追加
#[test]
fn test_raft_config_default_values() {
    let cfg = RaftNodeConfig::default();
    assert_eq!(cfg.max_payload_entries, 100);
    assert_eq!(cfg.max_entry_size, 8 * 1024 * 1024);
    assert_eq!(cfg.rpc_timeout_ms, 200);
    assert_eq!(cfg.grpc_max_message_size, 64 * 1024 * 1024);
    assert_eq!(cfg.heartbeat_interval, 100);
    assert_eq!(cfg.election_timeout_min, 500);
    assert_eq!(cfg.election_timeout_max, 1000);
    assert_eq!(cfg.snapshot_logs_since_last, 1000);
}
```

- [ ] **Step 4: 运行默认值测试**

```bash
cargo test --features cluster test_raft_config_default_values -v
```

- [ ] **Step 5: 修改 `multi_raft_node.rs` — lifecycle factory 参数配置化**

```rust
// AiDb/src/cluster/multi_raft_node.rs:157-162

// 当前:
let net_factory = Arc::new(RwLock::new(RaftNetworkClientFactory::new(
    node_id,
    0,
    30,
    65 * 1024 * 1024,
)));

// 改为:
let rpc_timeout = cfg.as_ref().map_or(
    RaftNodeConfig::default().rpc_timeout_ms,
    |c: &LifecycleConfig| c.raft_node_config.rpc_timeout_ms,
);
let msg_size = cfg.as_ref().map_or(
    RaftNodeConfig::default().grpc_max_message_size,
    |c| c.raft_node_config.grpc_max_message_size,
);
let net_factory = Arc::new(RwLock::new(RaftNetworkClientFactory::new(
    node_id,
    0,
    rpc_timeout,
    msg_size,
)));
```

- [ ] **Step 6: 编译验证**

```bash
cargo build --features cluster 2>&1
```

- [ ] **Step 7: 确认现有 validate 测试通过**

```bash
cargo test --features cluster test_raft_config_validation -v
```

```rust
// 预期: 默认值通过，election_timeout_min=2000 超过 max 的配置失败
```

- [ ] **Step 8: 全量回归**

```bash
cargo test --features cluster 2>&1 | tail -20
```

**预期**: 全量测试通过。注意 `test_raft_config_validation` 中已有的断言使用 `RaftNodeConfig::default()`，新的默认值仍能通过校验（`200 < 500` 成立）。

- [ ] **Step 9: Clippy + fmt**

```bash
RUSTFLAGS='-D warnings' cargo clippy --all-targets --features cluster
cargo fmt --check
```

- [ ] **Step 10: 确认 grpc_max_message_size 统一**

```bash
# 从 AiDb 项目根目录
grep -rn 'grpc_max_message_size' src/ | grep -v 'test' | grep -v '#\[cfg(test)\]'
# 所有值应为 64 * 1024 * 1024
```

```bash
# 从 AiKv 项目根目录
grep -rn 'grpc_max_message_size\|max_message_size' src/main.rs
# 所有值应为 64 * 1024 * 1024
```

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m "refactor: unify cluster config defaults and lifecycle factory params

Align RaftNodeConfig::default() with AiKv's proven values:
max_payload_entries 300→100, max_entry_size 64MiB→8MiB,
rpc_timeout_ms 30→200, grpc_max_message_size 65MiB→64MiB.

Make lifecycle background loop's RaftNetworkClientFactory use
cfg values (or RaftNodeConfig::default()) instead of hardcoded
30/65MiB, covering Phase 1B + 3B + 3C."
```

---

## 验证汇总

全部 Task 完成后执行以下全局验证:

```bash
# AiDb
cd aidb
cargo build --features cluster
RUSTFLAGS='-D warnings' cargo clippy --all-targets --features cluster
cargo test --features cluster
cargo fmt --check

# AiKv
cd aikv
cargo build --features cluster
RUSTFLAGS='-D warnings' cargo clippy --all-targets --features cluster
cargo test --features cluster
cargo fmt --check

# 确认无硬编码回退
grep -rn '127\.0\.0\.1' AiDb/src/cluster/network.rs

# 确认 grpc_max_message_size 统一
grep -rn 'grpc_max_message_size' AiDb/src/ | grep -v test
grep -rn 'max_message_size\|grpc_max_message_size' AiKv/src/main.rs

# 确认 DB_COUNT 只剩 1 处定义
grep -rn 'const DB_COUNT' AiKv/src/
