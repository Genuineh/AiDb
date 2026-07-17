# AiDb — AI 助手指南

## 进仓必读

1. 先读 [CONTEXT.md](CONTEXT.md) — 领域术语以此为准, 不发明同义词
2. 通用沟通 / 代码原则 / Git / 文档同步由 **vibe-coding** 全局 Rules 提供 (`~/.cursor/rules/`), 本文件不重复
3. 开发流程按下方「工作流」走; Skills 来自 vibe-coding (`code-review` / `grilling` / `plan-review` / `handoff`)

## 项目是什么

**AiDb** 是 Rust **嵌入式 LSM-Tree KV 引擎** (lib crate).

- **单机**: WAL → MemTable → SSTable → Leveled Compaction; `put/get/delete/scan`、MVCC Snapshot、WriteBatch
- **集群** (`cluster` feature): MetaRaft (控制面) + MultiRaft (数据面); 16384 slot (CRC16, 与 Redis Cluster 一致)
- **生态**: 存储与共识层; Redis RESP / Cluster 协议在 **AiKv** (`../aikv`)

公共 API 同步; async 调用方用 `spawn_blocking`. 改 WAL / SSTable / Manifest / Compaction / **proto** 视为高风险.

排查 **LSM / Raft / Compaction** → 本仓库; 排查 **Redis 兼容性** → [../aikv/AGENTS.md](../aikv/AGENTS.md).

## 技术栈与参考

拿不准时: 先查「权威对照」, 再对照「刻意差异」; 术语见 [CONTEXT.md](CONTEXT.md).

AiDb **不实现** RESP / INFO / redis_exporter; Redis 协议语义在 **AiKv**.

| 层 | 本项目选型 | 权威对照 (拿不准时查) | 刻意差异 (勿照抄) |
|----|------------|----------------------|-------------------|
| **LSM-Tree** | 自研: WAL、SkipMap MemTable、SSTable、Leveled Compaction、Bloom、Block Cache | RocksDB leveled compaction 思路; 教学向 [mini-lsm](https://skyzh.github.io/mini-lsm/) | 非 RocksDB API/格式兼容; 不引入 LevelDB 代码 |
| **Raft** | OpenRaft 0.9 (`cluster` feature) | [OpenRaft 文档与示例](https://docs.rs/openraft/latest/openraft/) | 不照搬 etcd/raft 的存储与 API 边界; TinyKV 仅作流程直觉 |
| **MultiRaft** | MetaRaft (控制面) + MultiRaft (数据面) + LifecycleManager; 在线 slot 迁移 | TiKV raftstore 的 Group 生命周期思路 | **slot 固定 16384**, 非 TiKV Region 分裂/merge; 拓扑走 MetaRaft, 非 Redis gossip 共识 |
| **节点 RPC** | tonic + prost, `proto/raft.proto` (Vote / AppendEntries / InstallSnapshot) | gRPC 惯例; proto 与 OpenRaft 类型对齐 | 自定义 proto, 非 etcd gRPC 协议 |
| **Slot 模型** | 16384 slot, CRC16 — 与 Redis Cluster 槽位计算一致 | [Redis Cluster spec](https://redis.io/docs/latest/operate/oss_and_stack/reference/cluster-spec/) (仅 slot 数与哈希) | MOVED/ASK、CLUSTER 子命令在 AiKv; 勿按 Redis 16379 bus 臆测本库行为 |
| **引擎指标** | `aidb_*` 经 OTLP 出口 | [docs/modules/observability.md](docs/modules/observability.md) | **不进** Redis INFO / redis_exporter |

Redis 数据结构编码、RESP、INFO — 见 [../aikv/AGENTS.md](../aikv/AGENTS.md).

## 工作流 (vibe-coding)

按任务类型选入口 (详见全局 rule `workflows-routing`):

| 类型 | 流程 |
|------|------|
| **新功能 / 大改** (多文件、新模块、架构) | brainstorming → writing-plans → **plan-review** → **开分支** → implement → **code-review** → **documentation-sync** → 用户确认后 commit |
| **小改 / bug** (已知问题、单点) | **grilling** → **开分支** → implement → **code-review** → **documentation-sync** → 用户确认后 commit |

补充约定:

- plan / spec 是工作区根 `superpower/` 下的**过程制品**, 不进本仓、**也不从本仓文档引用**; 对仓库仍有效的结论须写入本仓 `docs/` / DESIGN / ARCHITECTURE (见 `documentation-sync`)
- **开分支**: 共识之后、改代码之前从原分支拉新分支; 纯文档微调或用户要求就地改时可跳过 (先问); 计划完成后经允许再 squash 回原分支
- **code-review** 通过后做 **documentation-sync**, 再请用户确认; 只在用户明确要求时 commit; 不推远程
- 会话切换用 **handoff** → 写工作区根 `CHAT.md`

不确定大改还是小改时, 先问用户.

## 本仓硬约束

- **Span**: 热路径 (`put/get/write/WAL/MemTable/SSTable/block/Raft apply/propose`) 的 `#[tracing::instrument]` 用 `level = "debug"`; 生产 `RUST_LOG=info` 不建热路径 span
- **指标**: 引擎指标前缀 `aidb_*`, 经 OTLP, **不进** Redis INFO
- **MultiRaft 数据面端口**: `rpc_port + offset`; offset 由 AiKv `--cluster-data-port-offset` 配置 (默认 10000)
- 修 bug **必带** 回归测: [CONTRIBUTING.md §回归测试](CONTRIBUTING.md#回归测试-必带)
- 新测写法与落点 (硬性): [tests/README.md §测试写法与范围 (硬性)](tests/README.md#测试写法与范围-硬性)
- 验证: `RUSTFLAGS='-D warnings'`; 测试加 `--test-threads=1`; `cluster` feature 需 protoc; 完整命令见 [CONTRIBUTING.md](CONTRIBUTING.md)

## 进一步阅读

- [ARCHITECTURE.md](ARCHITECTURE.md) · [DESIGN.md](DESIGN.md) · [docs/README.md](docs/README.md)
- [CONTRIBUTING.md](CONTRIBUTING.md) · [`.github/README.md`](.github/README.md)
- [../aikv/AGENTS.md](../aikv/AGENTS.md) — Redis 协议 / INFO 对照入口
