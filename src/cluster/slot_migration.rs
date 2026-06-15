//! Online slot migration — executor (key-by-key) and manager (orchestration) (Phase 15).

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
          self
            .multi_raft
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
    self
      .verify_migration(
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
          return Err(
            ClusterError::Internal(format!("migration verification failed for key {:?}", key))
              .into(),
          );
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
      return Err(ClusterError::InvalidConfig("source and target group must differ".into()).into());
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
    self
      .meta_raft
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
    match exec {
      Some(e) => e.execute(migration).await,
      None => Err(ClusterError::InvalidState("no active executor".into()).into()),
    }
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

  #[instrument(skip(self))]
  pub async fn commit_migration(&self) -> Result<()> {
    let exec = self.executor.write().take();
    if exec.is_none() {
      return Err(ClusterError::InvalidState("no active migration".into()).into());
    }
    let executor = exec.unwrap();
    if executor.is_cancelled() {
      self.executor.write().replace(executor);
      return Err(ClusterError::InvalidState("migration was cancelled".into()).into());
    }

    self
      .meta_raft
      .propose(MetaRequest::CommitSlotMigration)
      .await?;
    // executor dropped here — resources cleaned up
    Ok(())
  }

  #[instrument(skip(self))]
  pub async fn cancel_migration(&self) -> Result<()> {
    // Scope the read lock so it drops before any .await
    {
      let exec = self.executor.read();
      if exec.is_none() {
        return Err(ClusterError::InvalidState("no active migration".into()).into());
      }
      if let Some(ref e) = *exec {
        e.request_cancellation();
      }
    }

    // Proceed with cancel immediately — executor will detect on next batch
    self
      .meta_raft
      .propose(MetaRequest::CancelSlotMigration)
      .await?;
    self.executor.write().take();
    Ok(())
  }
}
