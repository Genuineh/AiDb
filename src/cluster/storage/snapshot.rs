//! Raft snapshot build / install.

use std::io::Cursor;

use openraft::{BasicNode, LogId, Snapshot, SnapshotMeta, StoredMembership};
use serde::{Deserialize, Serialize};

use crate::cluster::meta_types::METARAFT_GROUP_ID;
use crate::cluster::storage::keys::{
    meta_range_end, meta_range_start, sm_range_end, sm_range_start, snapshot_meta_key,
    user_key_from_sm_key,
};
use crate::cluster::types::{NodeId, TypeConfig};
use crate::engine::db::WriteBatch;
use crate::error::{ClusterError, Error, Result};
use crate::DB;

use super::{db_to_storage_err, db_to_storage_write_err, OpenRaftStorage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SnapshotKv {
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
}

impl SnapshotKv {
    pub(crate) fn new(pairs: Vec<(Vec<u8>, Vec<u8>)>) -> Self {
        Self { pairs }
    }
}

pub struct OpenRaftSnapshotBuilder {
    db: std::sync::Arc<DB>,
    group_id: u64,
}

impl OpenRaftSnapshotBuilder {
    pub fn new(db: std::sync::Arc<DB>, group_id: u64) -> Self {
        Self { db, group_id }
    }

    pub(crate) fn scan_sm_pairs(db: &DB, group_id: u64) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        if group_id == METARAFT_GROUP_ID {
            return Self::scan_meta_pairs(db);
        }
        let start = sm_range_start(group_id);
        let end = sm_range_end(group_id);
        let iter = db.scan(Some(&start), Some(&end))?;
        let mut pairs = Vec::new();
        for item in iter {
            let (k, v) = item?;
            if let Some(user_key) = user_key_from_sm_key(group_id, &k) {
                pairs.push((user_key, v));
            }
        }
        Ok(pairs)
    }

    pub(crate) fn scan_meta_pairs(db: &DB) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let start = meta_range_start();
        let end = meta_range_end();
        let iter = db.scan(Some(&start), Some(&end))?;
        let mut pairs = Vec::new();
        for item in iter {
            let (k, v) = item?;
            pairs.push((k, v));
        }
        Ok(pairs)
    }
}

impl OpenRaftStorage {
    pub(crate) fn install_snapshot_atomic(
        &self,
        meta: &SnapshotMeta<NodeId, BasicNode>,
        data: &[u8],
    ) -> Result<()> {
        let snapshot: SnapshotKv = rmp_serde::from_slice(data)
            .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string())))?;

        let mut batch = WriteBatch::new();

        if self.group_id == METARAFT_GROUP_ID {
            self.db
                .delete_range(&meta_range_start(), &meta_range_end())?;
            for (key, value) in &snapshot.pairs {
                batch.put(key.clone(), value.clone());
            }
        } else {
            let sm_start = sm_range_start(self.group_id);
            let sm_end = sm_range_end(self.group_id);
            self.db.delete_range(&sm_start, &sm_end)?;
            for (key, value) in &snapshot.pairs {
                batch.put(super::keys::sm_key(self.group_id, key), value.clone());
            }
        }

        let meta_bytes = bincode::serialize(meta)
            .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string())))?;
        batch.put(snapshot_meta_key(self.group_id), meta_bytes);
        if let Some(log_id) = meta.last_log_id {
            let la = rmp_serde::to_vec(&log_id)
                .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string())))?;
            batch.put(super::keys::last_applied_key(self.group_id), la);
        }
        self.db.write(&batch)?;

        if let Some(ref meta_sm) = self.meta_state {
            meta_sm.reload_from_db()?;
        }

        let mut state = self.state.write();
        state.snapshot_meta = Some(meta.clone());
        state.last_applied = meta.last_log_id;
        if let Some(log_id) = meta.last_log_id {
            state.last_purged_log_id = Some(log_id);
        }
        Ok(())
    }
}

