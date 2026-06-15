# 硬编码值治理设计方案

- 日期: 2026-06-02
- 状态: 待审核
- 涉及仓库: AiDb, AiKv

## 概述

对两个仓库中扫描出的不合理硬编码值进行系统性修复。分为 3 个 Phase 递进执行:

1. **安全修复** — 修复生产路径上的静默容错回退和参数不一致
2. **配置化扩展** — 将引擎运行时参数和集群调优参数从编译期常量提升为可配置字段
3. **代码清理与一致性统一** — DRY 修复、默认值对齐

`rpc_port + 10000` 数据面端口偏移由于改动影响面大，已记录至后续 TODO，本轮不处理。

---

## Phase 1: 安全修复

### 1A: 删除 `127.0.0.1` 生产回退

**文件**: `AiDb/src/cluster/network.rs:357`

**现状**:

```rust
.unwrap_or_else(|| format!("http://127.0.0.1:{}", 50_000 + target))
```

当 openraft 传递的节点地址为空且该节点未在 `RaftNetworkClientFactory` 内部注册时，静默回退到 `127.0.0.1:50000+N`。在生产集群中，这会导致跨节点 RPC 连接到错误的地址，且日志中只会看到 `Unreachable` 超时而无任何地址配置异常的线索。

**改动**:

- 将 `unwrap_or_else` 替换为 `tracing::error!` + 空字符串

```rust
} else {
    tracing::error!(
        target_node_id = target,
        "gRPC address for node {} is not registered in RaftNetworkClientFactory; \
         this indicates a cluster configuration bug",
        target,
    );
    String::new()
}
```

- 空 `target_addr` 导致 `RaftServiceClient::connect("")` 失败 → `NetworkError` → openraft 以 `Unreachable` 处理并重试
- 日志从无声失败变为显式 ERROR

**验证**:

- 编译通过，无新 clippy 警告
- 确认生产中该路径仅在 `node.addr` 为空时进入（正常集群不会，属于防御性编程）

### 1B: 统一 `grpc_max_message_size`

**问题**: 客户端和服务端的 `grpc_max_message_size` 值不一致:
- `RaftNodeConfig::default()`: `64 MiB + 1 MiB = 65 MiB`
- Lifecycle client factory (`multi_raft_node.rs:161`): `65 MiB`（硬编码）
- MetaRaft gRPC server (AiKv `main.rs:201`): `64 MiB`
- MultiRaft gRPC server (AiKv `main.rs:266`): `64 MiB`
- Raft client factory (AiKv `main.rs:146`): `64 MiB`

Lifecycle client 可能发送 `65 MiB` 的消息但服务器只接受 `64 MiB`，导致边界消息被静默拒绝。

**改动**:

1. **`src/cluster/types.rs`** — `RaftNodeConfig::default()`:
   ```rust
   // 当前: 64 * 1024 * 1024 + 1024 * 1024  (= 65 MiB)
   grpc_max_message_size: 64 * 1024 * 1024,  // 统一改为 64 MiB
   ```

2. **`src/cluster/multi_raft_node.rs:157-162`** — lifecycle factory 从 cfg 取值:
   ```rust
   let msg_size = cfg.as_ref().map_or(
       RaftNodeConfig::default().grpc_max_message_size,
       |c| c.raft_node_config.grpc_max_message_size,
   );
   let net_factory = Arc::new(RwLock::new(RaftNetworkClientFactory::new(
       node_id, 0, 30, msg_size,
   )));
   ```

3. **AiKv `src/main.rs:201, 266`** — 继续使用 `64 * 1024 * 1024`（已一致）

**风险**: 现有集群生产流量条目可能超过 `64 MiB`。但 AiKv 的 `max_entry_size` 从始至终是 `8 MiB`，单条 Raft 日志不会超过 `8 MiB`，所以 `64 MiB` 是安全的。

---

## Phase 2: 配置化扩展

### 2A: 引擎运行时参数 → Options

**文件**: `AiDb/src/config.rs` + `src/engine/db/inner.rs`

将 `inner.rs` 中 9 个编译期常量迁移到 `Options` 结构体，使其可通过 API 配置。

**新增字段**（插入 `Options` 现有 `background_compaction` 字段之后）:

```rust
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

**Default 值**:

```rust
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

**`inner.rs` 对应改动**:

| 位置 | 原代码 | 替换为 |
|------|--------|--------|
| 行 32 | `const FLUSH_POLL_MS: u64 = 500` | 删除 |
| 行 33 | `const COMPACTION_POLL_MS: u64 = 500` | 删除 |
| 行 183 | `Duration::from_millis(FLUSH_POLL_MS)` | `Duration::from_millis(self.options.flush_poll_ms)` |
| 行 345 | `Duration::from_millis(10)` | `Duration::from_millis(self.options.write_stall_poll_ms)` |
| 行 357 | `excess / cap * 100.0` | `excess / cap * self.options.write_stall_slowdown_max_ms as f64` |
| 行 838 | `.min(4)` | `.min(self.options.max_sub_compactions as u64)` |
| 行 839 | `n.max(2)` | `n.max(self.options.min_sub_compactions as u64)` |
| 行 968 | `const MAX_WAIT_ITERS: usize = 10_000` | `self.options.memtable_wait_iters` |
| 行 977 | `Duration::from_millis(1)` | `Duration::from_millis(self.options.memtable_wait_interval_ms)` |
| 行 1181,1185 | `COMPACTION_POLL_MS` | `self.options.compaction_poll_ms` |
| 行 119 | `bounded(64)` | `bounded(options.compaction_channel_size)` |

**`validate()` 新增**:

```rust
if self.flush_poll_ms == 0 {
    return Err(InvalidArgument("flush_poll_ms must be > 0".into()));
}
if self.compaction_poll_ms == 0 {
    return Err(InvalidArgument("compaction_poll_ms must be > 0".into()));
}
if self.min_sub_compactions > self.max_sub_compactions {
    return Err(InvalidArgument(
        "min_sub_compactions must be <= max_sub_compactions".into(),
    ));
}
```

**`for_testing()` 必须同步添加 9 个新字段**（否则 Rust 编译失败 — `Self { .. }` 语法要求列出全字段）:

```rust
// 在 Options::for_testing() 中追加:
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

同理 `for_high_write_throughput()` 和 `for_high_read_throughput()` 也需追加（使用默认值）:

```rust
..Self::default()  // 已包含新字段的默认值，无需显式列出
```

由于这两个方法使用 `..Self::default()` 结构体更新语法，新字段会自动继承默认值，无需显式追加。

### 2B: AiKv CLI 新增调优参数

**文件**: `AiKv/src/main.rs` + `src/cluster/config_auto_save.rs`

**Args 新增**:

```rust
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

**`init_cluster()` 对应改动**:

- **Lifecycle tick**: `LifecycleManager::new(...).with_tick_interval(Duration::from_millis(args.lifecycle_tick_ms))`（已有 setter）
- **Gossip 间隔**: `start_background_refresh(gossip, state, args.gossip_interval)`（已有参数）
- **Config auto-save**: 见下方 `ConfigAutoSave` 改造

**`ConfigAutoSave` 新增构造器**:

