//! OpenRaft storage — LSM backend for Raft logs and state machine.

mod apply;
pub(crate) mod keys;
mod log;
mod snapshot;

use std::fmt::Debug;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::path::PathBuf;
use std::sync::Arc;

use openraft::{
  storage::{LogState, Snapshot},
  BasicNode, Entry, LogId, RaftLogReader, RaftStorage, SnapshotMeta, StorageError, Vote,
};
use parking_lot::RwLock;
use tracing::instrument;

pub use keys::DEFAULT_GROUP_ID;

use crate::cluster::meta_state_machine::MetaStateMachine;
use crate::cluster::meta_types::METARAFT_GROUP_ID;
use crate::cluster::storage::log::{db_to_storage_err, db_to_storage_write_err};
use crate::cluster::storage::snapshot::SnapshotKv;
use crate::cluster::types::{NodeId, Response, TypeConfig};
use crate::error::{ClusterError, Result};
use crate::DB;

#[derive(Debug, Clone, Default)]
pub(crate) struct StorageState {
  vote: Option<Vote<NodeId>>,
  last_purged_log_id: Option<LogId<NodeId>>,
  last_log_id: Option<LogId<NodeId>>,
  last_applied: Option<LogId<NodeId>>,
  snapshot_meta: Option<SnapshotMeta<NodeId, BasicNode>>,
}

#[derive(Clone)]
pub struct OpenRaftStorage {
  pub(crate) db: Arc<DB>,
  pub(crate) state: Arc<RwLock<StorageState>>,
  pub(crate) group_id: u64,
  pub(crate) meta_state: Option<Arc<MetaStateMachine>>,
  pub(crate) snapshot_temp_path: Arc<RwLock<Option<PathBuf>>>,
}

impl OpenRaftStorage {
  pub fn new(
    db: Arc<DB>,
    group_id: u64,
    meta_state: Option<Arc<MetaStateMachine>>,
  ) -> Result<Self> {
    let storage = Self {
      db,
      state: Arc::new(RwLock::new(StorageState::default())),
      group_id,
      meta_state,
      snapshot_temp_path: Arc::new(RwLock::new(None)),
    };
    storage.load_state()?;
    Ok(storage)
  }

  pub fn db(&self) -> &Arc<DB> {
    &self.db
  }

  pub fn group_id(&self) -> u64 {
    self.group_id
  }
}

impl RaftLogReader<TypeConfig> for OpenRaftStorage {
  async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
    &mut self,
    range: RB,
  ) -> std::result::Result<Vec<Entry<TypeConfig>>, StorageError<NodeId>> {
    self.get_log_entries(range).map_err(db_to_storage_err)
  }
}

impl RaftStorage<TypeConfig> for OpenRaftStorage {
  type LogReader = Self;
  type SnapshotBuilder = snapshot::OpenRaftSnapshotBuilder;

  async fn get_log_state(
    &mut self,
  ) -> std::result::Result<LogState<TypeConfig>, StorageError<NodeId>> {
    let state = self.state.read();
    Ok(LogState {
      last_purged_log_id: state.last_purged_log_id,
      last_log_id: state.last_log_id,
    })
  }

  async fn get_log_reader(&mut self) -> Self::LogReader {
    self.clone()
  }

  #[instrument(name = "raft_save_vote", skip(self), fields(term = vote.leader_id.term))]
  async fn save_vote(
    &mut self,
    vote: &Vote<NodeId>,
  ) -> std::result::Result<(), StorageError<NodeId>> {
    self
      .save_vote_internal(vote)
      .map_err(db_to_storage_write_err)
  }

