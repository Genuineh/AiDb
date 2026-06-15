# AiDb 测试布局

## 分层

| 层级 | Cargo 入口 | 源码 | 说明 |
|------|------------|------|------|
| **L0** | `cargo test --lib` | `src/**` `#[cfg(test)]` | 单元测试 |
| **L1** | `tests/{wal,memtable,filter,cache,sstable,db,compaction,snapshot,raft}.rs` | `tests/modules/{mod}/` | 单模块功能 + **模块级** tracing (`dataflow.rs`) |
| **L2** | `tests/pipeline.rs`, `tests/engine.rs` | `tests/pipeline/`, `tests/engine/` | 跨模块 / 引擎黑盒 |
| **L3** | `tests/proptest.rs` | `tests/proptest/` | 随机操作 + 引擎不变式 |
| **L4** | `tests/regression.rs` | `tests/regression/` | 已修 bug 固化 |

L5–L7 (协议兼容 / E2E / bench) 在 aikv 或 `benches/`.

## L2 按场景类型

| 子目录 | 入口 | 含义 |
|--------|------|------|
| `pipeline/` | `--test pipeline` | 子系统直连 (如 WAL → MemTable), 不经 `DB::open` |
| `engine/` | `--test engine` | `DB` 公共 API: 全链路、崩溃恢复、compaction 集成、**dataflow** 等 |

新增 L2:

- 能用 `DB::*` → `engine/` 新文件
- 只连部分子系统 → `pipeline/` 新文件
- L3/L4 新增场景 → `proptest/` / `regression/` 新文件, 外层 `#[path]` 挂载
- 勿新建根目录 `*_integration.rs`; 保持少量入口 + 子目录
- **Phase 12 (cluster feature)**: `tests/raft.rs` → `tests/modules/cluster/` (storage/network/node/integration + harness)

`modules/{mod}/dataflow.rs` 是 L1 模块 tracing, 与 L2 `pipeline/` 不同义.

## 常用命令

```bash
cargo test --test wal -- --test-threads=1
cargo test --test memtable -- --test-threads=1
cargo test --test filter -- --test-threads=1
cargo test --test cache -- --test-threads=1
cargo test --test sstable -- --test-threads=1
cargo test --test db -- --test-threads=1
cargo test --test db dataflow -- --test-threads=1
cargo test --test engine dataflow -- --test-threads=1
cargo test --test compaction -- --test-threads=1
cargo test --test snapshot -- --test-threads=1
cargo test --test pipeline -- --test-threads=1
cargo test --test engine -- --test-threads=1
cargo test --test engine compaction -- --test-threads=1
PROPTEST_CASES=100 cargo test --test proptest -- --test-threads=1
cargo test --test regression -- --test-threads=1

# Phase7.6 bench (criterion 参数已内置, 默认 10K preload)
cargo bench --bench write_bench
cargo bench --bench read_bench
# 更大 read 数据集 (可选)
AIDB_BENCH_PRELOAD=100000 cargo bench --bench read_bench

# Phase 12 Raft (需 cluster feature; 集成测必须单线程)
cargo test --features cluster --test raft -- --test-threads=1
cargo test --features cluster raft_storage -- --test-threads=1
cargo test --features cluster raft_3nodes -- --test-threads=1
```
