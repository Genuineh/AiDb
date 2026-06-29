//! Raft log persistence (vote + log entries).

use std::ops::{Bound, RangeBounds};

use openraft::{Entry, LogId, Vote};

use crate::cluster::storage::keys::{
    last_applied_key, log_key, log_prefix, log_range_end, membership_key, snapshot_meta_key,
    snapshot_temp_global_end, snapshot_temp_global_start, vote_key,
};
use crate::cluster::types::{NodeId, TypeConfig};
use crate::error::{ClusterError, Error, Result};

use super::OpenRaftStorage;

impl OpenRaftStorage {
    pub(crate) fn save_vote_internal(&self, vote: &Vote<NodeId>) -> Result<()> {
        let data =
            rmp_serde::to_vec(vote).map_err(|e| ClusterError::Serialization(e.to_string()))?;
        self.db.put(&vote_key(self.group_id), &data)?;
        self.state.write().vote = Some(*vote);
        Ok(())
    }

    pub(crate) fn get_log_entries(
        &self,
        range: impl RangeBounds<u64>,
    ) -> Result<Vec<Entry<TypeConfig>>> {
        let state = self.state.read();
        let start = match range.start_bound() {
            Bound::Included(&x) => x,
            Bound::Excluded(&x) => x.saturating_add(1),
            Bound::Unbounded => state.last_purged_log_id.map(|id| id.index + 1).unwrap_or(0),
        };
        let end = match range.end_bound() {
            Bound::Included(&x) => x.saturating_add(1),
            Bound::Excluded(&x) => x,
            Bound::Unbounded => state.last_log_id.map(|id| id.index + 1).unwrap_or(0),
        };
        drop(state);

        if start >= end {
            return Ok(Vec::new());
        }

        let prefix = log_prefix(self.group_id);
        let end_key = log_range_end(self.group_id);
        let iter = self.db.scan(Some(&prefix), Some(&end_key))?;
        let mut entries = Vec::new();
        for item in iter {
            let (key, data) = item?;
            if key.len() < prefix.len() + 8 {
                continue;
            }
            let idx = u64::from_be_bytes(key[key.len() - 8..].try_into().unwrap());
            if idx < start || idx >= end {
                continue;
            }
            let entry: Entry<TypeConfig> = rmp_serde::from_slice(&data)
                .map_err(|e| ClusterError::Serialization(e.to_string()))?;
            entries.push(entry);
        }
        entries.sort_by_key(|e| e.log_id.index);
        Ok(entries)
    }

    pub(crate) fn append_log_entries(&self, entries: &[Entry<TypeConfig>]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let t0 = std::time::Instant::now();
        use crate::engine::db::WriteBatch;
        let mut batch = WriteBatch::new();
        for entry in entries {
            let key = log_key(self.group_id, entry.log_id.index);
            let data =
                rmp_serde::to_vec(entry).map_err(|e| ClusterError::Serialization(e.to_string()))?;
            batch.put(key, data);
        }
        self.db.write(&batch)?;
        let mut state = self.state.write();
        if let Some(last) = entries.last() {
            state.last_log_id = Some(last.log_id);
        }
        tracing::info!(target: "perf", group_id = self.group_id, entry_count = entries.len(), ms = t0.elapsed().as_millis(), "raft_append_log");
        Ok(())
    }

    pub(crate) fn delete_logs_from(&self, log_id: LogId<NodeId>) -> Result<()> {
        let last_index = self
            .state
            .read()
            .last_log_id
            .map(|id| id.index)
            .unwrap_or(0);
        if log_id.index > last_index {
            return Ok(());
        }
        let start = log_key(self.group_id, log_id.index);
        let end = log_range_end(self.group_id);
        self.db.delete_range(&start, &end)?;

        let mut state = self.state.write();
        if log_id.index == 0 {
            state.last_log_id = None;
        } else {
            let prev = log_key(self.group_id, log_id.index - 1);
            state.last_log_id = self
                .db
                .get(&prev)?
                .and_then(|data| rmp_serde::from_slice(&data).ok())
                .map(|e: Entry<TypeConfig>| e.log_id);
        }
        Ok(())
    }

    pub(crate) fn purge_logs_upto_internal(&self, log_id: LogId<NodeId>) -> Result<()> {
        let start_index = self
            .state
            .read()
            .last_purged_log_id
            .map(|id| id.index + 1)
            .unwrap_or(0);
        if log_id.index < start_index {
            return Ok(());
        }
        for idx in start_index..=log_id.index {
            self.db.delete(&log_key(self.group_id, idx))?;
        }
        self.state.write().last_purged_log_id = Some(log_id);
        Ok(())
    }

