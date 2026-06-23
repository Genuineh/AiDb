# AiDb — AI 助手指南

## 项目是什么

**AiDb** 是用 Rust 实现的 **嵌入式 LSM-Tree KV 存储引擎** (lib crate).

- **单机**: WAL → MemTable → SSTable → Leveled Compaction; `put/get/delete/scan`、MVCC Snapshot、WriteBatch、备份恢复.
- **集群** (`cluster` feature): **MetaRaft** 管元数据 + **Multi-Raft** 管数据; **16384 slot** (CRC16, 与 Redis Cluster 槽模型一致); 成员变更与 slot 迁移.
- **生态**: 存储与共识层; **AiKv** 在其上实现 Redis RESP / Cluster 协议 (AiKv 通过 `path = "../aidb"` 依赖本库).

核心路径尽量 **零可选依赖**; 集群、监控、备份等通过 Cargo feature 按需启用.

## 架构要点

- LSM 写路径: 顺序 WAL + MemTable flush + 后台 Compaction; 读路径: MemTable → SSTable, Bloom Filter + Block Cache 控放大.
- 纯 Rust 实现; 对外 API 精简, 实现细节 `pub(crate)` 隔离.
- 集群: OpenRaft 共识 + gRPC (`proto/raft.proto`, `build.rs` 生成, 需 protoc); MetaRaft 与 MultiRaft 分离 (控制面低频 / 数据面高吞吐).
- Slot 路由与 per-Group 独立 DB; Redis Cluster **协议语义** 由 AiKv 实现, AiDb 提供 slot 与 Multi-Raft 存储.
- 公共 API **同步**; async 调用方 (AiKv) 用 `spawn_blocking` 包装.
- 改 WAL / SSTable / Manifest / Compaction 或 **proto** 视为高风险, 需充分测试.

## 技术栈与参考

| 领域 | 本项目 | 可参考 (设计/实现, 非直接依赖) |
|------|--------|--------------------------------|
| **LSM-Tree** | 自研引擎: WAL、SkipMap MemTable、SSTable、Leveled Compaction、Bloom、Block Cache | LevelDB / RocksDB 文档与格式思路; 教学向可看 mini-lsm |
| **Raft 共识** | **OpenRaft** 0.9 (`cluster` feature) | TinyKV、etcd/raft 的 Raft 流程与测试组织 |
| **Multi-Raft** | MetaRaft (元数据) + MultiRaft (按 slot/Group 多 Raft + LifecycleManager); 在线 slot 迁移 | TiKV raftstore、TinyKV 的 Multi-Raft / Region 生命周期思路 (本项目为 **slot**, 非 TiKV Region 分裂) |
| **节点 RPC** | **tonic** + **prost**, `proto/raft.proto` (Vote / AppendEntries / InstallSnapshot) | gRPC 惯例; 协议与 OpenRaft 类型对齐 |

Redis 数据结构编码、RESP、轻量 Gossip 在 **AiKv** 侧; 见 AiKv `AGENTS.md`.

## 开发与 CI

贡献流程与完整测试矩阵见 [CONTRIBUTING.md](CONTRIBUTING.md). CI 流程见 [`.github/README.md`](.github/README.md).

```bash
./install-hooks.sh
export RUSTFLAGS='-D warnings'
cargo fmt --check
cargo clippy --all-targets
cargo clippy --all-targets --features cluster   # 需 protoc
cargo test -- --test-threads=1
cargo test --features cluster -- --test-threads=1
```

慢测与压测 (CI: `test-slow`):

```bash
cargo test -- --ignored --test-threads=1
```

修 bug **必带** 回归测: 见 [CONTRIBUTING.md §回归测试](CONTRIBUTING.md#回归测试-必带).

## 已知限制

- OpenRaft / 快照 API 随上游演进, 升级需适配.
- MultiRaft 数据面 gRPC 端口: `rpc_port + offset`; offset 由 AiKv `--cluster-data-port-offset` 配置 (默认 10000).

## 进一步阅读

- [README.md](README.md)
- [ARCHITECTURE.md](ARCHITECTURE.md)
- [docs/README.md](docs/README.md) — 按域 WHEN 与 modules 导航
- [.github/README.md](.github/README.md)