  async fn read_vote(&mut self) -> std::result::Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
    Ok(self.state.read().vote)
  }

  #[instrument(name = "raft_append_log", skip(self, entries))]
  async fn append_to_log<I>(&mut self, entries: I) -> std::result::Result<(), StorageError<NodeId>>
  where
    I: IntoIterator<Item = Entry<TypeConfig>> + Send,
  {
    let entries_vec: Vec<_> = entries.into_iter().collect();
    self
      .append_log_entries(&entries_vec)
      .map_err(db_to_storage_write_err)
  }

  async fn delete_conflict_logs_since(
    &mut self,
    log_id: LogId<NodeId>,
  ) -> std::result::Result<(), StorageError<NodeId>> {
    self
      .delete_logs_from(log_id)
      .map_err(db_to_storage_write_err)
  }

  async fn purge_logs_upto(
    &mut self,
    log_id: LogId<NodeId>,
  ) -> std::result::Result<(), StorageError<NodeId>> {
    self
      .purge_logs_upto_internal(log_id)
      .map_err(db_to_storage_write_err)
  }

  async fn last_applied_state(
    &mut self,
  ) -> std::result::Result<
    (
      Option<LogId<NodeId>>,
      openraft::StoredMembership<NodeId, BasicNode>,
    ),
    StorageError<NodeId>,
  > {
    let state = self.state.read();
    let membership = self.load_membership().map_err(db_to_storage_err)?;
    Ok((state.last_applied, membership))
  }

  #[instrument(name = "raft_apply_sm", skip(self, entries), fields(entry_count = entries.len()))]
  async fn apply_to_state_machine(
    &mut self,
    entries: &[Entry<TypeConfig>],
  ) -> std::result::Result<Vec<Response>, StorageError<NodeId>> {
    self
      .apply_entries_internal(entries)
      .map_err(db_to_storage_write_err)
  }

  #[instrument(name = "raft_snapshot", skip(self))]
  async fn get_current_snapshot(
    &mut self,
  ) -> std::result::Result<Option<Snapshot<TypeConfig>>, StorageError<NodeId>> {
    let state = self.state.read();
    let Some(meta) = state.snapshot_meta.clone() else {
      return Ok(None);
    };
    drop(state);

    let pairs = if self.group_id == METARAFT_GROUP_ID {
      snapshot::OpenRaftSnapshotBuilder::scan_meta_pairs(&self.db).map_err(db_to_storage_err)?
    } else {
      snapshot::OpenRaftSnapshotBuilder::scan_sm_pairs(&self.db, self.group_id)
        .map_err(db_to_storage_err)?
    };
    let body = SnapshotKv::new(pairs);
    let data = rmp_serde::to_vec(&body).map_err(|e| {
      db_to_storage_write_err(crate::error::Error::Cluster(ClusterError::Serialization(
        e.to_string(),
      )))
    })?;
    Ok(Some(Snapshot {
      meta,
      snapshot: Box::new(Cursor::new(data)),
    }))
  }

  async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
    snapshot::OpenRaftSnapshotBuilder::new(self.db.clone(), self.group_id)
  }

  #[instrument(name = "raft_recv_snapshot", skip(self))]
  async fn begin_receiving_snapshot(
    &mut self,
  ) -> std::result::Result<Box<Cursor<Vec<u8>>>, StorageError<NodeId>> {
    let db_path = self.db.path().to_path_buf();
    let temp_path = db_path.join(format!(".snapshot_temp_{}", self.group_id));
    // Clean up any previous temp file from a crashed install
    let _ = std::fs::remove_file(&temp_path);
    *self.snapshot_temp_path.write() = Some(temp_path);
    Ok(Box::new(Cursor::new(Vec::new())))
  }

  #[instrument(name = "raft_install_snapshot", skip(self, snapshot))]
  async fn install_snapshot(
    &mut self,
    meta: &SnapshotMeta<NodeId, BasicNode>,
    snapshot: Box<Cursor<Vec<u8>>>,
  ) -> std::result::Result<(), StorageError<NodeId>> {
    let data = snapshot.into_inner();

    // Write to temp file first for crash safety
    if let Some(ref temp_path) = *self.snapshot_temp_path.read() {
      if let Err(e) = std::fs::write(temp_path, &data) {
        let _ = std::fs::remove_file(temp_path);
        return Err(db_to_storage_write_err(crate::error::Error::Io(e)));
      }
      // fsync the parent directory to ensure metadata durability
      if let Some(parent) = temp_path.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
          let _ = dir.sync_all();
        }
      }
    }

    // Apply the snapshot data atomically
    self.install_snapshot_atomic(meta, &data).map_err(|e| {
      // Clean up temp on install failure
      if let Some(ref temp_path) = *self.snapshot_temp_path.read() {
        let _ = std::fs::remove_file(temp_path);
      }
      *self.snapshot_temp_path.write() = None;
      db_to_storage_write_err(e)
    })?;

    // Clean up temp file after successful install
    if let Some(ref temp_path) = *self.snapshot_temp_path.read() {
      let _ = std::fs::remove_file(temp_path);
    }
    *self.snapshot_temp_path.write() = None;

    Ok(())
  }
}