    pub(crate) fn load_state(&self) -> Result<()> {
        let mut state = self.state.write();
        let gid = self.group_id;

        tracing::debug!(group_id = gid, "load_state: loading Raft state from DB");

        if let Some(data) = self.db.get(&vote_key(gid))? {
            state.vote = Some(
                rmp_serde::from_slice(&data)
                    .map_err(|e| ClusterError::Serialization(e.to_string()))?,
            );
        }

        if let Some(data) = self.db.get(&last_applied_key(gid))? {
            state.last_applied = Some(
                rmp_serde::from_slice(&data)
                    .map_err(|e| ClusterError::Serialization(e.to_string()))?,
            );
        }

        if let Some(data) = self.db.get(&snapshot_meta_key(gid))? {
            state.snapshot_meta = Some(
                bincode::deserialize(&data)
                    .map_err(|e| ClusterError::Serialization(e.to_string()))?,
            );
        }

        let prefix = log_prefix(gid);
        let end = log_range_end(gid);
        let mut max_index: Option<u64> = None;
        if let Ok(iter) = self.db.scan(Some(&prefix), Some(&end)) {
            for item in iter {
                let (key, _) = item?;
                if key.len() >= prefix.len() + 8 {
                    let idx = u64::from_be_bytes(key[key.len() - 8..].try_into().unwrap());
                    max_index = Some(max_index.map_or(idx, |m| m.max(idx)));
                }
            }
        }
        // Reconstruct last_log_id from the actual log entry to preserve the
        // correct CommittedLeaderId (term, node_id).  Using (0,0) causes the
        // follower's log state to appear as if it belongs to a different leader,
        // which can break replication after learner catch-up.
        if let Some(max_idx) = max_index {
            let leader_id = match self.db.get(&log_key(gid, max_idx)) {
                Ok(Some(data)) => match rmp_serde::from_slice::<Entry<TypeConfig>>(&data) {
                    Ok(entry) => entry.log_id.leader_id,
                    Err(_) => openraft::CommittedLeaderId::new(0, 0),
                },
                _ => openraft::CommittedLeaderId::new(0, 0),
            };
            state.last_log_id = Some(openraft::LogId::new(leader_id, max_idx));
        }

        tracing::debug!(
            group_id = gid,
            last_log_id_term = state.last_log_id.as_ref().map(|id| id.leader_id.term),
            last_log_id_node = state.last_log_id.as_ref().map(|id| id.leader_id.node_id),
            last_log_id_index = state.last_log_id.as_ref().map(|id| id.index),
            last_applied_index = state.last_applied.as_ref().map(|id| id.index),
            "load_state: state loaded",
        );

        let _ = self
            .db
            .delete_range(snapshot_temp_global_start(), snapshot_temp_global_end());

        // Clean up any leftover snapshot temp files from previous crash
        let temp_path = self.db.path().join(format!(".snapshot_temp_{}", gid));
        let _ = std::fs::remove_file(&temp_path);

        if self.db.get(&membership_key(gid))?.is_none() {
            if let Some(last_idx) = state.last_applied.as_ref().map(|id| id.index).or(max_index) {
                for idx in (1..=last_idx).rev() {
                    if let Some(data) = self.db.get(&log_key(gid, idx))? {
                        if let Ok(entry) = rmp_serde::from_slice::<Entry<TypeConfig>>(&data) {
                            if let openraft::EntryPayload::Membership(ref m) = entry.payload {
                                let stored =
                                    openraft::StoredMembership::new(Some(entry.log_id), m.clone());
                                let mem_data = bincode::serialize(&stored)
                                    .map_err(|e| ClusterError::Serialization(e.to_string()))?;
                                self.db.put(&membership_key(gid), &mem_data)?;
                                break;
                            }
                        }
                    }
                }
            }
        }

        if max_index.is_some() && state.last_purged_log_id.is_none() {
            let mut first: Option<u64> = None;
            if let Ok(iter) = self.db.scan(Some(&prefix), Some(&end)) {
                for item in iter {
                    let (key, _) = item?;
                    if key.len() >= prefix.len() + 8 {
                        let idx = u64::from_be_bytes(key[key.len() - 8..].try_into().unwrap());
                        first = Some(first.map_or(idx, |f| f.min(idx)));
                    }
                }
            }
            if let Some(first_idx) = first {
                if first_idx > 0 {
                    // Use the entry at first_idx to get the correct leader for the purged region.
                    let leader_id = match self.db.get(&log_key(gid, first_idx)) {
                        Ok(Some(data)) => match rmp_serde::from_slice::<Entry<TypeConfig>>(&data) {
                            Ok(entry) => entry.log_id.leader_id,
                            Err(_) => openraft::CommittedLeaderId::new(0, 0),
                        },
                        _ => openraft::CommittedLeaderId::new(0, 0),
                    };
                    state.last_purged_log_id = Some(openraft::LogId::new(leader_id, first_idx - 1));
                }
            }
        }

        Ok(())
    }

