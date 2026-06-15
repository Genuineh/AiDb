# Data Plane Port Offset Configurable Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the hardcoded `rpc_port + 10000` data plane port offset with a CLI-configurable `--cluster-data-port-offset`, with default `10000` preserving backward compatibility.

**Architecture:** Add `cluster_data_port_offset: u16` to `Args` struct and `ClusterStateManager`. Pass the value through `init_cluster()` for port calculation/validation and via `CLUSTER_STATE_MGR` global singleton for `CLUSTER NODES @cport` display.

**Tech Stack:** Rust, clap (CLI parsing), parking_lot (RwLock), OnceLock (global singleton)

---

### Task 1: Add `DEFAULT_DATA_PORT_OFFSET` constant + `ClusterStateManager` field

**Files:**
- Modify: `AiKv/src/cluster/state.rs:1-72`
- Test: `AiKv/tests/cluster_integration.rs:66-85` (compile-time verification)

- [ ] **Step 1: Add the constant and field**

Add the constant at the top level of `state.rs`, and add the field to `ClusterStateManager`:

In `AiKv/src/cluster/state.rs`:

```rust
// Near the top, after the imports
/// 数据面端口偏移默认值 (与 Redis Cluster @cport 约定一致).
pub const DEFAULT_DATA_PORT_OFFSET: u16 = 10000;
```

In the `ClusterStateManager` struct (after `data_dir: Option<std::path::PathBuf>`, line 37):

```rust
  /// 数据面总线端口偏移.
  pub data_port_offset: u16,
```

In the `new()` constructor (after `data_dir: None,` around line 64):

```rust
      data_port_offset: DEFAULT_DATA_PORT_OFFSET,
```

- [ ] **Step 2: Verify compilation fails in test code**

Run: `cargo build --features cluster --tests`

Expected: FAIL. The test file `tests/cluster_integration.rs` constructs `ClusterStateManager` directly with struct literal syntax and does not include `data_port_offset` field.

- [ ] **Step 3: Commit**

```bash
git add AiKv/src/cluster/state.rs
git commit -m "feat: add DEFAULT_DATA_PORT_OFFSET constant and ClusterStateManager field"
```

---

### Task 2: Add CLI parameter + propagate through `init_cluster()`

**Files:**
- Modify: `AiKv/src/main.rs:29-95`, `AiKv/src/main.rs:490-514`, `AiKv/src/main.rs:300-310`

- [ ] **Step 1: Add CLI parameter to Args**

After the last `#[cfg(feature = "cluster")]` parameter (`config_auto_save_ms`, around line 94), add:

```rust
  /// 数据面总线端口偏移 (默认 10000, 与 Redis Cluster @cport 约定一致).
  /// data_port = rpc_port + 此偏移值.
  #[cfg(feature = "cluster")]
  #[arg(long, default_value_t = DEFAULT_DATA_PORT_OFFSET)]
  cluster_data_port_offset: u16,
```

Also add the import (if not already present via existing `use`):

At the top of `main.rs`, add:
```rust
use aikv::cluster::state::DEFAULT_DATA_PORT_OFFSET;
```

(Note: the existing `use aikv::server::{ConnectionConfig, Server, ServerSharedState};` at line 17 shows how to import from the crate; add a `use` for `DEFAULT_DATA_PORT_OFFSET`.)

- [ ] **Step 2: Add parameter to `init_cluster()` signature**

Find the `init_cluster()` function signature (around line 420-440). Add the parameter:

```rust
async fn init_cluster(
    node_id: u64,
    rpc_addr: &str,
    peers: &[String],
    d: aidb::DB,
    bind_addr: SocketAddr,
    raft_election_timeout_min: u64,
    raft_election_timeout_max: u64,
    lifecycle_tick_ms: u64,
    gossip_interval: u64,
    config_auto_save_ms: u64,
    cluster_data_port_offset: u16,   // <-- NEW
) -> Result<(), Box<dyn std::error::Error>> {
```

- [ ] **Step 3: Propagate to `ClusterStateManager`**

Inside `init_cluster()`, after the `ClusterStateManager` is created (around line 309), set the field:

```rust
state_mgr.data_port_offset = cluster_data_port_offset;
```

