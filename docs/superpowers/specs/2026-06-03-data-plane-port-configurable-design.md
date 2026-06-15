# 数据面端口偏移可配置化设计方案

- 日期: 2026-06-03
- 状态: 待审核
- 涉及仓库: AiKv
- 关联文档: [2026-06-02-hardcoded-values-remediation-design.md](2026-06-02-hardcoded-values-remediation-design.md)

## 概述

AiKv 采用三端口架构:

| 端口 | 用途 | 关系 |
|------|------|------|
| **client_port** | Redis 客户端连接 (RESP/TCP) | `--bind` 参数 |
| **rpc_port** | MetaRaft 控制面 gRPC | `--cluster-rpc-addr` 参数 |
| **data_port** | MultiRaft 数据面 gRPC | `rpc_port + 10000` (硬编码) |

当前 `data_port = rpc_port + 10000` 是编译期硬编码，要求 `rpc_port ≤ 55535`。
本设计将此偏移改为 CLI 参数 `--cluster-data-port-offset` 配置。

### 三端口关系图

```
Redis client  <--RESP/TCP-->  client_port
                                   |
                              AiKv 进程
                            /              \
                    rpc_port (控制面)   data_port (数据面)
                        |                    |
                   MetaRaft gRPC       MultiRaft gRPC
                   (成员/配置管理)      (数据复制)
```

**注意:** `CLUSTER MEET` 命令中 `client_port → rpc_port` 的默认推导 (`+10000`) 是 Redis Docker pipeline 布局约定，与本设计的数据面端口偏移是不同层的概念。本设计**不**修改 `CLUSTER MEET` 默认推导。

## 影响范围

### 硬编码位置

| # | 文件 | 行 | 用途 | 是否修改 |
|---|------|----|------|---------|
| 1 | `src/main.rs` | 228-242 | 启动时 `data_port = rpc_port + 10000` + 端口溢出校验 | **修改** |
| 2 | `src/cluster/commands.rs` | 176-184 | `CLUSTER NODES` 输出中的 `@cport` 显示字段 | **修改** |
| 3 | `src/cluster/commands.rs` | 1191-1195 | `CLUSTER MEET` 中 `rpc_port` 默认推导 | **不修改** (见下方说明) |

### CLUSTER MEET 不修改的原因

`CLUSTER MEET` 的参数是 `host client_port [rpc_port]`，省略 `rpc_port` 时的默认推导 `client_port + 10000` 解决的是 **client_port 到 rpc_port 的映射**（Docker pipeline 布局约定），与 **rpc_port 到 data_port 的偏移**是不同层的概念。两者不应绑定到同一个配置参数。

如果需要，`CLUSTER MEET` 可显式传入 `rpc_port` 参数，无需依赖默认推导。

## 设计方案

### 总体思路

通过 `--cluster-data-port-offset` CLI 参数将偏移从编译期常量提升为运行时配置，通过 `ClusterStateManager` 全局单例传播到命令处理器。默认值 `10000` 保证向后兼容。

```mermaid
flowchart LR
    Args["--cluster-data-port-offset"]
    Args -->|init_cluster()| main["data_port 计算 + 溢出校验"]
    Args -->|ClusterStateManager| state["全局状态"]
    state -->|CLUSTER NODES| nodes["@cport 显示"]
```

### 改动细节

#### 0. 常量定义

```rust
/// 数据面端口偏移默认值 (与 Redis Cluster @cport 约定一致).
pub(crate) const DEFAULT_DATA_PORT_OFFSET: u16 = 10000;
```

放置位置: `AiKv/src/cluster/mod.rs` 或 `AiKv/src/cluster/state.rs`（一处定义，避免 DRY 缺口）。

#### 1. `AiKv/src/main.rs` — CLI 参数 + 启动逻辑

**Args 结构体新增** (Phase 2 参数之后, 第 94 行附近):

```rust
/// 数据面总线端口偏移 (默认 10000, 与 Redis Cluster @cport 约定一致).
/// data_port = rpc_port + 此偏移值, 数据面 gRPC 端口 = RPC 端口 + 偏移.
#[cfg(feature = "cluster")]
#[arg(long, default_value_t = DEFAULT_DATA_PORT_OFFSET)]
cluster_data_port_offset: u16,
```

**`init_cluster()` 新增参数** (签名扩展):

