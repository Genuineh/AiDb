//! Raft storage implementation for AiDb
//!
//! This module implements the Raft storage interface required by raft-rs.
//! It provides persistent storage for Raft logs, snapshots, and state.

use parking_lot::RwLock;
#[cfg(feature = "raft-cluster")]
use protobuf::Message;
#[cfg(feature = "raft-cluster")]
use raft::{
    eraftpb::{ConfState, Entry, HardState, Snapshot},
    storage::GetEntriesContext,
    Error as RaftError, RaftState, Result as RaftResult, Storage, StorageError,
};
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::DB;

/// Raft storage implementation using AiDb's LSM-Tree
pub struct RaftStorage {
    /// Local database for storing Raft data
    db: Arc<DB>,
    /// In-memory cache of Raft state for fast access
    cache: Arc<RwLock<RaftCache>>,
}

/// Cached Raft state
#[derive(Debug, Clone)]
struct RaftCache {
    /// Hard state (term, vote, commit)
    hard_state: HardState,
    /// Configuration state (voters, learners)
    conf_state: ConfState,
    /// Last snapshot metadata
    snapshot_metadata: Snapshot,
    /// Cached log entries (first_index -> entries)
    #[allow(dead_code)]
    entries: Vec<Entry>,
    /// First log index in storage
    first_index: u64,
    /// Last log index in storage
    last_index: u64,
}

impl Default for RaftCache {
    fn default() -> Self {
        // Initialize snapshot with index 0, which means the log starts from index 1
        let mut snapshot = Snapshot::default();
        let metadata = snapshot.mut_metadata();
        metadata.index = 0;
        metadata.term = 0;

        Self {
            hard_state: HardState::default(),
            conf_state: ConfState::default(),
            snapshot_metadata: snapshot,
            entries: Vec::new(),
            first_index: 1, // Raft log indices start after the snapshot
            last_index: 0,  // 0 means empty log (no entries after snapshot)
        }
    }
}

impl RaftStorage {
    /// Create a new Raft storage
    pub fn new(db: Arc<DB>) -> Result<Self> {
        let storage = Self { db: db.clone(), cache: Arc::new(RwLock::new(RaftCache::default())) };

        // Load existing state from database
        storage.load_state()?;

        Ok(storage)
    }