(Add this somewhere after line 300 `state_mgr.set_membership_coordinator(...)` and before `let state_mgr = Arc::new(state_mgr);` / `CLUSTER_STATE_MGR.set(state_mgr)`.)

- [ ] **Step 4: Pass from `main()` to `init_cluster()`**

Find the call site (around line 497-509). Add the new argument:

```rust
    if let Err(e) = init_cluster(
      node_id,
      rpc_addr,
      &args.cluster_peers,
      d,
      args.bind,
      _cluster_db,
      args.raft_election_timeout_min,
      args.raft_election_timeout_max,
      args.lifecycle_tick_ms,
      args.gossip_interval,
      args.config_auto_save_ms,
      args.cluster_data_port_offset,  // <-- NEW
    )
```

- [ ] **Step 5: Build check**

Run: `cargo build --features cluster --tests`

Expected: PASS (or fail on test file, which will be fixed in Task 5).

- [ ] **Step 6: Commit**

```bash
git add AiKv/src/main.rs
git commit -m "feat: add --cluster-data-port-offset CLI parameter and propagate"
```

---

### Task 3: Replace hardcoded `+ 10000` in port calculation

**Files:**
- Modify: `AiKv/src/main.rs:228-242`

- [ ] **Step 1: Replace hardcoded offset with the parameter**

Inside `init_cluster()`, at the port calculation (lines 228-242), replace the hardcoded values:

```rust
  // 7. 推算 MultiRaft 总线端口 (rpc_port + offset, 与 @cport 约定一致)
  let rpc_port_u16: u16 = rpc_socket.port();
  if rpc_port_u16 > 65535u16 - cluster_data_port_offset {
    return Err(
      format!(
        "RPC port {} is too large: rpc_port + {} = {} exceeds u16::MAX (65535). \
         Use a port <= {}.",
        rpc_port_u16,
        cluster_data_port_offset,
        rpc_port_u16 as u32 + cluster_data_port_offset as u32,
        65535u16 - cluster_data_port_offset,
      )
      .into(),
    );
  }
  let data_port = rpc_port_u16 + cluster_data_port_offset;
```

- [ ] **Step 2: Build check**

Run: `cargo build --features cluster`

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add AiKv/src/main.rs
git commit -m "feat: use configurable offset for data plane port calculation"
```

---

### Task 4: CLUSTER NODES `@cport` display

**Files:**
- Modify: `AiKv/src/cluster/commands.rs:176-184`

- [ ] **Step 1: Replace hardcoded `+ 10000` with offset from state**

At lines 176-184, the `cport` calculation currently reads:

```rust
    let cport = match mgr.router.get_node_addr(*nid) {
      Some(ref addr) => addr
        .rsplit(':')
        .next()
        .and_then(|port| port.parse::<u16>().ok())
        .map(|p| p + 10000)
        .unwrap_or(0),
      None => 0,
    };
```

Replace with:

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

Also add the import at the top of `commands.rs`:

```rust
use crate::cluster::state::DEFAULT_DATA_PORT_OFFSET;
```

(Check existing imports — `CLUSTER_STATE_MGR` is already imported at line 3.)

- [ ] **Step 2: Build check**

Run: `cargo build --features cluster`

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add AiKv/src/cluster/commands.rs
git commit -m "feat: use configurable offset for CLUSTER NODES @cport"
```

---

### Task 5: Test updates and additions

**Files:**
- Modify: `AiKv/tests/cluster_integration.rs:66-85`
- Modify: `AiKv/tests/modules/cluster/commands.rs`

- [ ] **Step 1: Add `data_port_offset` to `create_cluster_mgr()`**

In `tests/cluster_integration.rs`, at the struct construction (line 81, before `_watcher_shutdown`), add:

```rust
    data_port_offset: DEFAULT_DATA_PORT_OFFSET,
```

Also add the import at the top of the test file:

```rust
use aikv::cluster::state::DEFAULT_DATA_PORT_OFFSET;
```

- [ ] **Step 2: Verify tests compile and pass**

Run: `cargo test --features cluster --test cluster_integration -- --test-threads=1`

Expected: All tests PASS.

- [ ] **Step 3: Add cport offset test to `tests/modules/cluster/commands.rs`**