```rust
#[cfg(feature = "cluster")]
async fn init_cluster(
    node_id: u64,
    rpc_addr: String,
    peers: &[String],
    d: aidb::DB,
    bind_addr: SocketAddr,
    raft_election_timeout_min: u64,
    raft_election_timeout_max: u64,
    lifecycle_tick_ms: u64,
    gossip_interval: u64,
    config_auto_save_ms: u64,
    cluster_data_port_offset: u16,   // <-- 新增
) -> Result<(), Box<dyn std::error::Error>> {
```

**端口计算** (原 228-242 行):

```rust
let rpc_port_u16: u16 = rpc_socket.port();
if rpc_port_u16 > 65535u16 - cluster_data_port_offset {
    return Err(format!(
        "RPC port {} is too large: rpc_port + {} = {} exceeds u16::MAX (65535). \
         Use a port <= {}.",
        rpc_port_u16,
        cluster_data_port_offset,
        rpc_port_u16 as u32 + cluster_data_port_offset as u32,
        65535u16 - cluster_data_port_offset,
    ).into());
}
let data_port = rpc_port_u16 + cluster_data_port_offset;
```

**调用处传参** (main() 中):

```rust
if let Err(e) = init_cluster(
    node_id,
    rpc_addr,
    &args.cluster_peers,
    d,
    args.bind,
    args.raft_election_timeout_min,
    args.raft_election_timeout_max,
    args.lifecycle_tick_ms,
    args.gossip_interval,
    args.config_auto_save_ms,
    args.cluster_data_port_offset,  // <-- 新增
)
```

**传播到 ClusterStateManager** (第 310 行附近):

```rust
state_mgr.data_port_offset = args.cluster_data_port_offset;
```

#### 2. `AiKv/src/cluster/state.rs` — 新增字段 + 常量

```rust
/// 数据面端口偏移默认值 (与 Redis Cluster @cport 约定一致).
pub const DEFAULT_DATA_PORT_OFFSET: u16 = 10000;
```

**ClusterStateManager 新增字段**:

```rust
/// 数据面总线端口偏移.
pub data_port_offset: u16,
```

**new() 中初始化**:

```rust
data_port_offset: DEFAULT_DATA_PORT_OFFSET,
```

#### 3. `AiKv/src/cluster/commands.rs` — CLUSTER NODES 使用偏移

```rust
let offset = CLUSTER_STATE_MGR.get().map_or(DEFAULT_DATA_PORT_OFFSET, |m| m.data_port_offset);
let cport = match mgr.router.get_node_addr(*nid) {
    Some(ref addr) => addr
        .rsplit(':')
        .next()
        .and_then(|port| port.parse::<u16>().ok())
        .map(|p| p + offset)
        .unwrap_or(0),
    None => 0,
};
```

#### 4. `CLUSTER MEET` — 不修改

原 `client_port + 10000` 保持不变。这是 `client_port → rpc_port` 的 Docker pipeline 约定，与 data plane offset 无关。

### 边界约束

| 场景 | 行为 | 说明 |
|------|------|------|
| `offset = 0` | `data_port = rpc_port` | 两个 gRPC server 争用同一端口，第二个 bind 会失败并报错。合法但实际不可用，应在文档中标注为不推荐。 |
| `offset = 65535` | `data_port = rpc_port + 65535` | 仅当 `rpc_port = 0` 时不溢出（保留端口，实际不可用）。 |
| `offset` 过大导致溢出 | 启动时报错 | 校验 `rpc_port > 65535 - offset` 时拒绝启动。 |
| 在线修改 offset | 不生效 | offset 仅启动时从 CLI 读取，不持久化，不动态重载。须重启进程。 |

### 文件变更汇总

| 文件 | 改动类型 | 说明 |
|------|---------|------|
| `AiKv/src/main.rs` | 新增参数 + `init_cluster` 参数 + 端口计算 + 校验逻辑 + ClusterStateManager 传播 | ~15 行 |
| `AiKv/src/cluster/state.rs` | 新增常量 `DEFAULT_DATA_PORT_OFFSET` + `data_port_offset` 字段 + new() 初始化 | ~5 行 |
| `AiKv/src/cluster/commands.rs` | CLUSTER NODES cport 使用偏移（MEET 不改） | ~3 行 |
| `AiKv/src/cluster/mod.rs` | 可选: 导出常量 | ~1 行 |

