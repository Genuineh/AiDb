//! OpenRaft storage implementation for AiDb
//!
//! This module implements the OpenRaft storage interface for AiDb.
//! It provides persistent storage for Raft logs, snapshots, and state using LSM-Tree.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::sync::Arc;

#[cfg(feature = "raft-cluster")]
use openraft::{
    storage::{LogState, Snapshot},
    BasicNode, Entry, EntryPayload, LogId, RaftLogReader, RaftSnapshotBuilder, RaftStorage,
    SnapshotMeta, StorageError, StorageIOError, Vote,
};

use crate::error::{Error, Result};
use crate::DB;

/// Node ID type for AiDb Raft cluster
pub type NodeId = u64;

/// Raft log entry type for AiDb
pub type LogEntry = Entry<TypeConfig>;

/// Type configuration for OpenRaft
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TypeConfig;

#[cfg(feature = "raft-cluster")]
impl openraft::RaftTypeConfig for TypeConfig {
    type D = Request;
    type R = Response;
    type NodeId = NodeId;
    type Node = BasicNode;
    type Entry = LogEntry;
    type SnapshotData = Cursor<Vec<u8>>;
    type AsyncRuntime = openraft::TokioRuntime;
    type Responder = openraft::impls::OneshotResponder<TypeConfig>;
}

/// Request type for state machine operations (Thin Replication Support)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Single put operation (backward compatible)
    Put {
        /// Key to insert
        key: Vec<u8>,
        /// Value to insert
        value: Vec<u8>,
    },
    /// Single delete operation (backward compatible)
    Delete {
        /// Key to delete
        key: Vec<u8>,
    },
    /// Batch write operations (thin replication)
    WriteBatch(crate::cluster::thin_replication::WriteBatch),
}

impl Request {
    /// Convert to WriteBatch for uniform processing
    ///
    /// This method provides backward compatibility by converting single
    /// operations to batches, allowing the state machine to handle all
    /// requests uniformly.
    pub fn to_batch(self) -> crate::cluster::thin_replication::WriteBatch {
        use crate::cluster::thin_replication::WriteBatch;
        match self {
            Request::Put { key, value } => {
                let mut batch = WriteBatch::new();
                batch.put(key, value);
                batch
            }
            Request::Delete { key } => {
                let mut batch = WriteBatch::new();
                batch.delete(key);
                batch
            }
            Request::WriteBatch(batch) => batch,
        }
    }
}

/// Response type for state machine operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// Operation successful
    Ok,
    /// Get operation result
    Value(Option<Vec<u8>>),
    /// Error occurred
    Error(String),
}

/// OpenRaft storage implementation using AiDb's LSM-Tree
#[derive(Clone)]
pub struct OpenRaftStorage {
    /// Local database for storing Raft data
    db: Arc<DB>,
    /// In-memory state cache
    state: Arc<RwLock<StorageState>>,
}

/// Cached storage state
#[derive(Debug, Clone, Default)]
struct StorageState {
    /// Current vote information
    vote: Option<Vote<NodeId>>,
    /// Last purged log ID (compaction point)
    last_purged_log_id: Option<LogId<NodeId>>,
    /// Last log ID in storage
    last_log_id: Option<LogId<NodeId>>,
    /// Last applied log ID
    last_applied: Option<LogId<NodeId>>,
    /// Current snapshot metadata
    snapshot_meta: Option<SnapshotMeta<NodeId, BasicNode>>,
}

impl OpenRaftStorage {
    /// Create a new OpenRaft storage
    pub fn new(db: Arc<DB>) -> Result<Self> {
        let storage =
            Self { db: db.clone(), state: Arc::new(RwLock::new(StorageState::default())) };

        // Load existing state from database
        storage.load_state()?;

        Ok(storage)
    }