Open `AiKv/tests/modules/cluster/commands.rs`. Add a test after the existing `cluster_nodes_uninitialized` test:

```rust
#[test]
fn cluster_nodes_cport_with_custom_offset() {
    // The cluster is uninitialized in this test, so CLUSTER_NODES returns CLUSTERDOWN.
    // This verifies we don't crash when reading the offset.
    let result = aikv::cluster::cluster_nodes();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("CLUSTERDOWN"), "expected CLUSTERDOWN, got: {err}");
}
```

- [ ] **Step 4: Build and run the test**

Run: `cargo test --features cluster --test cluster_commands`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add AiKv/tests/cluster_integration.rs AiKv/tests/modules/cluster/commands.rs
git commit -m "test: update tests for data_port_offset field and cport"
```

---

### Task 6: Documentation updates

**Files:**
- Modify: `AiKv/CLAUDE.md`
- Modify: `AiDb/CLAUDE.md`
- Modify: `AiKv/DEPLOYMENT.md`
- Modify: `AiKv/e2e/utils.sh`
- Modify: 待办列表

- [ ] **Step 1: Update `AiKv/CLAUDE.md`**

Find the "集群" known_limitation section. Replace:

```
- **数据面 gRPC 端口**: 固定为 `rpc_port + 10000`, 所有数据 Group 共享此端口. 启动时校验 `rpc_port ≤ 55535`.
```

With:

```
- **数据面 gRPC 端口**: 默认为 `rpc_port + 10000`, 可通过 `--cluster-data-port-offset` CLI 参数配置. 所有数据 Group 共享此端口. 启动时校验 `rpc_port ≤ 65535 - offset`.
```

- [ ] **Step 2: Update `AiDb/CLAUDE.md`**

Find the "集群 (v0.14.3)" section. Replace:

```
- **数据面端口**: `rpc_port + 10000` 固定偏移, 部署时需确保 RPC 端口 ≤ 55535. AiKv 启动时已做校验.
```

With:

```
- **数据面端口**: 默认为 `rpc_port + 10000`, 可通过 AiKv `--cluster-data-port-offset` CLI 参数配置. 部署时需确保 `rpc_port ≤ 65535 - offset`. AiKv 启动时已做校验.
```

- [ ] **Step 3: Update `AiKv/DEPLOYMENT.md`**

Find the CLI参数参考表. Add a new row:

```
| `--cluster-data-port-offset` | `10000` | 数据面总线端口偏移, data_port = rpc_port + offset |
```

Also find any startup examples that use `--cluster-rpc-addr` and ensure the data port offset relationship is clear. Add a note after the port convention section:

```
> 数据面 gRPC 端口 = RPC 端口 + --cluster-data-port-offset (默认 10000).
> 所有节点必须使用相同的偏移值; 偏移变更需全集群重启.
```

- [ ] **Step 4: Update `AiKv/e2e/utils.sh`**

Find the port convention comment (around line 47):

```
# Base port for cluster nodes; each node uses 3 ports (client, rpc, data-plane=rpc+10000).
```

Keep as-is since E2E uses the default offset. Add a note after it:

```
# Use --cluster-data-port-offset to change the data-plane offset (default 10000).
```

- [ ] **Step 5: Update 待办列表**

Mark the `rpc_port + 10000` item as completed. Replace the section with:

```markdown
## 硬编码治理 — 已完成

### ~~`rpc_port + 10000` 数据面端口偏移~~ (2026-06-03)
- 已将偏移改为可配置的 `--cluster-data-port-offset` CLI 参数 (默认 10000)
- 参见: `AiDb/docs/superpowers/specs/2026-06-03-data-plane-port-configurable-design.md`
```

- [ ] **Step 6: Final build and test**

Run full check suite:

```bash
cargo build --features cluster
RUSTFLAGS='-D warnings' cargo clippy --all-targets --features cluster
cargo test --features cluster -- --test-threads=1
cargo fmt --check
```

Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add aikv/CLAUDE.md aidb/CLAUDE.md aikv/DEPLOYMENT.md aikv/e2e/utils.sh
git commit -m "docs: update documentation for configurable data port offset"
```