```rust
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

---

## Phase 3: 代码清理与一致性统一

### 3A: DB_COUNT 提取到单一定义

**现状**: `const DB_COUNT: usize = 16` 在 3 个文件中各自定义（违反 DRY）:

| 文件 | 行 | 代码 |
|------|-----|------|
| `src/storage/adapter.rs` | 17 | `const DB_COUNT: usize = 16` |
| `src/command/database.rs` | 12 | `const DB_COUNT: usize = 16` |
| `src/command/server.rs` | 15 | `const DB_COUNT: usize = 16` |

**改动**: 在 `src/storage/types.rs`（已有 `TTL_NO_EXPIRY`、`WRONGTYPE` 等公共常量）中新增定义:

```rust
/// AiKv 支持的数据库数量 (与 Redis 默认 16 个 DB 一致)
pub const DB_COUNT: usize = 16;
```

其余三处删除各自的 `const DB_COUNT` 定义，改为 `use crate::storage::types::DB_COUNT`.

### 3B: RaftNodeConfig 默认值对齐

**文件**: `AiDb/src/cluster/types.rs`

**改动**: 将 `RaftNodeConfig::default()` 中与 AiKv `main.rs` 实际值不一致的参数对齐:

| 参数 | 原默认值 | 改为 | 理由 |
|------|---------|------|------|
| `max_payload_entries` | `300` | `100` | AiKv 已验证 100 为合理值，300 在慢网络下批处理延迟高 |
| `max_entry_size` | `64 MiB` | `8 MiB` | 与 AiKv 一致；单条日志 64 MiB 不切实际 |
| `rpc_timeout_ms` | `30` | `200` | 30ms 在跨 AZ/跨机房场景下极容易超时导致选主抖动 |
| `grpc_max_message_size` | `65 MiB` | `64 MiB` | **(Phase 1B 已包含，此处列齐供参考)** |

这些值仅为 `Default` trait 实现；AiKv 自行构造 `RaftNodeConfig`（main.rs:150-161）使用自己的值，不受 Default 变更影响。

### 3C: Lifecycle factory 超时使用 config 默认值

**文件**: `AiDb/src/cluster/multi_raft_node.rs:157-162`

**现状**: lifecycle 后台循环中创建 `RaftNetworkClientFactory` 时硬编码 `rpc_timeout_ms=30`:

```rust
let net_factory = Arc::new(RwLock::new(RaftNetworkClientFactory::new(
    node_id, 0, 30, /* max_message_size */,
)));
```

**改为**: 和 max_message_size（Phase 1B）一样从 `cfg` 或 `RaftNodeConfig::default()` 取值:

```rust
let rpc_timeout = cfg.as_ref().map_or(
    RaftNodeConfig::default().rpc_timeout_ms,
    |c| c.raft_node_config.rpc_timeout_ms,
);
```

---

## 验证清单

### 编译与测试

每个 Phase 完成后执行:

- [ ] `cargo build --features cluster` 编译通过（AiDb + AiKv）
- [ ] `RUSTFLAGS='-D warnings' cargo clippy --all-targets --features cluster` 无新警告
- [ ] `cargo test --features cluster` 全部通过（AiDb + AiKv）
- [ ] `cargo fmt --check` 通过

### 代码确认

- [ ] `grep -rn '127\.0\.0\.1' src/cluster/network.rs` 确认生产回退已删除
- [ ] `grep -rn '1024 \* 1024' src/` 确认 `grpc_max_message_size` 统一
- [ ] `grep -rn 'const DB_COUNT' src/` 确认只剩 1 处定义

### 新增测试（必须）

Phase 完成后对应补充:

| # | 改动 | 建议测试 | 优先级 |
|---|------|---------|--------|
| 1 | 1A | 单元测试: `new_client()` 在 `node.addr` 为空时返回空 string 并记录 ERROR | 高 |
| 2 | 1B | 单元测试: `RaftNodeConfig::default().grpc_max_message_size == 64 * 1024 * 1024` | 中 |
| 3 | 2A | 单元测试: 9 个新字段的默认值验证 | 高 |
| 4 | 2A | 单元测试: `validate()` 新增的 3 条规则（`flush_poll_ms>0`、`compaction_poll_ms>0`、`min<=max`） | 高 |
| 5 | 2A | 单元测试: `for_testing()` 包含新字段（编译即验证，但仍需断言值） | 中 |
| 6 | 2B | 单元测试: `ConfigAutoSave::new_with_interval()` 正确设置间隔 | 高 |
| 7 | 2B | 集成测试: CLI 参数解析 `--lifecycle-tick-ms`、`--gossip-interval`、`--config-auto-save-ms` | 中 |
| 8 | 3A | 单元测试: `DB_COUNT == 16` 且仅 1 处定义（编译期验证，无需额外测试） | 中 |
| 9 | 3B | 单元测试: `RaftNodeConfig::default()` 各字段值符合预期 | 高 |
| 10 | 3C | 单元测试: lifecycle factory 从 config 取值正确 | 中 |

### 文档同步

- [ ] `AiKv/DEPLOYMENT.md` 或相关文档中补充 `--lifecycle-tick-ms`、`--gossip-interval`、`--config-auto-save-ms` 的说明

### 提交粒度

建议每个子项（1A、1B、2A、2B、3A、3B、3C）独立 commit，便于回滚和 Code Review。

---

## 性能影响评估

Phase 2A 将 9 个编译期常量变为运行时 struct 字段访问:
- Rust 编译器对 `const` 常量会做常量折叠和内联，而对 struct 字段是间接加载（`self.options.flush_poll_ms` 通过 Arc 指针 + 字段偏移）
- 但这些字段用于 **毫秒级** 的 sleep 参数（500ms、10ms、1ms），每次读取的 CPU 开销（1-2 条 mov 指令）相对于实际 sleep 时间可忽略不计
- **结论**: 无实际性能影响

## 回滚策略

- 1A/1B/3A/3B/3C: 单文件改动，`git revert <commit>` 即可
- 2A: `Options` 新增字段不破坏 API 兼容性（新增字段有默认值），回滚只需 revert 提交；`for_testing()` 需注意如果回滚不及时可能导致编译失败
- 2B: 新增 CLI 参数有默认值，不改变现有行为；回滚安全

## Notes

### `rpc_port + 10000` 偏移（本轮不处理）

- **影响范围**: AiDb `CLAUDE.md` 已知限制，AiKv `main.rs:210-222`、`src/cluster/commands.rs:181-182, 1195`
- **问题**: 数据面端口固定偏移 `rpc_port + 10000`，要求 `rpc_port ≤ 55535`
- **目标**: 改为可配置参数（CLI arg 或集群配置），同时需考虑滚动升级兼容性
- **已记录**: 后续 TODO