    pub(crate) fn read_last_applied_from_db(&self) -> Result<Option<LogId<NodeId>>> {
        match self.db.get(&last_applied_key(self.group_id))? {
            None => Ok(None),
            Some(bytes) => Ok(Some(
                rmp_serde::from_slice(&bytes)
                    .map_err(|e| ClusterError::Serialization(e.to_string()))?,
            )),
        }
    }

    pub(crate) fn load_membership(
        &self,
    ) -> std::result::Result<openraft::StoredMembership<NodeId, openraft::BasicNode>, Error> {
        match self.db.get(&membership_key(self.group_id))? {
            Some(data) => bincode::deserialize(&data)
                .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string()))),
            None => Ok(openraft::StoredMembership::default()),
        }
    }
}

pub(crate) fn map_db_err(e: Error) -> ClusterError {
    match e {
        Error::Io(io) => ClusterError::Io(io),
        other => ClusterError::Internal(other.to_string()),
    }
}

pub(crate) fn db_to_storage_err(e: Error) -> openraft::StorageError<NodeId> {
    openraft::StorageError::IO {
        source: openraft::StorageIOError::read(openraft::AnyError::error(e.to_string())),
    }
}

pub(crate) fn db_to_storage_write_err(e: Error) -> openraft::StorageError<NodeId> {
    openraft::StorageError::IO {
        source: openraft::StorageIOError::write(openraft::AnyError::error(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::storage::keys::{log_key, vote_key, DEFAULT_GROUP_ID};
    use crate::cluster::types::{Request, TypeConfig};
    use crate::config::Options;
    use crate::DB;
    use openraft::{CommittedLeaderId, Entry, EntryPayload, LogId, Vote};
    use tempfile::TempDir;

    fn put_entry(index: u64, term: u64) -> Entry<TypeConfig> {
        Entry {
            log_id: LogId::new(CommittedLeaderId::new(term, 1), index),
            payload: EntryPayload::Normal(Request::Put {
                key: format!("k{index}").into_bytes(),
                value: format!("v{index}").into_bytes(),
            }),
        }
    }

    fn open_storage(dir: &TempDir) -> OpenRaftStorage {
        let db = DB::open(dir.path(), Options::for_testing()).unwrap();
        OpenRaftStorage::new(db, DEFAULT_GROUP_ID, None).unwrap()
    }

    #[test]
    fn test_save_read_vote() {
        let dir = TempDir::new().unwrap();
        let storage = open_storage(&dir);
        let vote = Vote {
            leader_id: openraft::LeaderId::new(2, 1),
            committed: true,
        };
        storage.save_vote_internal(&vote).unwrap();
        assert_eq!(storage.state.read().vote, Some(vote));
        assert!(storage
            .db
            .get(&vote_key(DEFAULT_GROUP_ID))
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_append_and_read_log() {
        let dir = TempDir::new().unwrap();
        let storage = open_storage(&dir);
        let entries = vec![put_entry(1, 1), put_entry(2, 1)];
        storage.append_log_entries(&entries).unwrap();
        let read = storage.get_log_entries(1..=2).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].log_id.index, 1);
        assert_eq!(read[1].log_id.index, 2);
        assert!(storage
            .db
            .get(&log_key(DEFAULT_GROUP_ID, 1))
            .unwrap()
            .is_some());
    }

    #[test]
    fn test_delete_logs_from() {
        let dir = TempDir::new().unwrap();
        let storage = open_storage(&dir);
        storage
            .append_log_entries(&[put_entry(1, 1), put_entry(2, 1), put_entry(3, 1)])
            .unwrap();
        let from = LogId::new(CommittedLeaderId::new(1, 1), 2);
        storage.delete_logs_from(from).unwrap();
        assert_eq!(storage.get_log_entries(1..=3).unwrap().len(), 1);
        assert_eq!(storage.state.read().last_log_id.map(|id| id.index), Some(1));
    }

    #[test]
    fn test_storage_restart_recovery() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        {
            let storage = open_storage(&dir);
            let vote = Vote {
                leader_id: openraft::LeaderId::new(3, 2),
                committed: true,
            };
            storage.save_vote_internal(&vote).unwrap();
            storage
                .append_log_entries(&[put_entry(1, 3), put_entry(2, 3)])
                .unwrap();
        }
        let db = DB::open(&path, Options::for_testing()).unwrap();
        let storage = OpenRaftStorage::new(db, DEFAULT_GROUP_ID, None).unwrap();
        assert_eq!(
            storage.state.read().vote,
            Some(Vote {
                leader_id: openraft::LeaderId::new(3, 2),
                committed: true,
            })
        );
        let logs = storage.get_log_entries(1..=2).unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].log_id.index, 1);
    }
}
