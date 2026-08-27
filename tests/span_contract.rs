//! @component aidb-observability
//! 热路径 span 级别契约测试 (源码扫描, 无需运行服务器)
//!
//! 验证 AGENTS.md 硬约束: 热路径 (put/get/write/WAL/MemTable/SSTable/block/Raft
//! apply/propose) 的 `#[tracing::instrument]` 必须显式 `level = "debug"`, 保证生产
//! `RUST_LOG=info` 不创建热路径 span. 源码扫描防止 level 回退到默认 Info.
//!
//! ```bash
//! cargo test --test span_contract -- --test-threads=1
//! ```

#[path = "modules/span_contract.rs"]
mod span_contract;