    /// Load Raft state from persistent storage
    fn load_state(&self) -> Result<()> {
        let mut cache = self.cache.write();

        // Load hard state
        if let Some(data) = self.db.get(b"raft:hard_state")? {
            cache.hard_state = HardState::parse_from_bytes(&data).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to parse hard state: {}", e),
                ))
            })?;
        }

        // Load conf state
        if let Some(data) = self.db.get(b"raft:conf_state")? {
            cache.conf_state = ConfState::parse_from_bytes(&data).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to parse conf state: {}", e),
                ))
            })?;
        }

        // Load snapshot metadata
        if let Some(data) = self.db.get(b"raft:snapshot")? {
            cache.snapshot_metadata = Snapshot::parse_from_bytes(&data).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to parse snapshot: {}", e),
                ))
            })?;
        }

        // Load first and last index
        if let Some(data) = self.db.get(b"raft:first_index")? {
            cache.first_index = bincode::deserialize(&data).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to deserialize first_index: {}", e),
                ))
            })?;
        }

        if let Some(data) = self.db.get(b"raft:last_index")? {
            cache.last_index = bincode::deserialize(&data).map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to deserialize last_index: {}", e),
                ))
            })?;
        }

        Ok(())
    }

    /// Save hard state to persistent storage
    #[allow(dead_code)]
    fn save_hard_state(&self, hs: &HardState) -> Result<()> {
        let data = hs.write_to_bytes().map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to serialize hard state: {}", e),
            ))
        })?;
        self.db.put(b"raft:hard_state", &data)?;
        Ok(())
    }

    /// Save conf state to persistent storage
    fn save_conf_state(&self, cs: &ConfState) -> Result<()> {
        let data = cs.write_to_bytes().map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to serialize conf state: {}", e),
            ))
        })?;
        self.db.put(b"raft:conf_state", &data)?;
        Ok(())
    }

    /// Append log entries to storage
    pub fn append_entries(&self, entries: &[Entry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut cache = self.cache.write();

        for entry in entries {
            let key = format!("raft:log:{}", entry.index);
            let data = entry.write_to_bytes().map_err(|e| {
                Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to serialize entry: {}", e),
                ))
            })?;
            self.db.put(key.as_bytes(), &data)?;

            // Update cache
            if cache.first_index == 0 {
                cache.first_index = entry.index;
            }
            cache.last_index = cache.last_index.max(entry.index);
        }

        // Update last_index in storage
        let data = bincode::serialize(&cache.last_index).map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to serialize last_index: {}", e),
            ))
        })?;
        self.db.put(b"raft:last_index", &data)?;

        Ok(())
    }

    /// Apply snapshot to storage
    pub fn apply_snapshot(&self, snapshot: Snapshot) -> Result<()> {
        let mut cache = self.cache.write();

        // Save snapshot metadata
        let data = snapshot.write_to_bytes().map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to serialize snapshot: {}", e),
            ))
        })?;
        self.db.put(b"raft:snapshot", &data)?;

        // Update cache
        cache.snapshot_metadata = snapshot.clone();

        // Clear old log entries before snapshot
        let metadata = snapshot.get_metadata();
        cache.first_index = metadata.index + 1;

        // Update conf state from snapshot
        if metadata.has_conf_state() {
            let conf_state = metadata.get_conf_state().clone();
            cache.conf_state = conf_state.clone();
            self.save_conf_state(&conf_state)?;
        }

        Ok(())
    }

    /// Compact log entries up to the given index
    pub fn compact(&self, compact_index: u64) -> Result<()> {
        let mut cache = self.cache.write();

        if compact_index <= cache.first_index {
            return Ok(());
        }

        // Delete old log entries
        for idx in cache.first_index..compact_index {
            let key = format!("raft:log:{}", idx);
            self.db.delete(key.as_bytes())?;
        }

        // Update first index
        cache.first_index = compact_index;
        let data = bincode::serialize(&cache.first_index).map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to serialize first_index: {}", e),
            ))
        })?;
        self.db.put(b"raft:first_index", &data)?;

        Ok(())
    }

    /// Get log entries in the given range
    fn get_entries(&self, low: u64, high: u64, max_size: Option<u64>) -> Result<Vec<Entry>> {
        let cache = self.cache.read();

        if low < cache.first_index {
            return Err(Error::ClusterError(format!(
                "Log compacted: requested {} but first_index is {}",
                low, cache.first_index
            )));
        }

        if high > cache.last_index + 1 {
            return Err(Error::ClusterError(format!(
                "Index out of bound: requested {} but last_index is {}",
                high, cache.last_index
            )));
        }

        let mut entries = Vec::new();
        let mut size: u64 = 0;

        for idx in low..high {
            let key = format!("raft:log:{}", idx);
            if let Some(data) = self.db.get(key.as_bytes())? {
                let entry = Entry::parse_from_bytes(&data).map_err(|e| {
                    Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Failed to parse entry: {}", e),
                    ))
                })?;

                size += entry.data.len() as u64;
                entries.push(entry);

                if let Some(max) = max_size {
                    if size >= max {
                        break;
                    }
                }
            }
        }

        Ok(entries)
    }
}

#[cfg(feature = "raft-cluster")]
impl Storage for RaftStorage {
    fn initial_state(&self) -> RaftResult<RaftState> {
        let cache = self.cache.read();
        Ok(
            RaftState {
                hard_state: cache.hard_state.clone(),
                conf_state: cache.conf_state.clone(),
            },
        )
    }

    fn entries(
        &self,
        low: u64,
        high: u64,
        max_size: impl Into<Option<u64>>,
        _context: GetEntriesContext,
    ) -> RaftResult<Vec<Entry>> {
        self.get_entries(low, high, max_size.into())
            .map_err(|e| RaftError::Store(StorageError::Other(Box::new(e))))
    }

