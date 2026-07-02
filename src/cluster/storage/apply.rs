//! State machine apply path.

use openraft::{Entry, EntryPayload, LogId, MessageSummary};

use crate::cluster::meta_types::{MetaRequest, METARAFT_GROUP_ID};
use crate::cluster::storage::keys::{last_applied_key, membership_key, sm_key};
use crate::cluster::types::{
    ClusterError, Request, Response, ThinWriteBatch, ThinWriteOp, TypeConfig,
};
use crate::engine::db::WriteBatch;
use crate::error::{Error, Result};

use super::OpenRaftStorage;
use crate::cluster::storage::log::map_db_err;

impl OpenRaftStorage {
    #[cfg(test)]
    pub(crate) fn apply_batch_to_sm(&self, batch: &ThinWriteBatch) -> Result<()> {
        let mut db_batch = WriteBatch::new();
        self.append_thin_batch_to_db_batch(&mut db_batch, batch);
        if db_batch.is_empty() {
            return Ok(());
        }
        self.db
            .write(&db_batch)
            .map_err(|e| Error::Cluster(map_db_err(e)))?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn apply_put_conditional(&self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
        let mut batch = WriteBatch::new();
        self.append_put_conditional_to_batch(&mut batch, &key, &value)?;
        if batch.is_empty() {
            return Ok(());
        }
        self.db
            .write(&batch)
            .map_err(|e| Error::Cluster(map_db_err(e)))?;
        Ok(())
    }

    pub(crate) fn apply_entries_internal(
        &self,
        entries: &[Entry<TypeConfig>],
    ) -> Result<Vec<Response>> {
        let mut last_applied = self.read_last_applied_from_db()?;
        let mut responses = Vec::with_capacity(entries.len());

        tracing::debug!(
            group_id = self.group_id,
            entry_count = entries.len(),
            last_applied_index = last_applied.as_ref().map(|id| id.index),
            "apply_entries_internal: applying batch",
        );

        let mut i = 0;
        while i < entries.len() {
            let entry = &entries[i];

            // 跳过已 apply 的 entry
            if let Some(ref applied) = last_applied {
                if entry.log_id.index <= applied.index {
                    tracing::trace!(index = entry.log_id.index, "skipping already-applied entry",);
                    responses.push(Response::Ok);
                    i += 1;
                    continue;
                }
            }

            tracing::debug!(
              index = entry.log_id.index,
              term = entry.log_id.leader_id.term,
              payload = %entry.payload.summary(),
              "applying entry",
            );

            match &entry.payload {
                EntryPayload::Normal(Request::Meta(meta_req))
                    if self.group_id == METARAFT_GROUP_ID && self.meta_state.is_some() =>
                {
                    let response = self.apply_meta_entry(meta_req, &entry.log_id)?;
                    last_applied = Some(entry.log_id);
                    responses.push(response);
                    i += 1;
                }
                EntryPayload::Normal(_request) => {
                    // 将连续的 data entry 批量合并到一个 WriteBatch.
                    //
                    // append_request_to_batch 目前唯一可能返回 Err 的路径是
                    // PutConditional 内部 self.db.get() 的磁盘 I/O 读取失败 —
                    // 这是基础设施故障, 不是业务结果 (Put/Delete/WriteBatch 三个
                    // 分支永远不失败; PutConditional "条件不满足" 本身走的是
                    // Ok(()) 分支, 不是 Err). 因此这里的错误必须和本文件里其它
                    // DB 写操作一样直接 `?` 向上抛, 交给 openraft 判定为该 raft
                    // 实例的致命 StorageError 并停止服务 (对齐 etcd/TiKV: apply
                    // 对已提交 entry 必须是确定性的, 不能把一次真实的存储故障
                    // 悄悄当成"这条 entry 失败了但继续处理下一条", 否则
                    // last_applied 会越过这条 entry 被持久化, 数据永久丢失且
                    // 不同副本可能因为存储故障的偶然性而分叉.
                    let t0 = std::time::Instant::now();
                    let mut batch = WriteBatch::new();
                    let mut batched_responses: Vec<Response> = Vec::new();
                    let batch_start = i;
                    let mut last_log_id = entry.log_id;

                    while i < entries.len() {
                        let e = &entries[i];
                        if let Some(ref applied) = last_applied {
                            if e.log_id.index <= applied.index {
                                break; // 已 skip 的不纳入当前批
                            }
                        }
                        match &e.payload {
                            EntryPayload::Normal(Request::Meta(_)) => break,
                            EntryPayload::Membership(_) => break,
                            EntryPayload::Blank => break,
                            EntryPayload::Normal(req) => {
                                let response = self.append_request_to_batch(&mut batch, req)?;
                                batched_responses.push(response);
                                last_log_id = e.log_id;
                                i += 1;
                            }
                        }
                    }

                    if !batch.is_empty() {
                        batch.put(
                            last_applied_key(self.group_id),
                            Self::serialize_last_applied(&last_log_id)?,
                        );
                        self.db
                            .write(&batch)
                            .map_err(|e| Error::Cluster(map_db_err(e)))?;
                        self.state.write().last_applied = Some(last_log_id);

                        let count = batched_responses.len();
                        tracing::info!(
                            target: "perf",
                            group_id = self.group_id,
                            batch_size = count,
                            from_index = entries[batch_start].log_id.index,
                            to_index = last_log_id.index,
                            ms = t0.elapsed().as_millis(),
                            "raft_apply_batch",
                        );

                        responses.extend(batched_responses);
                        last_applied = Some(last_log_id);
                    } else {
                        // 所有 entry 都是 PutConditional 跳过: 只需写 last_applied
                        self.persist_last_applied_atomic(&last_log_id)?;
                        responses.extend(batched_responses);
                        last_applied = Some(last_log_id);
                    }
                }
                EntryPayload::Membership(m) => {
                    let response = self.apply_membership_entry_atomic(m, &entry.log_id)?;
                    last_applied = Some(entry.log_id);
                    responses.push(response);
                    i += 1;
                }
                EntryPayload::Blank => {
                    self.persist_last_applied_atomic(&entry.log_id)?;
                    responses.push(Response::Ok);
                    last_applied = Some(entry.log_id);
                    i += 1;
                }
            }
        }

        Ok(responses)
    }

    fn append_thin_batch_to_db_batch(&self, batch: &mut WriteBatch, thin: &ThinWriteBatch) {
        for op in &thin.ops {
            match op {
                ThinWriteOp::Put { key, value } => {
                    batch.put(sm_key(self.group_id, key), value.clone());
                }
                ThinWriteOp::Delete { key } => {
                    batch.delete(sm_key(self.group_id, key));
                }
            }
        }
    }

    fn append_put_conditional_to_batch(
        &self,
        batch: &mut WriteBatch,
        key: &[u8],
        value: &[u8],
    ) -> Result<()> {
        let sm = sm_key(self.group_id, key);
        if self.db.get(&sm)?.is_some() {
            return Ok(());
        }
        batch.put(sm, value.to_vec());
        Ok(())
    }

    fn append_request_to_batch(
        &self,
        batch: &mut WriteBatch,
        request: &Request,
    ) -> Result<Response> {
        match request {
            Request::Meta(_) => Ok(Response::Error("meta request on non-meta storage".into())),
            Request::PutConditional { key, value } => {
                self.append_put_conditional_to_batch(batch, key, value)?;
                Ok(Response::Ok)
            }
            Request::Put { key, value } => {
                batch.put(sm_key(self.group_id, key), value.clone());
                Ok(Response::Ok)
            }
            Request::Delete { key } => {
                batch.delete(sm_key(self.group_id, key));
                Ok(Response::Ok)
            }
            Request::WriteBatch(wb) => {
                self.append_thin_batch_to_db_batch(batch, wb);
                Ok(Response::Ok)
            }
        }
    }

    fn serialize_last_applied(log_id: &LogId<crate::cluster::types::NodeId>) -> Result<Vec<u8>> {
        rmp_serde::to_vec(log_id)
            .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string())))
    }

    fn persist_last_applied_atomic(
        &self,
        log_id: &LogId<crate::cluster::types::NodeId>,
    ) -> Result<()> {
        let mut batch = WriteBatch::new();
        batch.put(
            last_applied_key(self.group_id),
            Self::serialize_last_applied(log_id)?,
        );
        self.db
            .write(&batch)
            .map_err(|e| Error::Cluster(map_db_err(e)))?;
        self.state.write().last_applied = Some(*log_id);
        Ok(())
    }

    fn apply_membership_entry_atomic(
        &self,
        membership: &openraft::Membership<crate::cluster::types::NodeId, openraft::BasicNode>,
        log_id: &LogId<crate::cluster::types::NodeId>,
    ) -> Result<Response> {
        let stored = openraft::StoredMembership::new(Some(*log_id), membership.clone());
        let data = bincode::serialize(&stored)
            .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string())))?;
        let mut batch = WriteBatch::new();
        batch.put(membership_key(self.group_id), data);
        batch.put(
            last_applied_key(self.group_id),
            Self::serialize_last_applied(log_id)?,
        );
        self.db
            .write(&batch)
            .map_err(|e| Error::Cluster(map_db_err(e)))?;
        self.state.write().last_applied = Some(*log_id);
        Ok(Response::Ok)
    }

    fn apply_meta_entry(
        &self,
        req: &MetaRequest,
        log_id: &LogId<crate::cluster::types::NodeId>,
    ) -> Result<Response> {
        let meta_state = self.meta_state.as_ref().expect("meta_state required");
        let mut batch = WriteBatch::new();
        let response = match meta_state.apply_meta_request(req.clone()) {
            Ok(output) => {
                for (key, value) in output.kv_pairs {
                    batch.put(key, value);
                }
                Response::Ok
            }
            Err(e) => Response::Error(e.to_string()),
        };
        batch.put(
            last_applied_key(self.group_id),
            Self::serialize_last_applied(log_id)?,
        );
        self.db
            .write(&batch)
            .map_err(|e| Error::Cluster(map_db_err(e)))?;
        self.state.write().last_applied = Some(*log_id);
        Ok(response)
    }

    pub fn get_state_machine_value(&self, user_key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db
            .get(&sm_key(self.group_id, user_key))
            .map_err(|e| Error::Cluster(map_db_err(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::meta_state_machine::MetaStateMachine;
    use crate::cluster::meta_types::MetaRequest;
    use crate::cluster::storage::keys::{meta_cluster_meta_key, sm_key, DEFAULT_GROUP_ID};
    use crate::config::Options;
    use crate::DB;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_storage() -> (OpenRaftStorage, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = DB::open(dir.path(), Options::for_testing()).unwrap();
        (
            OpenRaftStorage::new(db, DEFAULT_GROUP_ID, None).unwrap(),
            dir,
        )
    }

    fn meta_storage() -> (OpenRaftStorage, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = DB::open(dir.path(), Options::for_testing()).unwrap();
        let meta = Arc::new(MetaStateMachine::new(db.clone()).unwrap());
        (
            OpenRaftStorage::new(db, METARAFT_GROUP_ID, Some(meta)).unwrap(),
            dir,
        )
    }

    #[test]
    fn test_apply_put_delete() {
        let (storage, _dir) = test_storage();
        let mut batch = ThinWriteBatch::new();
        batch.put(b"k1".to_vec(), b"v1".to_vec());
        storage.apply_batch_to_sm(&batch).unwrap();
        assert_eq!(
            storage.get_state_machine_value(b"k1").unwrap(),
            Some(b"v1".to_vec())
        );
        let mut del = ThinWriteBatch::new();
        del.delete(b"k1".to_vec());
        storage.apply_batch_to_sm(&del).unwrap();
        assert!(storage.get_state_machine_value(b"k1").unwrap().is_none());
    }

    #[test]
    fn test_request_put_conditional() {
        let (storage, _dir) = test_storage();
        storage
            .apply_put_conditional(b"k".to_vec(), b"v1".to_vec())
            .unwrap();
        storage
            .apply_put_conditional(b"k".to_vec(), b"v2".to_vec())
            .unwrap();
        assert_eq!(
            storage.get_state_machine_value(b"k").unwrap(),
            Some(b"v1".to_vec())
        );
        assert_eq!(
            storage.db.get(&sm_key(DEFAULT_GROUP_ID, b"k")).unwrap(),
            Some(b"v1".to_vec())
        );
    }

    #[test]
    fn test_append_request_to_batch_includes_sm_only() {
        let (storage, _dir) = test_storage();
        let mut batch = WriteBatch::new();
        storage
            .append_request_to_batch(
                &mut batch,
                &Request::Put {
                    key: b"k".to_vec(),
                    value: b"v".to_vec(),
                },
            )
            .unwrap();
        assert_eq!(batch.len(), 1);
        assert!(batch
            .operations
            .iter()
            .all(|op| matches!(op, crate::engine::db::WriteOp::Put { .. })));
    }

    #[test]
    fn test_apply_idempotent_last_applied() {
        let (storage, _dir) = test_storage();
        use openraft::{CommittedLeaderId, Entry, EntryPayload, LogId};
        let entry = Entry::<TypeConfig> {
            log_id: LogId::new(CommittedLeaderId::new(1, 1), 1),
            payload: EntryPayload::Normal(Request::Put {
                key: b"k".to_vec(),
                value: b"v".to_vec(),
            }),
        };
        storage
            .apply_entries_internal(std::slice::from_ref(&entry))
            .unwrap();
        storage
            .apply_entries_internal(std::slice::from_ref(&entry))
            .unwrap();
        assert_eq!(
            storage.get_state_machine_value(b"k").unwrap(),
            Some(b"v".to_vec())
        );
        assert_eq!(
            storage
                .read_last_applied_from_db()
                .unwrap()
                .map(|id| id.index),
            Some(1)
        );
    }

    #[test]
    fn test_meta_storage_apply_output_integration() {
        let (storage, _dir) = meta_storage();
        use openraft::{CommittedLeaderId, Entry, EntryPayload, LogId};
        let entry = Entry::<TypeConfig> {
            log_id: LogId::new(CommittedLeaderId::new(1, 1), 1),
            payload: EntryPayload::Normal(Request::Meta(MetaRequest::RegisterNode {
                node_id: 1,
                rpc_addr: "http://127.0.0.1:1".into(),
                client_addr: None,
                tags: HashMap::new(),
            })),
        };
        storage.apply_entries_internal(&[entry]).unwrap();
        assert!(storage.db.get(&meta_cluster_meta_key()).unwrap().is_some());
        assert_eq!(
            storage
                .read_last_applied_from_db()
                .unwrap()
                .map(|id| id.index),
            Some(1)
        );
    }

    /// 回归测试: apply 过程中真实的存储故障必须直接向上抛错, 绝不能把
    /// last_applied 悄悄推进到失败的 entry —— 这正是修复前的 P0 bug
    /// (last_applied 越过失败 entry, 数据永久丢失且副本间可能分叉).
    #[test]
    fn test_apply_storage_error_does_not_advance_last_applied() {
        use openraft::{CommittedLeaderId, Entry, EntryPayload, LogId};

        let dir = TempDir::new().unwrap();
        let db = DB::open(dir.path(), Options::for_testing()).unwrap();
        let storage = OpenRaftStorage::new(db.clone(), DEFAULT_GROUP_ID, None).unwrap();

        // Entry #1 applies cleanly.
        let e1 = Entry::<TypeConfig> {
            log_id: LogId::new(CommittedLeaderId::new(1, 1), 1),
            payload: EntryPayload::Normal(Request::Put {
                key: b"k1".to_vec(),
                value: b"v1".to_vec(),
            }),
        };
        storage.apply_entries_internal(std::slice::from_ref(&e1)).unwrap();
        assert_eq!(
            storage.read_last_applied_from_db().unwrap().map(|id| id.index),
            Some(1)
        );

        // Force a genuine storage I/O failure on the *next* entry: closing the
        // DB makes any further `db.get()`/`db.write()` return Err, simulating
        // e.g. a disk fault hit by PutConditional's dedup read.
        db.close().unwrap();

        let e2 = Entry::<TypeConfig> {
            log_id: LogId::new(CommittedLeaderId::new(1, 1), 2),
            payload: EntryPayload::Normal(Request::PutConditional {
                key: b"k2".to_vec(),
                value: b"v2".to_vec(),
            }),
        };
        let result = storage.apply_entries_internal(std::slice::from_ref(&e2));
        assert!(
            result.is_err(),
            "a genuine storage error must propagate as Err, not be swallowed as a per-entry business Response::Error"
        );

        // In-memory last_applied must still be at index 1 — the failed entry
        // #2 was never marked as applied, in memory or (transitively) on disk.
        assert_eq!(
            storage.state.read().last_applied.map(|id| id.index),
            Some(1),
            "last_applied must not advance past the entry that failed to apply"
        );
    }

    #[test]
    fn test_membership_persist() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_path_buf();
        {
            let db = DB::open(&path, Options::for_testing()).unwrap();
            let storage = OpenRaftStorage::new(db, DEFAULT_GROUP_ID, None).unwrap();
            use crate::cluster::types::NodeId;
            use openraft::{BasicNode, CommittedLeaderId, Entry, EntryPayload, LogId, Membership};
            use std::collections::{BTreeMap, BTreeSet};
            let mut voters = BTreeSet::new();
            voters.insert(1);
            voters.insert(2);
            voters.insert(3);
            let mut nodes = BTreeMap::new();
            for id in [1u64, 2, 3] {
                nodes.insert(
                    id,
                    BasicNode {
                        addr: format!("http://127.0.0.1:{id}"),
                    },
                );
            }
            let membership = Membership::<NodeId, BasicNode>::new(vec![voters], nodes);
            let entry = Entry::<TypeConfig> {
                log_id: LogId::new(CommittedLeaderId::new(1, 1), 1),
                payload: EntryPayload::Membership(membership),
            };
            storage.apply_entries_internal(&[entry]).unwrap();
        }
        let db = DB::open(&path, Options::for_testing()).unwrap();
        let storage = OpenRaftStorage::new(db, DEFAULT_GROUP_ID, None).unwrap();
        let loaded = storage.load_membership().unwrap();
        assert_eq!(loaded.log_id().map(|id| id.index), Some(1));
        assert_eq!(loaded.membership().get_joint_config().len(), 1);
        assert_eq!(
            storage
                .read_last_applied_from_db()
                .unwrap()
                .map(|id| id.index),
            Some(1)
        );
    }
}
