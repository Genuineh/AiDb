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
    /// 扫描 `[start, end)` 范围内现存的 key, 把对应的 `Delete` 操作追加进
    /// `batch` (不落盘) —— 供调用方把"清空旧范围"和"写入新数据"合并成
    /// 同一次 `db.write()`, 参见 `install_snapshot_atomic` 顶部的说明。
    fn append_delete_range_to_batch(
        db: &DB,
        batch: &mut WriteBatch,
        start: &[u8],
        end: &[u8],
    ) -> Result<()> {
        if start >= end {
            return Ok(());
        }
        let iter = db.scan(Some(start), Some(end))?;
        for item in iter {
            let (k, _) = item?;
            batch.delete(k);
        }
        Ok(())
    }

    pub(crate) fn install_snapshot_atomic(
        &self,
        meta: &SnapshotMeta<NodeId, BasicNode>,
        data: &[u8],
    ) -> Result<()> {
        let snapshot: SnapshotKv = rmp_serde::from_slice(data)
            .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string())))?;

        let mut batch = WriteBatch::new();

        // 旧范围的删除必须和新数据的写入合入*同一个* WriteBatch, 才能保证
        // 一次 db.write() 的原子性 —— 之前 delete_range() 是单独一次
        // db.write() (它内部自己 scan + batch delete), 和下面新数据的
        // db.write() 是两次独立的写. 中途 (delete 已落盘, 新数据还没写)
        // 崩溃重启, 状态机会被永久留空: openraft 认为 snapshot 已装好
        // (last_applied 会指向 snapshot 的 log_id), 但实际数据一条都没有,
        // 且触发这次 snapshot 的日志很可能已经被 purge, 无法重新 apply 找回。
        // 这里改成先把待删除的 key 收集进同一个 batch, 和新 pairs 一起
        // 只 write 一次: 崩溃时要么全部还是旧数据, 要么全部是新数据。
        if self.group_id == METARAFT_GROUP_ID {
            Self::append_delete_range_to_batch(
                &self.db,
                &mut batch,
                &meta_range_start(),
                &meta_range_end(),
            )?;
            for (key, value) in &snapshot.pairs {
                batch.put(key.clone(), value.clone());
            }
        } else {
            let sm_start = sm_range_start(self.group_id);
            let sm_end = sm_range_end(self.group_id);
            Self::append_delete_range_to_batch(&self.db, &mut batch, &sm_start, &sm_end)?;
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

    /// 崩溃注入回归测试: `install_snapshot_atomic` 必须把"删旧范围"和"写新数据"
    /// 合并进同一个 WriteBatch, 才能靠 WAL 的 batch 级原子回放保证 —— 崩溃发生在
    /// 这次写入落盘过程中的任意时刻, 重启后要么完全看不到这次写入 (回滚到旧状态),
    /// 要么完全看到新状态, 绝不会出现"旧数据已删、新数据没写"的空洞。
    ///
    /// 用截断 WAL 尾部字节模拟"写入中途断电/进程被杀", 这是本仓库现有崩溃测试
    /// (`tests/modules/db/wal_corruption.rs`) 采用的标准手法: 一次成功写入后
    /// 截掉尾部若干字节, 相当于验证"如果这次写只有部分字节落盘会怎样"。
    #[test]
    fn test_install_snapshot_atomic_truncated_mid_write_never_leaves_hole() {
        fn find_latest_wal(dir: &std::path::Path) -> Option<std::path::PathBuf> {
            std::fs::read_dir(dir)
                .ok()?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("wal_") && n.ends_with(".log"))
                })
                .max_by_key(|e| {
                    e.path()
                        .file_name()
                        .and_then(|n| n.to_str())
                        .and_then(|n| n.strip_prefix("wal_"))
                        .and_then(|n| n.strip_suffix(".log"))
                        .and_then(|n| n.parse::<u64>().ok())
                        .unwrap_or(0)
                })
                .map(|e| e.path())
        }

        let dir = TempDir::new().unwrap();
        let mut opts = Options::for_testing();
        opts.memtable_size = 64 * 1024 * 1024; // 足够大, 不触发 flush/rotate
        opts.sync_wal = true;

        {
            let db = DB::open(dir.path(), opts.clone()).unwrap();
            let storage = OpenRaftStorage::new(db, DEFAULT_GROUP_ID, None).unwrap();

            // 旧状态: snapshot 安装前 state machine 里已有的数据.
            let mut old_batch = ThinWriteBatch::new();
            old_batch.put(b"old1".to_vec(), b"v1".to_vec());
            old_batch.put(b"old2".to_vec(), b"v2".to_vec());
            storage.apply_batch_to_sm(&old_batch).unwrap();

            // 新 snapshot: old1 被覆盖新值, old2 被删除 (不在新 pairs 里), 新增 new1.
            let pairs = vec![
                (b"old1".to_vec(), b"newv1".to_vec()),
                (b"new1".to_vec(), b"fresh".to_vec()),
            ];
            let data = rmp_serde::to_vec(&SnapshotKv::new(pairs)).unwrap();
            let log_id = LogId::new(CommittedLeaderId::new(2, 1), 5);
            let meta = SnapshotMeta {
                last_log_id: Some(log_id),
                last_membership: StoredMembership::default(),
                snapshot_id: "snap-5".into(),
            };
            storage.install_snapshot_atomic(&meta, &data).unwrap();

            // 不 close —— 模拟进程崩溃, WAL 文件原样保留在磁盘上.
        }

        // 截断 WAL 尾部, 模拟"这次 write() 只有部分字节真正落盘"的崩溃场景.
        let wal = find_latest_wal(dir.path()).expect("WAL file must exist after crash-drop");
        let bytes = std::fs::read(&wal).unwrap();
        assert!(bytes.len() > 40, "WAL must contain the snapshot batch");
        let truncated_len = bytes.len() - 24;
        std::fs::write(&wal, &bytes[..truncated_len]).unwrap();

        // 重新打开: 不 panic, 状态机必须处于"完全旧" 或"完全新"两个一致状态之一,
        // 绝不能是二者的混合 (old2 被删了但 new1 没写进来这种空洞).
        let db2 = DB::open(dir.path(), opts).unwrap();
        let storage2 = OpenRaftStorage::new(db2, DEFAULT_GROUP_ID, None).unwrap();

        let old1 = storage2.get_state_machine_value(b"old1").unwrap();
        let old2 = storage2.get_state_machine_value(b"old2").unwrap();
        let new1 = storage2.get_state_machine_value(b"new1").unwrap();

        let fully_old =
            old1 == Some(b"v1".to_vec()) && old2 == Some(b"v2".to_vec()) && new1.is_none();
        let fully_new =
            old1 == Some(b"newv1".to_vec()) && old2.is_none() && new1 == Some(b"fresh".to_vec());

        assert!(
            fully_old || fully_new,
            "snapshot install must be all-or-nothing across a crash, got old1={old1:?} old2={old2:?} new1={new1:?}"
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
