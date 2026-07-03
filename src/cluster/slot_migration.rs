//! Online slot migration — executor (key-by-key) and manager (orchestration) (Phase 15).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::instrument;

use crate::cluster::meta_raft_node::MetaRaftNode;
use crate::cluster::meta_types::{MetaRequest, SlotMigrationState, SLOT_COUNT};
use crate::cluster::multi_raft_node::MultiRaftNode;
use crate::cluster::router::Router;
use crate::cluster::types::{ClusterError, NodeId, Request};
use crate::config::MigrationConfig;
use crate::error::Result;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationPhase {
    Prepare,
    Migrating,
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

/// 记录最近一次 `run_pending_migration` 的执行结果, 供 `commit_migration`
/// 校验"是否真的跑过一遍拷贝且跑完了", 而不是仅凭 `executor` 字段是否为空
/// 来判断 (`executor` 在 `run_pending_migration` 一开始就被 take 走, 无法
/// 用来区分"从未执行"和"已经执行完毕").
///
/// `(source_group, target_group, slots)` 三元组用于防止 commit 时复用一次
/// 已经过期/属于另一批迁移的完成标记.
#[derive(Debug, Clone)]
struct CompletedRun {
    source_group: u64,
    target_group: u64,
    slots: Vec<u16>,
    completed: bool,
}

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
    #[expect(dead_code)]
    node_id: NodeId,
    executor: RwLock<Option<SlotMigrationExecutor>>,
    /// 最近一次 `run_pending_migration` 的结果, 用于 `commit_migration` 校验进度.
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
        Self {
            meta_raft,
            multi_raft,
            router,
            node_id,
            executor: RwLock::new(None),
            last_run: RwLock::new(None),
            checkpoint_dir,
            config,
        }
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
        // (`is_completed=true`) —— `commit_migration` 靠这份记录, 而不是
        // "executor 是否为空" 来判断能不能提交, 因为上面这行 take() 之后
        // executor 必然为空, 无法区分"从未跑过"与"跑完了".
        *self.last_run.write() = Some(CompletedRun {
            source_group: migration.source_group,
            target_group: migration.target_group,
            slots: migration.slots,
            completed: result.is_completed,
        });
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
    /// 校验规则 (对齐 stage2-migration 修复前发现的问题: `CLUSTER SETSLOT ...
    /// STABLE` 曾经可以在 `run_pending_migration` 从未被调用过的情况下直接
    /// 提交, target 上一个 key 都没有, 提交后该批 slot 的数据全部静默丢失):
    ///
    /// 1. `executor` 仍然存在 (即 `start_migration` 之后从未跑过实际拷贝) 时
    ///    直接拒绝 —— target 上必然没有数据。
    /// 2. `executor` 已被消费 (跑过 `run_pending_migration`) 时, 必须存在一次
    ///    针对*完全相同* `(source_group, target_group, slots)` 的
    ///    `last_run`, 且其 `is_completed == true`, 否则拒绝 (覆盖"跑到一半被
    ///    取消""跑的是另一批 slots 的历史记录"等情况)。
    #[instrument(skip(self))]
    pub async fn commit_migration(&self) -> Result<()> {
        let (source_group, target_group, slots) = self.current_migration_signature()?;

        if self.executor.read().is_some() {
            return Err(ClusterError::InvalidState(
                "migration has not been executed yet (run_pending_migration was never called); \
                 refusing to commit without a verified data copy"
                    .into(),
            )
            .into());
        }

        let verified = matches!(
            &*self.last_run.read(),
            Some(run)
                if run.completed
                    && run.source_group == source_group
                    && run.target_group == target_group
                    && run.slots == slots
        );
        if !verified {
            return Err(ClusterError::InvalidState(
                "migration progress not verified as complete for the current (source, target, \
                 slots); run_pending_migration must finish with is_completed=true first"
                    .into(),
            )
            .into());
        }

        self.meta_raft
            .propose(MetaRequest::CommitSlotMigration)
            .await?;
        *self.last_run.write() = None;
        Ok(())
    }

    /// 取消迁移 — 回滚 slot 所有权到 source_group, 并清理 target 上已拷贝的
    /// 残留副本, 避免这批 slot 未来再次迁入同一个 target 时, `PutConditional`
    /// 因为 key "已存在" 而跳过拷贝, 悄悄让 target 继续持有本次已取消、可能
    /// 早已过期的旧值。
    ///
    /// 清理放在 propose(CancelSlotMigration) *之前*: 清理期间 slot 仍处于
    /// `Migrating(source_group)`, 读仍路由到 source (source 保有完整数据),
    /// 写已 ASK 重定向到 target (见 router.rs 的 ASK-Redirect-Migrate), 不
    /// 影响线上流量; 若清理中途失败, meta 状态仍是
    /// `Migrating`, 可以安全重试 cancel, 不会把一个"target 数据不完整"的
    /// 迁移错误地回滚成"看起来已清理干净"。
    #[instrument(skip(self))]
    pub async fn cancel_migration(&self) -> Result<()> {
        let (_source_group, target_group, slots) = self.current_migration_signature()?;

        if let Some(ref e) = *self.executor.read() {
            e.request_cancellation();
        }

        self.cleanup_target_residuals(target_group, &slots).await?;

        self.meta_raft
            .propose(MetaRequest::CancelSlotMigration)
            .await?;
        self.executor.write().take();
        *self.last_run.write() = None;
        Ok(())
    }

    /// 从 MetaRaft 的权威迁移状态读取当前 `(source_group, target_group,
    /// slots)`, 而不是从本地 `executor`/`last_run` 推断 —— 这两者在
    /// `run_pending_migration` 执行后会被清空/消费, 不能用来判断"当前是否有
    /// 迁移在进行"。
    fn current_migration_signature(&self) -> Result<(u64, u64, Vec<u16>)> {
        match self.meta_raft.get_migration_state() {
            None => Err(ClusterError::InvalidState("no active migration".into()).into()),
            Some(SlotMigrationState::Prepare {
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
