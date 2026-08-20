# Changelog

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/),
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/).

## [Unreleased]

### Added

### Changed

- 序列化: `bincode 1.x` → `postcard 1.x` (开发期磁盘格式不兼容: MANIFEST payload, Raft snapshot/membership, MigrationRunRecord checkpoint, RemotePropose)
- 升级 tonic 0.11 → 0.14.6 (tonic-prost), Raft gRPC 生成代码改为 OUT_DIR, 入站连接启用 TCP_NODELAY
- cluster 层模块拆分: `network` / `multi_raft_node` / `slot_migration` / `meta_state_machine` 改为子目录, 生成代码 `aidb.raft.rs` 随 `network/` 隔离
- CONTRIBUTING 补充 test-compression 与 partition/failover 测试说明
- 同步与 AiKv 的依赖关系 (git 依赖 + 本地 patch)
- 全面重构并优化项目核心文档与模块文档体系
- 统一贡献指南结构并全面规范化 Markdown 文档与标点

### Fixed

- 依赖安全: `anyhow` 1.0.102 → 1.0.104 (RUSTSEC-2026-0190)
- 依赖安全: 移除 tonic 0.11 引入的 h2 0.3.27 (RUSTSEC-2026-0258); SKIP_SECURITY 逃生门已删除; deny.toml 允许 Zlib (foldhash)
- 热路径 tracing 收敛: 写路径、raft apply/append/RPC 与 sst 构建的 `info!` 降为 `debug!`, `db_delete_range` span 补 `level = "debug"` 并纳入 span_contract 契约
- 依赖安全: `crossbeam-epoch` 0.9.18 → 0.9.20 (RUSTSEC-2026-0204), `deny.toml` 豁免 hashbrown 构建链重复版本
- `test_flush_reclaim` 断言放宽为容忍后台 flush 竞态 (对齐 `test_auto_flush_on_memtable_full`), 消除 flaky

