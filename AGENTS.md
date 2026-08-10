# AiDb — AI 助手指南

## 项目简介

**AiDb** 是 Rust 实现的**嵌入式 LSM-Tree KV 存储引擎库** (lib crate, 无网络服务).

- **单机**: WAL → MemTable → SSTable → Leveled Compaction; `put/get/delete/scan`、MVCC Snapshot、WriteBatch
- **集群** (`cluster` feature): MetaRaft (控制面) + MultiRaft (数据面); 16384 slot (CRC16, 与 Redis Cluster 一致)
- **生态**: 存储与共识层; Redis RESP / Cluster 协议在 **AiKv** (`../aikv`)

## 技术栈与参考

仓库专业术语见 [CONTEXT.md](CONTEXT.md).

| 层 | 本项目选型 | 主流实现参考 | 取舍 / 差异 |
|----|-----------|-------------|------------|
| **LSM-Tree** | 自研: WAL、SkipMap MemTable、SSTable、Leveled Compaction、Bloom、Block Cache | RocksDB leveled compaction 思路; 教学向 [mini-lsm](https://skyzh.github.io/mini-lsm/) | 非 RocksDB API/格式兼容; 不引入 LevelDB 代码 |
| **Raft** | OpenRaft 0.10 (`cluster` feature) | [OpenRaft 文档与示例](https://docs.rs/openraft/latest/openraft/) | 不照搬 etcd/raft 的存储与 API 边界; TinyKV 仅作流程直觉 |
| **MultiRaft** | MetaRaft (控制面) + MultiRaft (数据面) + LifecycleManager; 在线 slot 迁移 | TiKV raftstore 的 Group 生命周期思路 | **slot 固定 16384**, 非 TiKV Region 分裂/merge; 拓扑走 MetaRaft, 非 Redis gossip 共识 |
| **节点 RPC** | tonic + prost, `proto/raft.proto` (Vote / AppendEntries / InstallSnapshot) | gRPC 惯例; proto 与 OpenRaft 类型对齐 | 自定义 proto, 非 etcd gRPC 协议 |
| **Slot 模型** | 16384 slot, CRC16 — 与 Redis Cluster 槽位计算一致; slot 计算与路由在 aidb (`key_to_slot`), 含 hash tag | [Redis Cluster spec](https://redis.io/docs/latest/operate/oss_and_stack/reference/cluster-spec/) (仅 slot 数与哈希) | MOVED/ASK、CLUSTER 子命令在 AiKv (协议层); 勿按 Redis 16379 bus 臆测本库行为 |

> **决策说明**: 上述选型仅为参考, 先由 AI 列出候选方案优劣, 再供开发者决策.

## 本仓硬约束

- **API 形态**: 公共 API 同步; async 调用方用 `spawn_blocking`. 改 WAL / SSTable / Manifest / Compaction / **proto** 视为高风险
- **Span**: 热路径 (`put/get/write/WAL/MemTable/SSTable/block/Raft apply/propose`) 的 `#[tracing::instrument]` 用 `level = "debug"`; 生产 `RUST_LOG=info` 不建热路径 span
- **指标**: 引擎指标前缀 `aidb_*`, 经 OTLP, **不进** Redis INFO
- **测试纪律**: 修 bug 必带回归测 ([CONTRIBUTING.md §回归测试](CONTRIBUTING.md#回归测试-必带)); 新测写法与落点 ([tests/README.md §测试写法与范围 (硬性)](tests/README.md#测试写法与范围-硬性)); 验证 `RUSTFLAGS='-D warnings'` + 测试 `--test-threads=1`, `cluster` feature 需 protoc (完整命令见 [CONTRIBUTING.md](CONTRIBUTING.md))
- **文档同步 (强制)**: 改公共 API / 行为 / 模块边界必须同步对应 `docs/modules/*.md` 与根文档; commit 消息修 bug 须带 `(ISSUE-NNN)`; 不满足不进 commit (见 [CONTRIBUTING.md §文档同步](CONTRIBUTING.md#文档同步-硬性))

## 进一步阅读

- [ARCHITECTURE.md](ARCHITECTURE.md) · [DESIGN.md](DESIGN.md) · [docs/README.md](docs/README.md)
- [CONTRIBUTING.md](CONTRIBUTING.md) · `[.github/README.md](.github/README.md)`
- [../aikv/AGENTS.md](../aikv/AGENTS.md) — Redis 协议 / INFO 对照入口
