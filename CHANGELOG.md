# Changelog

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/),
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/).

## [Unreleased]

### Added

### Changed

- cluster 层模块拆分: `network` / `multi_raft_node` / `slot_migration` / `meta_state_machine` 改为子目录, 生成代码 `aidb.raft.rs` 随 `network/` 隔离
- CONTRIBUTING 补充 test-compression 与 partition/failover 测试说明
- 同步与 AiKv 的依赖关系 (git 依赖 + 本地 patch)
- 全面重构并优化项目核心文档与模块文档体系
- 统一贡献指南结构并全面规范化 Markdown 文档与标点

### Fixed

- 热路径 tracing 收敛: 写路径、raft apply/append/RPC 与 sst 构建的 `info!` 降为 `debug!`, `db_delete_range` span 补 `level = "debug"` 并纳入 span_contract 契约
- 依赖安全: `crossbeam-epoch` 0.9.18 → 0.9.20 (RUSTSEC-2026-0204), `deny.toml` 豁免 hashbrown 构建链重复版本
- `test_flush_reclaim` 断言放宽为容忍后台 flush 竞态 (对齐 `test_auto_flush_on_memtable_full`), 消除 flaky

