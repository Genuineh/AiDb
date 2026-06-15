//! 引擎黑盒: 经 `DB` 公共 API 的场景测试
//!
//! ```bash
//! cargo test --test engine -- --test-threads=1
//! cargo test --test engine scenarios
//! cargo test --test engine crash_recovery
//! cargo test --test engine compaction -- --test-threads=1
//! ```

mod common;

#[path = "engine/scenarios.rs"]
mod scenarios;

#[path = "engine/crash_recovery.rs"]
mod crash_recovery;

#[path = "engine/compaction.rs"]
mod compaction;

#[path = "engine/dataflow.rs"]
mod dataflow;
