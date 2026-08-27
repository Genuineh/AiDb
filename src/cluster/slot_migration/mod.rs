//! 在线 slot 迁移 — `SlotMigrationExecutor` (逐 key 拷贝) + `SlotMigrationManager`
//! (编排收尾). 把 slot 所有权从 source group 原子切换到 target group, 全程经
//! MetaRaft (`\x00meta_raft/migration_state`) 记录权威状态.
//!
//! # 迁移流程
//!
//! ```text
//! start_migration(source, target, slots)
//!   └─ MetaRaft BeginSlotMigration -> migration_id = cluster_meta.version (epoch)
//!   └─ run_pending_migration -> SlotMigrationExecutor::execute
//!        ├─ scan_keys(source) -> 按 slot 过滤 -> 排序
//!        ├─ 分批: get_key_from_group(source) -> propose_group(target,
//!        │        PutConditional{epoch})  <- apply 内查 Del tombstone
//!        ├─ checkpoint (migration_{id}.ckpt) 断点续跑
//!        ├─ 进度 UpdateMigrationProgress
//!        └─ verify_migration (确定性抽样比对 source/target)
//!   └─ finish_migration (收尾链)
//!        ├─ freeze_for_commit      Migrating -> Frozen
//!        ├─ quiesce_writes         target MigrationBarrier + quiesce_token
//!        ├─ drain_oplog_tip_stable 两次读 tip (read_migration_tip) 相等
//!        ├─ final_verify           run record + token 校验
//!        ├─ mark_ready             Frozen -> ReadyToCommit
//!        └─ commit_migration       ReadyToCommit -> Assigned(target) + GC mig oplog
//! ```
//!
//! 读侧一致性 (FIX-0056-A1): 迁移期间对 slot 的读优先读 target 的
//! `read_migration_tip` (tombstone/tip 在 target group 的 Raft apply 内单调分配),
//! source 被 Del tombstone 标记的 key 不再返回, 未迁移的 key 回落到 source —
//! 合并读 (`multi_raft_node.rs` 的 `get_key_from_group_remote`) 消除"先查后写"窗口.
//!
//! # Invariant
//!
//! - `commit_migration` 仅接受 `ReadyToCommit` 状态 (Meta validate + Manager 双保险).
//! - Cancel 先 Meta 回滚 (读回 source), 再清理 target 残留 — 避免 Frozen 下
//!   先清 target 造成读空洞.
//! - `MigrationGc` 只删 `\x02mig/{gid}/{epoch}/*`, 不动用户 `sm_key`.
//! - tip 只在 target group Raft apply 内单调分配; 读 tip 必须走 leader 语义,
//!   不允许落后 follower 冒充最新.

use serde::{Deserialize, Serialize};

use crate::cluster::types::NodeId;

/// Freeze 后写屏障的稳定观察间隔 (ms).
const QUIESCE_STABLE_MS: u64 = 50;
/// `quiesce_writes` 整体超时 (ms).
const QUIESCE_TIMEOUT_MS: u64 = 5000;
/// Cancel 后等待 Meta 可见为 none 的轮询上限.
const CANCEL_META_WAIT_ATTEMPTS: u32 = 50;
const CANCEL_META_WAIT_MS: u64 = 20;

const MIGRATION_RUN_FILE: &str = "migration_run.bin";
const QUIESCE_TOKEN_FILE: &str = "quiesce_token";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationPhase {
    Prepare,
    Migrating,
    Frozen,
    ReadyToCommit,
}

#[derive(Debug, Clone)]
pub struct MigrationProgress {
    pub migration_id: u64,
    pub source_group: u64,
    pub target_group: u64,
    pub slots: Vec<u16>,
    pub completed_keys: u64,
    pub total_keys: u64,
    pub state: MigrationPhase,
}

#[derive(Debug, Clone)]
pub struct ActiveMigration {
    pub migration_id: u64,
    pub source_group: u64,
    pub target_group: u64,
    pub slots: Vec<u16>,
    pub checkpoint: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct BatchMigrateResult {
    pub migrated_count: u64,
    pub failed_count: u64,
    pub last_migrated_key: Option<Vec<u8>>,
    pub is_completed: bool,
}

/// 记录最近一次 `run_pending_migration` 的执行结果, 供收尾链 /
/// `commit_migration` 校验"是否真的跑过一遍拷贝且跑完了", 而不是仅凭
/// `executor` 字段是否为空来判断 (`executor` 在 `run_pending_migration`
/// 一开始就被 take 走, 无法用来区分"从未执行"和"已经执行完毕").
///
/// `(source_group, target_group, slots)` 三元组用于防止 commit 时复用一次
/// 已经过期/属于另一批迁移的完成标记. 成功完成后会持久化到
/// `checkpoint_dir/migration_run.bin`, 以便 Frozen 重启后仍可续跑收尾.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MigrationRunRecord {
    source_group: u64,
    target_group: u64,
    slots: Vec<u16>,
    completed: bool,
    node_id: NodeId,
    #[serde(default)]
    completed_at_ms: u64,
    /// FIX-0056-A1: 迁移 oplog epoch (= `BeginSlotMigration` 时的
    /// `cluster_meta.version`, 即 `ActiveMigration.migration_id`).
    /// `drain_oplog_tip_stable` / GC 用它定位 `mig/{gid}/{epoch}/*`.
    #[serde(default)]
    migration_id: u64,
}

/// 内存侧别名, 与历史 `CompletedRun` 语义一致.
type CompletedRun = MigrationRunRecord;

mod executor;
mod manager;

pub use executor::SlotMigrationExecutor;
pub use manager::SlotMigrationManager;