    fn term(&self, idx: u64) -> RaftResult<u64> {
        let cache = self.cache.read();

        let snapshot_idx = cache.snapshot_metadata.get_metadata().index;

        // Special case: snapshot index
        if idx == snapshot_idx {
            return Ok(cache.snapshot_metadata.get_metadata().term);
        }

        if idx < cache.first_index {
            return Err(RaftError::Store(StorageError::Compacted));
        }

        if idx > cache.last_index {
            return Err(RaftError::Store(StorageError::Unavailable));
        }

        let key = format!("raft:log:{}", idx);
        if let Some(data) = self
            .db
            .get(key.as_bytes())
            .map_err(|e| RaftError::Store(StorageError::Other(Box::new(e))))?
        {
            let entry = Entry::parse_from_bytes(&data)
                .map_err(|e| RaftError::Store(StorageError::Other(Box::new(e))))?;
            Ok(entry.term)
        } else {
            Err(RaftError::Store(StorageError::Unavailable))
        }
    }

    fn first_index(&self) -> RaftResult<u64> {
        let cache = self.cache.read();
        Ok(cache.first_index)
    }

    fn last_index(&self) -> RaftResult<u64> {
        let cache = self.cache.read();
        Ok(cache.last_index)
    }

    fn snapshot(&self, request_index: u64, _to: u64) -> RaftResult<Snapshot> {
        let cache = self.cache.read();

        let snapshot_index = cache.snapshot_metadata.get_metadata().index;

        if request_index <= snapshot_index {
            Ok(cache.snapshot_metadata.clone())
        } else {
            Err(RaftError::Store(StorageError::SnapshotTemporarilyUnavailable))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    use tempfile::TempDir;

    fn create_test_storage() -> (RaftStorage, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        let storage = RaftStorage::new(Arc::new(db)).unwrap();
        (storage, temp_dir)
    }

    #[test]
    fn test_storage_creation() {
        let (storage, _temp_dir) = create_test_storage();
        let cache = storage.cache.read();
        assert_eq!(cache.first_index, 1); // Raft log starts at index 1
        assert_eq!(cache.last_index, 0); // Empty log
    }

    #[test]
    fn test_append_entries() {
        let (storage, _temp_dir) = create_test_storage();

        let entry = Entry {
            index: 1,
            term: 1,
            data: b"test_data".to_vec().into(),
            ..Default::default()
        };

        storage.append_entries(std::slice::from_ref(&entry)).unwrap();

        let cache = storage.cache.read();
        assert_eq!(cache.first_index, 1);
        assert_eq!(cache.last_index, 1);
    }

    #[test]
    fn test_get_entries() {
        let (storage, _temp_dir) = create_test_storage();

        let mut entries = Vec::new();
        for i in 1..=5 {
            let entry = Entry {
                index: i,
                term: 1,
                data: format!("data_{}", i).into_bytes().into(),
                ..Default::default()
            };
            entries.push(entry);
        }

        storage.append_entries(&entries).unwrap();

        let retrieved = storage.get_entries(1, 6, None).unwrap();
        assert_eq!(retrieved.len(), 5);
        assert_eq!(retrieved[0].index, 1);
        assert_eq!(retrieved[4].index, 5);
    }

    #[test]
    fn test_compact() {
        let (storage, _temp_dir) = create_test_storage();

        let mut entries = Vec::new();
        for i in 1..=10 {
            let entry = Entry {
                index: i,
                term: 1,
                ..Default::default()
            };
            entries.push(entry);
        }

        storage.append_entries(&entries).unwrap();

        // Compact up to index 5
        storage.compact(5).unwrap();

        let cache = storage.cache.read();
        assert_eq!(cache.first_index, 5);
        assert_eq!(cache.last_index, 10);

        // Should not be able to get compacted entries
        assert!(storage.get_entries(1, 5, None).is_err());

        // Should be able to get entries after compaction point
        let retrieved = storage.get_entries(5, 11, None).unwrap();
        assert_eq!(retrieved.len(), 6);
    }
}