    /// Load Raft state from persistent storage
    fn load_state(&self) -> Result<()> {
        let mut state = self.state.write();

        // Load vote information
        if let Some(data) = self.db.get(b"raft:vote")? {
            state.vote = Some(bincode::deserialize(&data).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to deserialize vote: {}", e),
                ))
            })?);
        }

        // Load last purged log ID
        if let Some(data) = self.db.get(b"raft:last_purged_log_id")? {
            state.last_purged_log_id = Some(bincode::deserialize(&data).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to deserialize last_purged_log_id: {}", e),
                ))
            })?);
        }

        // Load last log ID
        if let Some(data) = self.db.get(b"raft:last_log_id")? {
            state.last_log_id = Some(bincode::deserialize(&data).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to deserialize last_log_id: {}", e),
                ))
            })?);
        }

        // Load last applied log ID
        if let Some(data) = self.db.get(b"raft:last_applied")? {
            state.last_applied = Some(bincode::deserialize(&data).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to deserialize last_applied: {}", e),
                ))
            })?);
        }

        // Load snapshot metadata
        if let Some(data) = self.db.get(b"raft:snapshot_meta")? {
            state.snapshot_meta = Some(bincode::deserialize(&data).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to deserialize snapshot_meta: {}", e),
                ))
            })?);
        }

        Ok(())
    }

    /// Apply a WriteBatch to the local DB (Thin Replication)
    ///
    /// This method applies a batch of write operations to the local database.
    /// Each node independently applies these operations, enabling thin replication
    /// where only WAL entries (WriteOps) are replicated, not the full SSTables.
    ///
    /// # Arguments
    ///
    /// * `batch` - The batch of write operations to apply
    ///
    /// # Returns
    ///
    /// * `Result<()>` - Ok if successful, Error otherwise
    fn apply_batch_internal(
        &self,
        batch: &crate::cluster::thin_replication::WriteBatch,
    ) -> Result<()> {
        use crate::cluster::thin_replication::WriteOp;

        // Use AiDb's native WriteBatch for atomic application
        let mut db_batch = crate::WriteBatch::new();

        for op in batch.iter() {
            match op {
                WriteOp::Put { key, value, .. } => {
                    // Add "sm:" prefix for state machine data
                    let sm_key = format!("sm:{}", String::from_utf8_lossy(key));
                    db_batch.put(sm_key.as_bytes(), value);
                }
                WriteOp::Delete { key, .. } => {
                    // Add "sm:" prefix for state machine data
                    let sm_key = format!("sm:{}", String::from_utf8_lossy(key));
                    db_batch.delete(sm_key.as_bytes());
                }
            }
        }

        // Write batch atomically
        self.db.write(db_batch)?;

        Ok(())
    }

    /// Save vote to persistent storage
    fn save_vote_internal(&self, vote: &Vote<NodeId>) -> Result<()> {
        let data = bincode::serialize(vote).map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to serialize vote: {}", e),
            ))
        })?;
        self.db.put(b"raft:vote", &data)?;

        // Update cache
        let mut state = self.state.write();
        state.vote = Some(*vote);

        Ok(())
    }

    /// Get log entries in the given range
    fn get_log_entries(&self, range: impl RangeBounds<u64>) -> Result<Vec<Entry<TypeConfig>>> {
        use std::ops::Bound;

        let state = self.state.read();

        let start = match range.start_bound() {
            Bound::Included(&x) => x,
            Bound::Excluded(&x) => x + 1,
            Bound::Unbounded => {
                if let Some(ref purged) = state.last_purged_log_id {
                    purged.index + 1
                } else {
                    0
                }
            }
        };

        let end = match range.end_bound() {
            Bound::Included(&x) => x + 1,
            Bound::Excluded(&x) => x,
            Bound::Unbounded => {
                if let Some(ref last) = state.last_log_id {
                    last.index + 1
                } else {
                    0
                }
            }
        };

        let mut entries = Vec::new();

        for idx in start..end {
            let key = format!("raft:log:{}", idx);
            if let Some(data) = self.db.get(key.as_bytes())? {
                let entry: Entry<TypeConfig> = bincode::deserialize(&data).map_err(|e| {
                    Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to deserialize entry: {}", e),
                    ))
                })?;
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    /// Append log entries to storage
    fn append_log_entries(&self, entries: &[Entry<TypeConfig>]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut state = self.state.write();

        for entry in entries {
            let key = format!("raft:log:{}", entry.log_id.index);
            let data = bincode::serialize(entry).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to serialize entry: {}", e),
                ))
            })?;
            self.db.put(key.as_bytes(), &data)?;

            // Update last log ID
            state.last_log_id = Some(entry.log_id);
        }

        // Persist last log ID
        if let Some(ref last_log_id) = state.last_log_id {
            let data = bincode::serialize(last_log_id).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to serialize last_log_id: {}", e),
                ))
            })?;
            self.db.put(b"raft:last_log_id", &data)?;
        }

        Ok(())
    }

    /// Delete log entries from a specific log ID
    fn delete_logs_from(&self, log_id: LogId<NodeId>) -> Result<()> {
        let mut state = self.state.write();

        let last_index = if let Some(ref last) = state.last_log_id {
            last.index
        } else {
            return Ok(());
        };

        // Delete all logs from log_id.index onwards
        for idx in log_id.index..=last_index {
            let key = format!("raft:log:{}", idx);
            self.db.delete(key.as_bytes())?;
        }

        // Update last log ID to the entry before the deleted range
        if log_id.index > 0 {
            let prev_key = format!("raft:log:{}", log_id.index - 1);
            if let Some(data) = self.db.get(prev_key.as_bytes())? {
                let entry: Entry<TypeConfig> = bincode::deserialize(&data).map_err(|e| {
                    Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to deserialize entry: {}", e),
                    ))
                })?;
                state.last_log_id = Some(entry.log_id);
            } else {
                state.last_log_id = None;
            }
        } else {
            state.last_log_id = None;
        }

        // Persist last log ID
        if let Some(ref last_log_id) = state.last_log_id {
            let data = bincode::serialize(last_log_id).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to serialize last_log_id: {}", e),
                ))
            })?;
            self.db.put(b"raft:last_log_id", &data)?;
        } else {
            self.db.delete(b"raft:last_log_id")?;
        }

        Ok(())
    }

    /// Purge log entries up to a specific log ID
    fn purge_logs_upto_internal(&self, log_id: LogId<NodeId>) -> Result<()> {
        let mut state = self.state.write();

        let start_index = if let Some(ref purged) = state.last_purged_log_id {
            purged.index + 1
        } else {
            0
        };

        // Delete logs from start_index to log_id.index (inclusive)
        for idx in start_index..=log_id.index {
            let key = format!("raft:log:{}", idx);
            self.db.delete(key.as_bytes())?;
        }

        // Update last purged log ID
        state.last_purged_log_id = Some(log_id);

        // Persist last purged log ID
        let data = bincode::serialize(&log_id).map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to serialize last_purged_log_id: {}", e),
            ))
        })?;
        self.db.put(b"raft:last_purged_log_id", &data)?;

        Ok(())
    }

    /// Cleanup old log entries based on retention policy
    ///
    /// This removes log entries that:
    /// 1. Have been applied to the state machine
    /// 2. Exceed the maximum log entry count
    /// 3. Are covered by a snapshot
    ///
    /// # Arguments
    ///
    /// * `max_entries` - Maximum number of log entries to retain
    /// * `max_size_bytes` - Maximum log size in bytes (0 = unlimited)
    ///
    /// # Returns
    ///
    /// Number of entries purged
    pub fn cleanup_logs(&self, max_entries: u64, max_size_bytes: u64) -> Result<u64> {
        let state = self.state.read();

        let last_applied = match &state.last_applied {
            Some(id) => id.index,
            None => return Ok(0), // Nothing applied yet
        };

        let last_log = match &state.last_log_id {
            Some(id) => id.index,
            None => return Ok(0), // No logs
        };

        let last_purged = state.last_purged_log_id.as_ref().map(|id| id.index).unwrap_or(0);

        drop(state); // Release read lock

        // Calculate the safe purge point
        // We can safely purge up to: min(last_applied, last_log - max_entries)
        let retention_limit = last_log.saturating_sub(max_entries);

        let purge_upto = last_applied.min(retention_limit);

        if purge_upto <= last_purged {
            return Ok(0); // Nothing to purge
        }

        // Calculate log size if size-based cleanup is enabled
        if max_size_bytes > 0 {
            let mut total_size = 0u64;
            let mut entries_checked = 0u64;

            // Scan from newest to oldest to calculate size
            for idx in (last_purged + 1..=last_log).rev() {
                let key = format!("raft:log:{}", idx);
                if let Some(data) = self.db.get(key.as_bytes())? {
                    total_size += data.len() as u64;
                    entries_checked += 1;

                    // If we're over size limit, we need to purge earlier entries
                    if total_size > max_size_bytes {
                        // Calculate adjusted purge point
                        let adjusted_purge = last_log.saturating_sub(entries_checked);

                        // Use the more aggressive limit
                        let final_purge_upto = purge_upto.max(adjusted_purge).min(last_applied);

                        if final_purge_upto > last_purged {
                            return self.purge_logs_internal(final_purge_upto);
                        }
                    }
                }
            }
        }

        // Purge based on entry count
        if purge_upto > last_purged {
            return self.purge_logs_internal(purge_upto);
        }

        Ok(0)
    }

    /// Internal method to purge logs up to a specific index
    fn purge_logs_internal(&self, upto_index: u64) -> Result<u64> {
        let state = self.state.read();
        let last_purged = state.last_purged_log_id.as_ref().map(|id| id.index).unwrap_or(0);
        let last_purged_term =
            state.last_purged_log_id.as_ref().map(|id| id.leader_id.term).unwrap_or(0);
        drop(state);

        if upto_index <= last_purged {
            return Ok(0);
        }

        let mut purged_count = 0u64;

        // Delete log entries
        for idx in (last_purged + 1)..=upto_index {
            let key = format!("raft:log:{}", idx);
            if self.db.get(key.as_bytes())?.is_some() {
                self.db.delete(key.as_bytes())?;
                purged_count += 1;
            }
        }

        // Update last_purged_log_id
        // We need to get the term from the purged entry
        let new_purged_log_id = LogId {
            leader_id: openraft::LeaderId {
                term: last_purged_term,
                node_id: 0, // Will be updated when we have actual term info
            },
            index: upto_index,
        };

        let mut state = self.state.write();
        state.last_purged_log_id = Some(new_purged_log_id);

        // Persist
        let data = bincode::serialize(&new_purged_log_id).map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to serialize last_purged_log_id: {}", e),
            ))
        })?;
        self.db.put(b"raft:last_purged_log_id", &data)?;

        Ok(purged_count)
    }

    /// Get log size statistics
    ///
    /// Returns (total_entries, total_bytes, oldest_index, newest_index)
    pub fn get_log_stats(&self) -> Result<(u64, u64, u64, u64)> {
        let state = self.state.read();

        let last_purged = state.last_purged_log_id.as_ref().map(|id| id.index).unwrap_or(0);
        let last_log = state.last_log_id.as_ref().map(|id| id.index).unwrap_or(0);

        drop(state);

        let mut total_entries = 0u64;
        let mut total_bytes = 0u64;
        let oldest_index = last_purged + 1;
        let newest_index = last_log;

        for idx in oldest_index..=newest_index {
            let key = format!("raft:log:{}", idx);
            if let Some(data) = self.db.get(key.as_bytes())? {
                total_entries += 1;
                total_bytes += data.len() as u64;
            }
        }

        Ok((total_entries, total_bytes, oldest_index, newest_index))
    }
}

