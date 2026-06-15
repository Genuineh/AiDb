//! 子系统管线: 测试侧直连, 不经 `DB` 公共 API
//!
//! ```bash
//! cargo test --test pipeline -- --test-threads=1
//! cargo test --test pipeline wal_memtable
//! ```

mod common;

#[path = "pipeline/wal_memtable.rs"]
mod wal_memtable;