### 兼容性与滚动升级

| 场景 | 行为 |
|------|------|
| 原地升级, 不传参 | 默认 `10000`, 行为完全不变 |
| 原地升级, 传 `--cluster-data-port-offset 10000` | 同上 |
| 变更偏移 | **所有节点必须使用相同偏移值**, 且不能在线修改。推荐计划窗口内全集群逐台重启。 |
| 混合版本运行 | 不推荐 — 新旧节点偏移不一致导致数据面端口不可达, `CLUSTER NODES` 显示错误的 `@cport` |

### 离线状态持久化

`--cluster-data-port-offset` 是纯 CLI 参数，**不**写入 `nodes.conf`。重启必须携带相同参数。部署脚本（`entrypoint.sh`、docker compose 等）需确保一致性。

### CLUSTER SLOTS / CLUSTER SHARDS 预存问题

`CLUSTER SLOTS` 和 `CLUSTER SHARDS` 使用 `resolve_endpoint()` 从 `router.get_node_addr()` 提取端口，返回的是 RPC 端口而非 client 端口。这在 Redis Cluster 兼容性上是一个预存问题（`CLUSTER SLOTS` 应返回客户端可达的端口）。

与本设计无关，但在此记录避免混淆: `CLUSTER SLOTS`/`SHARDS` 返回的端口不受 `--cluster-data-port-offset` 影响。

### 文档同步清单

- [ ] `AiKv/CLAUDE.md` — 更新数据面端口已知限制描述
- [ ] `AiDb/CLAUDE.md` — 同上
- [ ] `AiKv/DEPLOYMENT.md` — 新增 `--cluster-data-port-offset` 参数说明
- [ ] `AiKv/e2e/utils.sh` — 注释中端口约定更新
- [ ] 更新待办列表 — 移除已完成项

### 性能影响

零运行时开销 — 偏移量仅在进程启动时解析一次。

### 回滚策略

`git revert <commit>` — 新增 CLI 参数有默认值，不影响现有部署。

## 自动化测试计划

### 单元测试

| # | 测试项 | 文件 | 优先级 |
|---|--------|------|--------|
| 1 | `DEFAULT_DATA_PORT_OFFSET == 10000` | `state.rs` | 高 |
| 2 | `ClusterStateManager` 构造后 `data_port_offset == 10000` | `state.rs` | 高 |
| 3 | 端口溢出: offset=10000, rpc=55535 → OK | `main.rs` 或独立测试 | 高 |
| 4 | 端口溢出: offset=10000, rpc=55536 → Err | 同上 | 高 |
| 5 | 端口溢出: offset=5000, rpc=60535 → OK | 同上 | 高 |
| 6 | 端口溢出: offset=5000, rpc=60536 → Err | 同上 | 高 |

### 集成测试

| # | 测试项 | 文件 | 优先级 |
|---|--------|------|--------|
| 7 | CLUSTER NODES `@cport`: `data_port_offset=5000`, addr=`127.0.0.1:7001` → `@cport=12001` | `tests/modules/cluster/commands.rs` | 高 |
| 8 | CLUSTER NODES `@cport`: `data_port_offset=10000` (默认) → `@cport=17001` | 同上 | 中 |
| 9 | `create_cluster_mgr()` 添加 `data_port_offset` 字段（编译即验证） | `tests/cluster_integration.rs` | 高 |

### E2E 测试

| # | 测试项 | 优先级 |
|---|--------|--------|
| 10 | 新脚本: 2 节点 `--cluster-data-port-offset 5000`, 验证 CLUSTER NODES 的 `@cport` | 中 |
| 11 | 回归: 现有 E2E 全部通过 (默认 10000 兼容) | 高 |

### 验证清单 (手动)

- [ ] `cargo build --features cluster` 编译通过
- [ ] `cargo clippy --all-targets --features cluster` 无新警告
- [ ] `cargo test --features cluster` 全部通过
- [ ] `--cluster-data-port-offset 5000` + rpc_port=60535 启动正常
- [ ] `--cluster-data-port-offset 5000` + rpc_port=60536 报错
- [ ] `CLUSTER NODES` 输出 `@cport` 使用新偏移
- [ ] `CLUSTER MEET` 省略第 3 参数时仍然使用 `+10000` (行为不变)
- [ ] 默认 `10000` 旧配置完全兼容
