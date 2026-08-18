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

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::cluster::meta_raft_node::MetaRaftNode;
use crate::cluster::meta_types::{MetaRequest, SlotMigrationState, SLOT_COUNT};
use crate::cluster::multi_raft_node::MultiRaftNode;
use crate::cluster::router::Router;
use crate::cluster::types::{ClusterError, NodeId, Request, Response};
use crate::config::MigrationConfig;
use crate::error::Result;

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

// ---------------------------------------------------------------------------
// SlotMigrationExecutor
// ---------------------------------------------------------------------------

pub struct SlotMigrationExecutor {
    meta_raft: Arc<MetaRaftNode>,
    multi_raft: Arc<MultiRaftNode>,
    checkpoint_dir: PathBuf,
    cancellation: Arc<AtomicBool>,
    config: MigrationConfig,
}

impl SlotMigrationExecutor {
    pub fn new(
        meta_raft: Arc<MetaRaftNode>,
        multi_raft: Arc<MultiRaftNode>,
        checkpoint_dir: PathBuf,
        config: MigrationConfig,
    ) -> Self {
        std::fs::create_dir_all(&checkpoint_dir).ok();
        Self {
            meta_raft,
            multi_raft,
            checkpoint_dir,
            cancellation: Arc::new(AtomicBool::new(false)),
            config,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.load(Ordering::Relaxed)
    }

    pub fn request_cancellation(&self) {
        self.cancellation.store(true, Ordering::Relaxed);
    }

    #[instrument(skip(self))]
    pub async fn execute(&self, migration: ActiveMigration) -> Result<BatchMigrateResult> {
        let last_key = self.load_checkpoint(migration.migration_id);

        // Scan source group keys
        let all_keys = self
            .multi_raft
            .scan_keys(migration.source_group, None)
            .await?;
        // Filter by slot range
        let slot_set: std::collections::HashSet<u16> = migration.slots.iter().copied().collect();
        let mut filtered: Vec<Vec<u8>> = all_keys
            .into_iter()
            .filter(|k| slot_set.contains(&crate::cluster::router::key_to_slot(k)))
            .collect();
        filtered.sort();

        // Apply checkpoint resume
        let start_idx = last_key.as_ref().map_or(0, |lk| {
            filtered
                .iter()
                .position(|k| k >= lk)
                .unwrap_or(filtered.len())
        });

        let total_keys = filtered.len() as u64;
        let mut migrated_count = 0u64;
        let mut last_migrated_key: Option<Vec<u8>> = None;

        for chunk in filtered[start_idx..].chunks(self.config.max_batch_size) {
            if self.is_cancelled() {
                self.save_checkpoint(migration.migration_id, &last_migrated_key)?;
                return Ok(BatchMigrateResult {
                    migrated_count,
                    failed_count: 0,
                    last_migrated_key: last_migrated_key.clone(),
                    is_completed: false,
                });
            }

            for key in chunk {
                let value = self
                    .multi_raft
                    .get_key_from_group(migration.source_group, key)
                    .await?;
                if let Some(v) = value {
                    self.multi_raft
                        .propose_group(
                            migration.target_group,
                            Request::PutConditional {
                                key: key.clone(),
                                value: v,
                                // FIX-0056-A1: 让 apply 内查 Del tombstone,
                                // 不复活迁移期已被客户端 DEL 掉的 key.
                                migration_epoch: Some(migration.migration_id),
                            },
                        )
                        .await?;
                }
                migrated_count += 1;
                last_migrated_key = Some(key.clone());

                if migrated_count.is_multiple_of(self.config.progress_report_interval) {
                    let _ = self
                        .meta_raft
                        .propose(MetaRequest::UpdateMigrationProgress {
                            progress: migrated_count,
                            total: total_keys,
                        })
                        .await;
                }
                self.save_checkpoint(migration.migration_id, &last_migrated_key)?;
            }
        }

        // Verify (deterministic step sampling — no rand dependency needed)
        self.verify_migration(
            migration.source_group,
            migration.target_group,
            &migration.slots,
        )
        .await?;

        // Cleanup checkpoint
        self.delete_checkpoint(migration.migration_id)?;

        tracing::info!(
            migrated_count,
            is_completed = true,
            "migration execution completed"
        );

        Ok(BatchMigrateResult {
            migrated_count,
            failed_count: 0,
            last_migrated_key,
            is_completed: true,
        })
    }

    async fn verify_migration(
        &self,
        source_group: u64,
        target_group: u64,
        slots: &[u16],
    ) -> Result<()> {
        // Deterministic step sampling: pick every N-th key in sorted order
        let source_keys = self.multi_raft.scan_keys(source_group, None).await?;
        let slot_set: std::collections::HashSet<u16> = slots.iter().copied().collect();
        let mut slot_keys: Vec<&Vec<u8>> = source_keys
            .iter()
            .filter(|k| slot_set.contains(&crate::cluster::router::key_to_slot(k)))
            .collect();
        slot_keys.sort();

        if slot_keys.is_empty() {
            return Ok(());
        }

        let sample_count =
            ((slot_keys.len() as f64).sqrt() * self.config.verify_sample_factor) as usize;
        let sample_count = sample_count.max(1).min(slot_keys.len());
        let step = slot_keys.len() / sample_count;

        for i in 0..sample_count {
            let key = slot_keys[i * step];
            let src_val = self
                .multi_raft
                .get_key_from_group(source_group, key)
                .await?;
            let tgt_val = self
                .multi_raft
                .get_key_from_group(target_group, key)
                .await?;

            match (src_val, tgt_val) {
                (Some(sv), Some(tv)) if sv != tv => {
                    return Err(ClusterError::Internal(format!(
                        "migration verification failed for key {:?}",
                        key
                    ))
                    .into());
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn save_checkpoint(&self, migration_id: u64, last_key: &Option<Vec<u8>>) -> Result<()> {
        if let Some(key) = last_key {
            let tmp = self
                .checkpoint_dir
                .join(format!("migration_{}.tmp", migration_id));
            let final_path = self
                .checkpoint_dir
                .join(format!("migration_{}.ckpt", migration_id));
            std::fs::write(&tmp, key).map_err(ClusterError::Io)?;
            std::fs::rename(&tmp, &final_path).map_err(ClusterError::Io)?;
        }
        Ok(())
    }

    fn load_checkpoint(&self, migration_id: u64) -> Option<Vec<u8>> {
        let path = self
            .checkpoint_dir
            .join(format!("migration_{}.ckpt", migration_id));
        std::fs::read(&path).ok()
    }

    fn delete_checkpoint(&self, migration_id: u64) -> Result<()> {
        let path = self
            .checkpoint_dir
            .join(format!("migration_{}.ckpt", migration_id));
        let _ = std::fs::remove_file(&path);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SlotMigrationManager
// ---------------------------------------------------------------------------

pub struct SlotMigrationManager {
    meta_raft: Arc<MetaRaftNode>,
    multi_raft: Arc<MultiRaftNode>,
    // kept for reserved/future use
    #[expect(dead_code)]
    router: Arc<Router>,
    node_id: NodeId,
    executor: RwLock<Option<SlotMigrationExecutor>>,
    /// 最近一次 `run_pending_migration` 的结果, 用于收尾 / commit 校验.
    last_run: RwLock<Option<CompletedRun>>,
    checkpoint_dir: PathBuf,
    config: MigrationConfig,
}

impl SlotMigrationManager {
    pub fn new(
        meta_raft: Arc<MetaRaftNode>,
        multi_raft: Arc<MultiRaftNode>,
        router: Arc<Router>,
        node_id: NodeId,
        checkpoint_dir: PathBuf,
        config: MigrationConfig,
    ) -> Self {
        std::fs::create_dir_all(&checkpoint_dir).ok();
        let last_run = Self::load_run_record(&checkpoint_dir);
        Self {
            meta_raft,
            multi_raft,
            router,
            node_id,
            executor: RwLock::new(None),
            last_run: RwLock::new(last_run),
            checkpoint_dir,
            config,
        }
    }

    fn run_record_path(dir: &std::path::Path) -> PathBuf {
        dir.join(MIGRATION_RUN_FILE)
    }

    fn quiesce_token_path(&self) -> PathBuf {
        self.checkpoint_dir.join(QUIESCE_TOKEN_FILE)
    }

    fn load_run_record(dir: &std::path::Path) -> Option<MigrationRunRecord> {
        let path = Self::run_record_path(dir);
        let bytes = std::fs::read(&path).ok()?;
        bincode::deserialize(&bytes).ok()
    }

    fn persist_run_record(&self, record: &MigrationRunRecord) -> Result<()> {
        let path = Self::run_record_path(&self.checkpoint_dir);
        let tmp = self
            .checkpoint_dir
            .join(format!("{}.tmp", MIGRATION_RUN_FILE));
        let bytes =
            bincode::serialize(record).map_err(|e| ClusterError::Serialization(e.to_string()))?;
        std::fs::write(&tmp, bytes).map_err(ClusterError::Io)?;
        std::fs::rename(&tmp, &path).map_err(ClusterError::Io)?;
        Ok(())
    }

    fn clear_run_record(&self) {
        let _ = std::fs::remove_file(Self::run_record_path(&self.checkpoint_dir));
        *self.last_run.write() = None;
    }

    fn write_quiesce_token(&self, token: u64) -> Result<()> {
        let path = self.quiesce_token_path();
        let tmp = self
            .checkpoint_dir
            .join(format!("{}.tmp", QUIESCE_TOKEN_FILE));
        std::fs::write(&tmp, token.to_string()).map_err(ClusterError::Io)?;
        std::fs::rename(&tmp, &path).map_err(ClusterError::Io)?;
        Ok(())
    }

    fn read_quiesce_token(&self) -> Option<u64> {
        let s = std::fs::read_to_string(self.quiesce_token_path()).ok()?;
        s.trim().parse().ok()
    }

    fn clear_quiesce_token(&self) {
        let _ = std::fs::remove_file(self.quiesce_token_path());
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// 单 key 迁移常达不到 `progress_report_interval`, Meta 可能仍停在 Prepare;
    /// Freeze 要求 Migrating, 因此在收尾前补一条进度推进.
    async fn ensure_migrating_phase(&self) -> Result<()> {
        match self.meta_raft.get_migration_state() {
            Some(SlotMigrationState::Prepare { .. }) => {
                let total = self
                    .last_run
                    .read()
                    .as_ref()
                    .map(|r| r.slots.len().max(1) as u64)
                    .unwrap_or(1);
                let progress = total.max(1);
                match self
                    .meta_raft
                    .propose(MetaRequest::UpdateMigrationProgress { progress, total })
                    .await?
                {
                    Response::Ok => Ok(()),
                    Response::Error(e) => Err(ClusterError::InvalidState(e).into()),
                    other => Err(ClusterError::Internal(format!(
                        "unexpected response advancing migration phase: {other:?}"
                    ))
                    .into()),
                }
            }
            Some(SlotMigrationState::Migrating { .. })
            | Some(SlotMigrationState::Frozen { .. })
            | Some(SlotMigrationState::ReadyToCommit { .. }) => Ok(()),
            None => Err(ClusterError::InvalidState("no active migration".into()).into()),
        }
    }

    fn require_completed_run_matching(
        &self,
        source_group: u64,
        target_group: u64,
        slots: &[u16],
    ) -> Result<MigrationRunRecord> {
        if self.executor.read().is_some() {
            return Err(ClusterError::InvalidState(
                "migration has not been executed yet (run_pending_migration was never called)"
                    .into(),
            )
            .into());
        }
        let run = self
            .last_run
            .read()
            .clone()
            .or_else(|| Self::load_run_record(&self.checkpoint_dir));
        match run {
            Some(r)
                if r.completed
                    && r.source_group == source_group
                    && r.target_group == target_group
                    && r.slots == slots
                    && r.node_id == self.node_id =>
            {
                Ok(r)
            }
            Some(_) => Err(ClusterError::InvalidState(
                "migration run record does not match current (source, target, slots) or node"
                    .into(),
            )
            .into()),
            None => Err(ClusterError::InvalidState(
                "migration progress not verified as complete for the current (source, target, \
                 slots); run_pending_migration must finish with is_completed=true first"
                    .into(),
            )
            .into()),
        }
    }

    /// Migrating → Frozen. 要求本节点已完成匹配的 execute.
    #[instrument(skip(self))]
    pub async fn freeze_for_commit(&self) -> Result<()> {
        let (source_group, target_group, slots) = self.current_migration_signature()?;
        self.require_completed_run_matching(source_group, target_group, &slots)?;
        self.ensure_migrating_phase().await?;

        match self
            .meta_raft
            .propose(MetaRequest::FreezeSlotMigration)
            .await?
        {
            Response::Ok => {}
            Response::Error(e) => return Err(ClusterError::InvalidState(e).into()),
            other => {
                return Err(ClusterError::Internal(format!(
                    "unexpected FreezeSlotMigration response: {other:?}"
                ))
                .into());
            }
        }

        // 确保磁盘上有 run record (execute 成功时已写; 此处幂等加固).
        if let Some(run) = self.last_run.read().clone() {
            self.persist_run_record(&run)?;
        }
        Ok(())
    }

    /// Frozen 后向 target propose 写屏障并短暂稳定观察, 再落盘 quiesce_token.
    #[instrument(skip(self))]
    pub async fn quiesce_writes(&self) -> Result<()> {
        match self.meta_raft.get_migration_state() {
            Some(SlotMigrationState::Frozen { target_group, .. }) => {
                let target = target_group;
                let work = async {
                    let token = Self::now_ms() ^ self.node_id.wrapping_mul(0x9e37);
                    match self
                        .multi_raft
                        .propose_group(target, Request::MigrationBarrier { token })
                        .await?
                    {
                        Response::Ok => {}
                        Response::Error(e) => {
                            return Err(ClusterError::InvalidState(format!(
                                "migration barrier failed: {e}"
                            ))
                            .into());
                        }
                        other => {
                            return Err(ClusterError::Internal(format!(
                                "unexpected MigrationBarrier response: {other:?}"
                            ))
                            .into());
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(QUIESCE_STABLE_MS)).await;
                    self.write_quiesce_token(token)?;
                    Ok(())
                };
                match tokio::time::timeout(
                    std::time::Duration::from_millis(QUIESCE_TIMEOUT_MS),
                    work,
                )
                .await
                {
                    Ok(r) => r,
                    Err(_) => Err(ClusterError::Timeout("quiesce_writes timed out".into()).into()),
                }
            }
            Some(_) => Err(ClusterError::InvalidState(
                "quiesce_writes requires Frozen migration phase".into(),
            )
            .into()),
            None => Err(ClusterError::InvalidState("no active migration".into()).into()),
        }
    }

    /// FIX-0056-A1: Frozen 后确认 target group 迁移 oplog tip 已稳定
    /// (`quiesce_writes` 之后, `final_verify` 之前的收尾前置) —— 两次相隔
    /// `QUIESCE_STABLE_MS` 读到的 tip 相等即认为 drain 完成. tip 在
    /// `MigrationWrite` apply 内单调递增, Frozen 下没有新客户端写会自然稳定.
    #[instrument(skip(self))]
    pub async fn drain_oplog_tip_stable(&self) -> Result<()> {
        let (source_group, target_group, slots) = match self.meta_raft.get_migration_state() {
            Some(SlotMigrationState::Frozen {
                source_group,
                target_group,
                slots,
            }) => (source_group, target_group, slots),
            Some(_) => {
                return Err(ClusterError::InvalidState(
                    "drain_oplog_tip_stable requires Frozen migration phase".into(),
                )
                .into());
            }
            None => return Err(ClusterError::InvalidState("no active migration".into()).into()),
        };
        let run = self.require_completed_run_matching(source_group, target_group, &slots)?;
        let epoch = run.migration_id;

        let work = async {
            let first = self
                .multi_raft
                .read_migration_tip(target_group, epoch)
                .await?;
            tokio::time::sleep(std::time::Duration::from_millis(QUIESCE_STABLE_MS)).await;
            let second = self
                .multi_raft
                .read_migration_tip(target_group, epoch)
                .await?;
            if first != second {
                return Err(ClusterError::InvalidState(
                    "migration oplog tip not stable yet, retry drain".into(),
                )
                .into());
            }
            Ok(())
        };
        match tokio::time::timeout(std::time::Duration::from_millis(QUIESCE_TIMEOUT_MS), work).await
        {
            Ok(r) => r,
            Err(_) => Err(ClusterError::Timeout("drain_oplog_tip_stable timed out".into()).into()),
        }
    }

    /// 收尾就绪检查: Frozen + run record 签名匹配 + quiesce_token 存在.
    /// 不做 source⊆target 存在性比对, 也不做 PutConditional 补拷.
    /// FIX-0056-A1: 调用顺序上位于 `drain_oplog_tip_stable` 之后 (见
    /// `finish_migration`) —— 本函数不重复校验 tip 是否稳定, 只信任链路顺序.
    #[instrument(skip(self))]
    pub async fn final_verify(&self) -> Result<()> {
        let (source_group, target_group, slots) = match self.meta_raft.get_migration_state() {
            Some(SlotMigrationState::Frozen {
                source_group,
                target_group,
                slots,
            }) => (source_group, target_group, slots),
            Some(_) => {
                return Err(ClusterError::InvalidState(
                    "final_verify requires Frozen migration phase".into(),
                )
                .into());
            }
            None => {
                return Err(ClusterError::InvalidState("no active migration".into()).into());
            }
        };
        self.require_completed_run_matching(source_group, target_group, &slots)?;
        if self.read_quiesce_token().is_none() {
            return Err(ClusterError::InvalidState(
                "quiesce_token missing; quiesce_writes must succeed before final_verify".into(),
            )
            .into());
        }
        Ok(())
    }

    /// Frozen → ReadyToCommit. 要求 quiesce_token 已落盘 (选择 A: Manager 内校验).
    #[instrument(skip(self))]
    pub async fn mark_ready(&self) -> Result<()> {
        self.final_verify().await?;
        if self.read_quiesce_token().is_none() {
            return Err(ClusterError::InvalidState(
                "quiesce_token missing; cannot MarkMigrationReady".into(),
            )
            .into());
        }
        match self
            .meta_raft
            .propose(MetaRequest::MarkMigrationReady)
            .await?
        {
            Response::Ok => {}
            Response::Error(e) => return Err(ClusterError::InvalidState(e).into()),
            other => {
                return Err(ClusterError::Internal(format!(
                    "unexpected MarkMigrationReady response: {other:?}"
                ))
                .into());
            }
        }
        self.clear_quiesce_token();
        Ok(())
    }

    /// 完整收尾链: freeze → quiesce → drain_oplog_tip_stable → final_verify
    /// → mark_ready → commit.
    #[instrument(skip(self))]
    pub async fn finish_migration(&self) -> Result<()> {
        self.freeze_for_commit().await?;
        self.quiesce_writes().await?;
        self.drain_oplog_tip_stable().await?;
        self.final_verify().await?;
        self.mark_ready().await?;
        self.commit_migration().await
    }

    #[instrument(skip(self))]
    pub async fn start_migration(
        &self,
        source_group: u64,
        target_group: u64,
        slots: Vec<u16>,
    ) -> Result<u64> {
        // Validate
        if source_group == target_group {
            return Err(
                ClusterError::InvalidConfig("source and target group must differ".into()).into(),
            );
        }
        if slots.is_empty() {
            return Err(ClusterError::InvalidConfig("slots must not be empty".into()).into());
        }
        {
            let mut sorted = slots.clone();
            sorted.sort();
            sorted.dedup();
            if sorted.len() != slots.len() {
                return Err(ClusterError::InvalidConfig("slots contains duplicates".into()).into());
            }
        }
        for &s in &slots {
            if s >= SLOT_COUNT as u16 {
                return Err(ClusterError::InvalidConfig("slot out of range".into()).into());
            }
        }

        let cluster_meta = self.meta_raft.get_cluster_meta();
        if !cluster_meta.groups.contains_key(&source_group) {
            return Err(ClusterError::InvalidState("source group not found".into()).into());
        }
        if !cluster_meta.groups.contains_key(&target_group) {
            return Err(ClusterError::InvalidState("target group not found".into()).into());
        }
        if self.meta_raft.get_migration_state().is_some() {
            return Err(ClusterError::InvalidState("migration already in progress".into()).into());
        }

        // Begin via MetaRaft
        self.meta_raft
            .propose(MetaRequest::BeginSlotMigration {
                source_group,
                target_group,
                slots: slots.clone(),
            })
            .await?;

        let migration_id = self.meta_raft.get_cluster_meta().version;
        let executor = SlotMigrationExecutor::new(
            self.meta_raft.clone(),
            self.multi_raft.clone(),
            self.checkpoint_dir.clone(),
            self.config.clone(),
        );

        let _migration = ActiveMigration {
            migration_id,
            source_group,
            target_group,
            slots,
            checkpoint: Vec::new(),
        };

        // Store executor; caller invokes run_pending_migration() to execute.
        *self.executor.write() = Some(executor);

        Ok(migration_id)
    }

    /// Run the pending migration synchronously in the current task.
    ///
    /// Must be called after `start_migration()` to drive actual key migration.
    /// Without this call, the migration remains in `Prepare` state indefinitely.
    /// Caller should spawn this on a separate task if concurrent execution is desired.
    pub async fn run_pending_migration(
        &self,
        migration: ActiveMigration,
    ) -> Result<BatchMigrateResult> {
        let exec = self.executor.write().take();
        let executor = match exec {
            Some(e) => e,
            None => return Err(ClusterError::InvalidState("no active executor".into()).into()),
        };
        let result = executor.execute(migration.clone()).await?;
        // 记录这一批 (source, target, slots) 是否真正跑完了整个拷贝
        // (`is_completed=true`) —— 收尾链 / `commit_migration` 靠这份记录.
        let record = MigrationRunRecord {
            source_group: migration.source_group,
            target_group: migration.target_group,
            slots: migration.slots,
            completed: result.is_completed,
            node_id: self.node_id,
            completed_at_ms: Self::now_ms(),
            migration_id: migration.migration_id,
        };
        *self.last_run.write() = Some(record.clone());
        if result.is_completed {
            self.persist_run_record(&record)?;
        }
        Ok(result)
    }

    pub fn get_migration_status(&self) -> Result<Option<MigrationProgress>> {
        let state = self.meta_raft.get_migration_state();
        match state {
            None => Ok(None),
            Some(s) => {
                let (source_group, target_group, slots) = match &s {
                    SlotMigrationState::Prepare {
                        source_group,
                        target_group,
                        slots,
                    }
                    | SlotMigrationState::Frozen {
                        source_group,
                        target_group,
                        slots,
                    }
                    | SlotMigrationState::ReadyToCommit {
                        source_group,
                        target_group,
                        slots,
                    } => (*source_group, *target_group, slots.clone()),
                    SlotMigrationState::Migrating {
                        source_group,
                        target_group,
                        slots,
                        ..
                    } => (*source_group, *target_group, slots.clone()),
                };
                let (completed_keys, total_keys) = match &s {
                    SlotMigrationState::Migrating {
                        progress, total, ..
                    } => (*progress, *total),
                    _ => (0, 0),
                };
                let phase = match &s {
                    SlotMigrationState::Prepare { .. } => MigrationPhase::Prepare,
                    SlotMigrationState::Migrating { .. } => MigrationPhase::Migrating,
                    SlotMigrationState::Frozen { .. } => MigrationPhase::Frozen,
                    SlotMigrationState::ReadyToCommit { .. } => MigrationPhase::ReadyToCommit,
                };
                Ok(Some(MigrationProgress {
                    migration_id: self.meta_raft.get_cluster_meta().version,
                    source_group,
                    target_group,
                    slots,
                    completed_keys,
                    total_keys,
                    state: phase,
                }))
            }
        }
    }

    /// 提交迁移 — 将 slot 所有权原子切换到 target_group.
    ///
    /// 校验规则:
    /// 1. Meta 相位必须为 `ReadyToCommit` (F-056; Meta validate 是权威, 此处双保险).
    /// 2. `executor` 仍然存在时拒绝 —— target 上必然没有完整拷贝.
    /// 3. 必须存在匹配 `(source, target, slots)` 且 `completed` 的 run record.
    #[instrument(skip(self))]
    pub async fn commit_migration(&self) -> Result<()> {
        let (source_group, target_group, slots) = self.current_migration_signature()?;

        if !matches!(
            self.meta_raft.get_migration_state(),
            Some(SlotMigrationState::ReadyToCommit { .. })
        ) {
            return Err(ClusterError::InvalidState(
                "commit requires ReadyToCommit state; call finish_migration (or mark_ready) first"
                    .into(),
            )
            .into());
        }

        let run = self.require_completed_run_matching(source_group, target_group, &slots)?;

        self.meta_raft
            .propose(MetaRequest::CommitSlotMigration)
            .await?;
        // FIX-0056-A1: oplog GC 是清理性动作 (风险矩阵: "Commit 后异步删除;
        // 可限速"), 失败不应回滚已经生效的 Commit —— 记警告, 允许后续重试.
        if let Err(e) = self
            .cleanup_migration_oplog(target_group, run.migration_id)
            .await
        {
            tracing::warn!(
                target_group,
                epoch = run.migration_id,
                error = %e,
                "migration oplog gc failed after commit, safe to retry later (best effort)"
            );
        }
        self.clear_run_record();
        self.clear_quiesce_token();
        Ok(())
    }

    /// 取消迁移 — 先 Meta 回滚到 Assigned(source), 再清理 target 残留.
    ///
    /// F-056: Ready/Frozen 下读可能已指向 target; 必须先 Cancel 让读回 source,
    /// 再 `cleanup_target_residuals`, 避免读空洞.
    #[instrument(skip(self))]
    pub async fn cancel_migration(&self) -> Result<()> {
        let (_source_group, target_group, slots) = self.current_migration_signature()?;

        if let Some(ref e) = *self.executor.read() {
            e.request_cancellation();
        }

        self.meta_raft
            .propose(MetaRequest::CancelSlotMigration)
            .await?;

        for _ in 0..CANCEL_META_WAIT_ATTEMPTS {
            if self.meta_raft.get_migration_state().is_none() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(CANCEL_META_WAIT_MS)).await;
        }
        if self.meta_raft.get_migration_state().is_some() {
            return Err(ClusterError::InvalidState(
                "CancelSlotMigration proposed but migration_state still visible".into(),
            )
            .into());
        }

        self.cleanup_target_residuals(target_group, &slots).await?;
        // FIX-0056-A1: cancel 可能发生在 execute 从未跑完 (甚至从未开始)
        // 的窗口, epoch 未必已知 —— 找不到就跳过, 只记警告 (best effort,
        // 残留 tombstone 不影响正确性, 只是占一点空间直到下次 GC/复用).
        match self.current_migration_epoch(target_group, &slots) {
            Some(epoch) => {
                if let Err(e) = self.cleanup_migration_oplog(target_group, epoch).await {
                    tracing::warn!(
                        target_group,
                        epoch,
                        error = %e,
                        "migration oplog gc failed on cancel (best effort)"
                    );
                }
            }
            None => tracing::warn!(
                target_group,
                "migration oplog epoch unknown on this node, skipping oplog gc on cancel (best effort)"
            ),
        }
        self.executor.write().take();
        self.clear_run_record();
        self.clear_quiesce_token();
        Ok(())
    }

    /// FIX-0056-A1: 尽力获取当前迁移的 oplog epoch, 不要求 `run_pending_migration`
    /// 已跑完 (`cancel_migration` 可能发生在拷贝完成前). 找不到匹配
    /// `(target_group, slots)` 的本地记录时返回 `None`.
    fn current_migration_epoch(&self, target_group: u64, slots: &[u16]) -> Option<u64> {
        let run = self
            .last_run
            .read()
            .clone()
            .or_else(|| Self::load_run_record(&self.checkpoint_dir))?;
        (run.target_group == target_group && run.slots == slots).then_some(run.migration_id)
    }

    /// FIX-0056-A1: 通过 Raft apply 删除 target group 上 `epoch` 对应的全部
    /// `mig/{gid}/{epoch}/*` tombstone/tip key. 调用方按 best-effort 处理
    /// 返回的错误 (不阻塞已经生效的 Commit/Cancel).
    async fn cleanup_migration_oplog(&self, target_group: u64, epoch: u64) -> Result<()> {
        match self
            .multi_raft
            .propose_group(target_group, Request::MigrationGc { epoch })
            .await?
        {
            Response::Ok => Ok(()),
            Response::Error(e) => {
                Err(ClusterError::InvalidState(format!("migration oplog gc failed: {e}")).into())
            }
            other => Err(ClusterError::Internal(format!(
                "unexpected MigrationGc response: {other:?}"
            ))
            .into()),
        }
    }

    /// 从 MetaRaft 的权威迁移状态读取当前 `(source_group, target_group,
    /// slots)`, 而不是从本地 `executor`/`last_run` 推断 —— 这两者在
    /// `run_pending_migration` 执行后会被清空/消费, 不能用来判断"当前是否有
    /// 迁移在进行".
    fn current_migration_signature(&self) -> Result<(u64, u64, Vec<u16>)> {
        match self.meta_raft.get_migration_state() {
            None => Err(ClusterError::InvalidState("no active migration".into()).into()),
            Some(SlotMigrationState::Prepare {
                source_group,
                target_group,
                slots,
            })
            | Some(SlotMigrationState::Frozen {
                source_group,
                target_group,
                slots,
            })
            | Some(SlotMigrationState::ReadyToCommit {
                source_group,
                target_group,
                slots,
            }) => Ok((source_group, target_group, slots)),
            Some(SlotMigrationState::Migrating {
                source_group,
                target_group,
                slots,
                ..
            }) => Ok((source_group, target_group, slots)),
        }
    }

    /// 删除 target_group 上属于 `slots` 的所有残留 key.
    async fn cleanup_target_residuals(&self, target_group: u64, slots: &[u16]) -> Result<()> {
        let slot_set: HashSet<u16> = slots.iter().copied().collect();
        let keys = self.multi_raft.scan_keys(target_group, None).await?;
        for key in keys {
            if slot_set.contains(&crate::cluster::router::key_to_slot(&key)) {
                self.multi_raft
                    .propose_group(target_group, Request::Delete { key })
                    .await?;
            }
        }
        Ok(())
    }
}