#[cfg(feature = "cluster")]
impl openraft::RaftSnapshotBuilder<TypeConfig> for OpenRaftSnapshotBuilder {
    async fn build_snapshot(
        &mut self,
    ) -> std::result::Result<Snapshot<TypeConfig>, openraft::StorageError<NodeId>> {
        let pairs = Self::scan_sm_pairs(&self.db, self.group_id).map_err(db_to_storage_err)?;
        let body = SnapshotKv::new(pairs);
        let data = rmp_serde::to_vec(&body).map_err(|e| {
            db_to_storage_write_err(Error::Cluster(ClusterError::Serialization(e.to_string())))
        })?;

        let membership: StoredMembership<NodeId, BasicNode> = self
            .db
            .get(&super::keys::membership_key(self.group_id))
            .map_err(db_to_storage_err)?
            .map(|d| bincode::deserialize(&d).unwrap_or_default())
            .unwrap_or_default();

        let last_applied: Option<LogId<NodeId>> = self
            .db
            .get(&super::keys::last_applied_key(self.group_id))
            .map_err(db_to_storage_err)?
            .and_then(|d| rmp_serde::from_slice(&d).ok());

        let snapshot_id = format!("snap-{}", last_applied.map(|id| id.index).unwrap_or(0));
        let meta = SnapshotMeta {
            last_log_id: last_applied,
            last_membership: membership,
            snapshot_id,
        };

        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(data)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::meta_state_machine::MetaStateMachine;
    use crate::cluster::meta_types::{MetaRequest, METARAFT_GROUP_ID};
    use crate::cluster::storage::keys::DEFAULT_GROUP_ID;
    use crate::cluster::types::ThinWriteBatch;
    use crate::config::Options;
    use crate::DB;
    use openraft::{CommittedLeaderId, LogId, SnapshotMeta, StoredMembership};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn test_snapshot_install() {
        let dir = TempDir::new().unwrap();
        let db = DB::open(dir.path(), Options::for_testing()).unwrap();
        let storage = OpenRaftStorage::new(db, DEFAULT_GROUP_ID, None).unwrap();

        let mut batch = ThinWriteBatch::new();
        batch.put(b"old".to_vec(), b"gone".to_vec());
        storage.apply_batch_to_sm(&batch).unwrap();

        let pairs = vec![(b"new".to_vec(), b"fresh".to_vec())];
        let data = rmp_serde::to_vec(&SnapshotKv::new(pairs)).unwrap();
        let log_id = LogId::new(CommittedLeaderId::new(2, 1), 5);
        let meta = SnapshotMeta {
            last_log_id: Some(log_id),
            last_membership: StoredMembership::default(),
            snapshot_id: "snap-5".into(),
        };
        storage.install_snapshot_atomic(&meta, &data).unwrap();

        assert_eq!(
            storage.get_state_machine_value(b"new").unwrap(),
            Some(b"fresh".to_vec())
        );
        assert!(storage.get_state_machine_value(b"old").unwrap().is_none());
        assert_eq!(
            storage
                .read_last_applied_from_db()
                .unwrap()
                .map(|id| id.index),
            Some(5)
        );
    }

    #[test]
    fn test_meta_snapshot_roundtrip() {
        let dir = TempDir::new().unwrap();
        let db = DB::open(dir.path(), Options::for_testing()).unwrap();
        let meta_sm = Arc::new(MetaStateMachine::new(db.clone()).unwrap());

        // Register a node via MetaStateMachine + persist ApplyOutput to DB
        let output = meta_sm
            .apply_meta_request(MetaRequest::RegisterNode {
                node_id: 1,
                rpc_addr: "http://127.0.0.1:1".into(),
                client_addr: None,
                tags: HashMap::new(),
            })
            .unwrap();
        {
            let mut wb = crate::engine::db::WriteBatch::new();
            for (k, v) in &output.kv_pairs {
                wb.put(k.clone(), v.clone());
            }
            db.write(&wb).unwrap();
        }

        // Build a snapshot from meta storage
        let pairs = OpenRaftSnapshotBuilder::scan_meta_pairs(&db).unwrap();
        assert!(
            !pairs.is_empty(),
            "meta snapshot must have at least the cluster_meta key"
        );

        let snapshot_kv = SnapshotKv::new(pairs.clone());
        let data = rmp_serde::to_vec(&snapshot_kv).unwrap();

        // Fresh DB + install snapshot
        let dir2 = TempDir::new().unwrap();
        let db2 = DB::open(dir2.path(), Options::for_testing()).unwrap();
        let meta_sm2 = Arc::new(MetaStateMachine::new(db2.clone()).unwrap());
        let storage2 =
            OpenRaftStorage::new(db2.clone(), METARAFT_GROUP_ID, Some(meta_sm2.clone())).unwrap();

        let log_id = LogId::new(CommittedLeaderId::new(1, 1), 3);
        let snap_meta = SnapshotMeta {
            last_log_id: Some(log_id),
            last_membership: StoredMembership::default(),
            snapshot_id: "meta-snap-3".into(),
        };
        storage2.install_snapshot_atomic(&snap_meta, &data).unwrap();

        // Verify state reloaded correctly
        let recovered = meta_sm2.get_cluster_meta();
        assert_eq!(recovered.nodes.len(), 1);
        assert!(recovered.nodes.contains_key(&1));
        assert_eq!(recovered.cluster_id, "uninitialized");
        assert!(meta_sm2.get_migration_state().is_none());
    }
}