/// Snapshot builder for creating snapshots
#[cfg(feature = "raft-cluster")]
pub struct OpenRaftSnapshotBuilder {
    db: Arc<DB>,
}

// Implement RaftLogReader trait for OpenRaftStorage
#[cfg(feature = "raft-cluster")]
impl RaftLogReader<TypeConfig> for OpenRaftStorage {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> std::result::Result<Vec<Entry<TypeConfig>>, StorageError<NodeId>> {
        self.get_log_entries(range).map_err(|e| StorageError::IO {
            source: StorageIOError::read(openraft::AnyError::error(e.to_string())),
        })
    }
}

// Implement RaftSnapshotBuilder trait for creating snapshots
#[cfg(feature = "raft-cluster")]
impl RaftSnapshotBuilder<TypeConfig> for OpenRaftSnapshotBuilder {
    async fn build_snapshot(
        &mut self,
    ) -> std::result::Result<Snapshot<TypeConfig>, StorageError<NodeId>> {
        // Create a snapshot of the current database state
        let snapshot_data = Vec::new();

        // In a real implementation, you would iterate through all state machine keys
        // and serialize them into the snapshot
        let cursor = Cursor::new(snapshot_data);

        // Get current snapshot metadata from storage
        let storage_state = self.db.get(b"raft:snapshot_meta").map_err(|e| StorageError::IO {
            source: StorageIOError::read(openraft::AnyError::error(e.to_string())),
        })?;

        let meta: SnapshotMeta<NodeId, BasicNode> = if let Some(data) = storage_state {
            bincode::deserialize(&data).map_err(|e| StorageError::IO {
                source: StorageIOError::read(openraft::AnyError::error(format!(
                    "Failed to deserialize snapshot_meta: {}",
                    e
                ))),
            })?
        } else {
            // Return a default empty snapshot
            SnapshotMeta {
                last_log_id: None,
                last_membership: Default::default(),
                snapshot_id: String::new(),
            }
        };

        Ok(Snapshot { meta, snapshot: Box::new(cursor) })
    }
}

