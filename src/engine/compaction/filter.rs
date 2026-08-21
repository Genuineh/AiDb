//! Compaction 过滤器 — 在 entry 写入输出 SST 前判断是否保留.
//!
//! 参考 RocksDB CompactionFilter: 允许应用层在 compaction 合并过程中
//! 丢弃过时或无效的 key-value 条目, 减少磁盘占用.
//!
//! # 副作用边界
//!
//! - [`CompactionFilter`] **必须保持纯决策**: 不修改全局状态、不发起 I/O.
//! - [`CompactionRemovalListener`] 由 `DB::run_compaction_once` 在 **Version 安装、
//!   读视图切换之后** 回调 (不是在 `CompactionJob` merge 热路径). 允许点查引擎.

/// Compaction 过滤决策.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterDecision {
    /// 保留此 entry, 写入输出 SST.
    Keep,
    /// 丢弃此 entry, 不写入输出 SST.
    Remove,
}

/// Compaction 过滤器 trait.
///
/// 实现者需要保证:
/// - **轻量**: 在 compaction worker 线程中执行, 不应阻塞.
/// - **无副作用**: 不应修改全局状态或发起 I/O.
/// - **不 panic**: 丢弃 entry 比崩溃更好; `filter()` 应 catch 所有可能的错误.
pub trait CompactionFilter: Send + Sync {
    /// 对 compaction 中的每条 entry 做过滤.
    ///
    /// - `level`: 输出目标 level
    /// - `key`: InternalKey 字节 (包含 user_key + sequence + value_type)
    /// - `value`: entry 的 value 字节
    ///
    /// 返回 `Keep` 保留, `Remove` 丢弃.
    fn filter(&self, level: usize, key: &[u8], value: &[u8]) -> FilterDecision;
}

/// 当 compaction **完成 Version 安装后**, 对曾被 filter 丢弃的最新 Put 的 user_key 通知上层.
///
/// 调用时机在读视图已切换之后, 便于上层用 `get==None` 安全扣减计数.
/// 同一 user_key 在单次 job 内至多通知一次 (仅最新 Put 被 Remove 时记录).
///
/// 实现者需要保证: 尽量轻量; 允许点查引擎 (此时 Version 已新); 不 panic.
pub trait CompactionRemovalListener: Send + Sync {
    /// `user_key` 为 LSM 用户键 (不含 sequence / value_type 后缀).
    fn on_latest_put_removed(&self, user_key: &[u8]);
}