// Implement RaftStorage trait for OpenRaftStorage
#[cfg(feature = "raft-cluster")]
impl RaftStorage<TypeConfig> for OpenRaftStorage {
    type LogReader = Self;
    type SnapshotBuilder = OpenRaftSnapshotBuilder;

    // Log state
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
        OpenRaftStorage { db: self.db.clone(), state: self.state.clone() }
    }

    // Vote management
    async fn save_vote(
        &mut self,
        vote: &Vote<NodeId>,
    ) -> std::result::Result<(), StorageError<NodeId>> {
        self.save_vote_internal(vote).map_err(|e| StorageError::IO {
            source: StorageIOError::write(openraft::AnyError::error(e.to_string())),
        })
    }

    async fn read_vote(
        &mut self,
    ) -> std::result::Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        let state = self.state.read();
        Ok(state.vote)
    }

    // Log management
    async fn append_to_log<I>(
        &mut self,
        entries: I,
    ) -> std::result::Result<(), StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + Send,
    {
        let entries_vec: Vec<_> = entries.into_iter().collect();
        self.append_log_entries(&entries_vec).map_err(|e| StorageError::IO {
            source: StorageIOError::write(openraft::AnyError::error(e.to_string())),
        })
    }

    async fn delete_conflict_logs_since(
        &mut self,
        log_id: LogId<NodeId>,
    ) -> std::result::Result<(), StorageError<NodeId>> {
        self.delete_logs_from(log_id).map_err(|e| StorageError::IO {
            source: StorageIOError::write(openraft::AnyError::error(e.to_string())),
        })
    }

    async fn purge_logs_upto(
        &mut self,
        log_id: LogId<NodeId>,
    ) -> std::result::Result<(), StorageError<NodeId>> {
        self.purge_logs_upto_internal(log_id).map_err(|e| StorageError::IO {
            source: StorageIOError::write(openraft::AnyError::error(e.to_string())),
        })
    }

    // State machine methods
    async fn last_applied_state(
        &mut self,
    ) -> std::result::Result<
        (Option<LogId<NodeId>>, openraft::StoredMembership<NodeId, BasicNode>),
        StorageError<NodeId>,
    > {
        let state = self.state.read();

        // Get last membership from storage
        let membership = if let Some(data) =
            self.db.get(b"raft:membership").map_err(|e| StorageError::IO {
                source: StorageIOError::read(openraft::AnyError::error(e.to_string())),
            })? {
            bincode::deserialize(&data).map_err(|e| StorageError::IO {
                source: StorageIOError::read(openraft::AnyError::error(format!(
                    "Failed to deserialize membership: {}",
                    e
                ))),
            })?
        } else {
            openraft::StoredMembership::default()
        };

        Ok((state.last_applied, membership))
    }

    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry<TypeConfig>],
    ) -> std::result::Result<Vec<Response>, StorageError<NodeId>> {
        let mut responses = Vec::new();

        for entry in entries {
            if let EntryPayload::Normal(ref request) = entry.payload {
                // Convert request to WriteBatch (thin replication)
                let batch = request.clone().to_batch();

                // Apply batch to local DB
                let response = match self.apply_batch_internal(&batch) {
                    Ok(_) => Response::Ok,
                    Err(e) => Response::Error(format!("Apply failed: {}", e)),
                };

                responses.push(response);

                // Update last applied
                let mut state = self.state.write();
                state.last_applied = Some(entry.log_id);

                // Persist last applied
                let data = bincode::serialize(&entry.log_id).map_err(|e| StorageError::IO {
                    source: StorageIOError::write(openraft::AnyError::error(format!(
                        "Failed to serialize last_applied: {}",
                        e
                    ))),
                })?;
                self.db.put(b"raft:last_applied", &data).map_err(|e| StorageError::IO {
                    source: StorageIOError::write(openraft::AnyError::error(e.to_string())),
                })?;
            } else {
                responses.push(Response::Ok);
            }
        }

        Ok(responses)
    }

    // Snapshot management
    async fn get_current_snapshot(
        &mut self,
    ) -> std::result::Result<Option<Snapshot<TypeConfig>>, StorageError<NodeId>> {
        let state = self.state.read();

        if state.snapshot_meta.is_none() {
            return Ok(None);
        }

        // Get snapshot data from storage
        let snapshot_data = self.db.get(b"raft:snapshot_data").map_err(|e| StorageError::IO {
            source: StorageIOError::read(openraft::AnyError::error(e.to_string())),
        })?;

        match (state.snapshot_meta.as_ref(), snapshot_data) {
            (Some(meta), Some(data)) => {
                Ok(Some(Snapshot { meta: meta.clone(), snapshot: Box::new(Cursor::new(data)) }))
            }
            _ => Ok(None),
        }
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        OpenRaftSnapshotBuilder { db: self.db.clone() }
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> std::result::Result<Box<Cursor<Vec<u8>>>, StorageError<NodeId>> {
        // Return an empty cursor for receiving snapshot data
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<NodeId, BasicNode>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> std::result::Result<(), StorageError<NodeId>> {
        let mut state = self.state.write();

        // Save snapshot metadata
        let meta_data = bincode::serialize(meta).map_err(|e| StorageError::IO {
            source: StorageIOError::write(openraft::AnyError::error(format!(
                "Failed to serialize snapshot meta: {}",
                e
            ))),
        })?;
        self.db.put(b"raft:snapshot_meta", &meta_data).map_err(|e| StorageError::IO {
            source: StorageIOError::write(openraft::AnyError::error(e.to_string())),
        })?;

        // Save snapshot data
        let snapshot_data = snapshot.into_inner();
        self.db
            .put(b"raft:snapshot_data", &snapshot_data)
            .map_err(|e| StorageError::IO {
                source: StorageIOError::write(openraft::AnyError::error(e.to_string())),
            })?;

        // Update state
        state.snapshot_meta = Some(meta.clone());
        state.last_applied = meta.last_log_id;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    use tempfile::TempDir;

    fn create_test_storage() -> (OpenRaftStorage, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        let storage = OpenRaftStorage::new(Arc::new(db)).unwrap();
        (storage, temp_dir)
    }

    #[test]
    fn test_storage_creation() {
        let (storage, _temp_dir) = create_test_storage();
        let state = storage.state.read();
        assert!(state.vote.is_none());
        assert!(state.last_log_id.is_none());
    }

    #[tokio::test]
    async fn test_save_and_read_vote() {
        let (mut storage, _temp_dir) = create_test_storage();

        let vote = Vote { leader_id: openraft::LeaderId::new(1, 1), committed: false };

        storage.save_vote(&vote).await.unwrap();

        let state = storage.state.read();
        assert_eq!(state.vote, Some(vote));
    }

    #[test]
    fn test_append_and_get_entries() {
        let (storage, _temp_dir) = create_test_storage();

        let leader_id = openraft::LeaderId::new(1, 1);
        let entries = vec![
            Entry { log_id: LogId { leader_id, index: 1 }, payload: EntryPayload::Blank },
            Entry { log_id: LogId { leader_id, index: 2 }, payload: EntryPayload::Blank },
        ];

        storage.append_log_entries(&entries).unwrap();

        let retrieved = storage.get_log_entries(1..3).unwrap();
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved[0].log_id.index, 1);
        assert_eq!(retrieved[1].log_id.index, 2);
    }

    #[test]
    fn test_delete_conflict_logs() {
        let (storage, _temp_dir) = create_test_storage();

        let leader_id = openraft::LeaderId::new(1, 1);
        let entries = vec![
            Entry { log_id: LogId { leader_id, index: 1 }, payload: EntryPayload::Blank },
            Entry { log_id: LogId { leader_id, index: 2 }, payload: EntryPayload::Blank },
            Entry { log_id: LogId { leader_id, index: 3 }, payload: EntryPayload::Blank },
        ];

        storage.append_log_entries(&entries).unwrap();

        // Delete logs from index 2 onwards
        storage.delete_logs_from(LogId { leader_id, index: 2 }).unwrap();

        let retrieved = storage.get_log_entries(1..4).unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].log_id.index, 1);
    }

    #[tokio::test]
    async fn test_purge_logs() {
        let (mut storage, _temp_dir) = create_test_storage();

        let leader_id = openraft::LeaderId::new(1, 1);
        let entries = vec![
            Entry { log_id: LogId { leader_id, index: 1 }, payload: EntryPayload::Blank },
            Entry { log_id: LogId { leader_id, index: 2 }, payload: EntryPayload::Blank },
            Entry { log_id: LogId { leader_id, index: 3 }, payload: EntryPayload::Blank },
        ];

        storage.append_log_entries(&entries).unwrap();

        // Purge logs up to index 2
        storage.purge_logs_upto(LogId { leader_id, index: 2 }).await.unwrap();

        // Should not be able to get purged entries
        let retrieved = storage.get_log_entries(1..3).unwrap();
        assert_eq!(retrieved.len(), 0);

        // Should still be able to get entry 3
        let retrieved = storage.get_log_entries(3..4).unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].log_id.index, 3);
    }

    // ===== Thin Replication Tests =====

    #[test]
    fn test_apply_batch_internal_single_put() {
        use crate::cluster::thin_replication::WriteBatch;

        let (storage, _temp_dir) = create_test_storage();

        let mut batch = WriteBatch::new();
        batch.put(b"key1".to_vec(), b"value1".to_vec());

        storage.apply_batch_internal(&batch).unwrap();

        // Verify data was written with "sm:" prefix
        let value = storage.db.get(b"sm:key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_apply_batch_internal_multiple_ops() {
        use crate::cluster::thin_replication::WriteBatch;

        let (storage, _temp_dir) = create_test_storage();

        // First write some data
        storage.db.put(b"sm:key2", b"old_value").unwrap();

        let mut batch = WriteBatch::new();
        batch.put(b"key1".to_vec(), b"value1".to_vec());
        batch.put(b"key2".to_vec(), b"new_value".to_vec());
        batch.delete(b"key3".to_vec());

        storage.apply_batch_internal(&batch).unwrap();

        // Verify all operations applied
        let value1 = storage.db.get(b"sm:key1").unwrap();
        assert_eq!(value1, Some(b"value1".to_vec()));

        let value2 = storage.db.get(b"sm:key2").unwrap();
        assert_eq!(value2, Some(b"new_value".to_vec()));

        let value3 = storage.db.get(b"sm:key3").unwrap();
        assert_eq!(value3, None);
    }

    #[test]
    fn test_apply_batch_internal_with_timestamps() {
        use crate::cluster::thin_replication::WriteBatch;

        let (storage, _temp_dir) = create_test_storage();

        let mut batch = WriteBatch::new();
        batch.put_with_ts(b"key1".to_vec(), b"value1".to_vec(), 12345);
        batch.delete_with_ts(b"key2".to_vec(), 12346);

        // Should work fine - timestamps are preserved in the ops but not used yet
        storage.apply_batch_internal(&batch).unwrap();

        let value1 = storage.db.get(b"sm:key1").unwrap();
        assert_eq!(value1, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_request_to_batch_put() {
        let request = Request::Put { key: b"key".to_vec(), value: b"value".to_vec() };
        let batch = request.to_batch();

        assert_eq!(batch.len(), 1);
        assert!(!batch.is_empty());
    }

    #[test]
    fn test_request_to_batch_delete() {
        let request = Request::Delete { key: b"key".to_vec() };
        let batch = request.to_batch();

        assert_eq!(batch.len(), 1);
        assert!(!batch.is_empty());
    }

    #[test]
    fn test_request_to_batch_writebatch() {
        use crate::cluster::thin_replication::WriteBatch;

        let mut wb = WriteBatch::new();
        wb.put(b"key1".to_vec(), b"value1".to_vec());
        wb.delete(b"key2".to_vec());

        let request = Request::WriteBatch(wb.clone());
        let batch = request.to_batch();

        assert_eq!(batch.len(), 2);
        assert_eq!(batch.ops, wb.ops);
    }

    #[test]
    fn test_batch_estimate_size() {
        use crate::cluster::thin_replication::WriteBatch;

        let mut batch = WriteBatch::new();
        batch.put(vec![0u8; 100], vec![0u8; 1024]); // 100B key + 1KB value
        batch.put(vec![0u8; 50], vec![0u8; 512]); // 50B key + 512B value

        let size = batch.estimate_size();
        // Should be roughly (100 + 1024 + 16) + (50 + 512 + 16) = ~1718
        assert!(size > 1600);
        assert!(size < 2000);

        // Compare to full SSTable replication (would be much larger after compaction)
        println!("Thin replication size: {} bytes", size);
    }
}
