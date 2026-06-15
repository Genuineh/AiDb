//! State machine apply path.

use openraft::{Entry, EntryPayload, LogId, MessageSummary};

use crate::cluster::meta_types::{MetaRequest, METARAFT_GROUP_ID};
use crate::cluster::storage::keys::{last_applied_key, sm_key};
use crate::cluster::types::{
  ClusterError, Request, Response, ThinWriteBatch, ThinWriteOp, TypeConfig,
};
use crate::engine::db::WriteBatch;
use crate::error::{Error, Result};

use super::OpenRaftStorage;
use crate::cluster::storage::log::map_db_err;

impl OpenRaftStorage {
  pub(crate) fn apply_batch_to_sm(&self, batch: &ThinWriteBatch) -> Result<()> {
    let mut db_batch = WriteBatch::new();
    for op in &batch.ops {
      match op {
        ThinWriteOp::Put { key, value } => {
          db_batch.put(sm_key(self.group_id, key), value.clone());
        }
        ThinWriteOp::Delete { key } => {
          db_batch.delete(sm_key(self.group_id, key));
        }
      }
    }
    self
      .db
      .write(&db_batch)
      .map_err(|e| Error::Cluster(map_db_err(e)))?;
    Ok(())
  }

  pub(crate) fn apply_put_conditional(&self, key: Vec<u8>, value: Vec<u8>) -> Result<()> {
    let sm = sm_key(self.group_id, &key);
    if self.db.get(&sm)?.is_some() {
      return Ok(());
    }
    self
      .db
      .put(&sm, &value)
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

    for entry in entries {
      if let Some(ref applied) = last_applied {
        if entry.log_id.index <= applied.index {
          tracing::trace!(index = entry.log_id.index, "skipping already-applied entry",);
          responses.push(Response::Ok);
          continue;
        }
      }

      tracing::debug!(
        index = entry.log_id.index,
        term = entry.log_id.leader_id.term,
        payload = %entry.payload.summary(),
        "applying entry",
      );

      let response = match &entry.payload {
        EntryPayload::Normal(request) => match request {
          Request::Meta(meta_req)
            if self.group_id == METARAFT_GROUP_ID && self.meta_state.is_some() =>
          {
            self.apply_meta_entry(meta_req, &entry.log_id)?
          }
          _ => self.apply_request(request)?,
        },
        EntryPayload::Membership(m) => {
          let stored = openraft::StoredMembership::new(Some(entry.log_id), m.clone());
          self.persist_membership(&stored)?;
          Response::Ok
        }
        EntryPayload::Blank => Response::Ok,
      };

      if !matches!(
        (&entry.payload, self.group_id),
        (EntryPayload::Normal(Request::Meta(_)), METARAFT_GROUP_ID) if self.meta_state.is_some()
      ) {
        self.persist_last_applied(&entry.log_id)?;
      }
      last_applied = Some(entry.log_id);
      responses.push(response);
    }

    Ok(responses)
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
    let la = rmp_serde::to_vec(log_id)
      .map_err(|e| Error::Cluster(ClusterError::Serialization(e.to_string())))?;
    batch.put(last_applied_key(self.group_id), la);
    self
      .db
      .write(&batch)
      .map_err(|e| Error::Cluster(map_db_err(e)))?;
    self.state.write().last_applied = Some(*log_id);
    Ok(response)
  }

  fn apply_request(&self, request: &Request) -> Result<Response> {
    match request {
      Request::Meta(_) => Ok(Response::Error("meta request on non-meta storage".into())),
      Request::PutConditional { key, value } => {
        self.apply_put_conditional(key.clone(), value.clone())?;
        Ok(Response::Ok)
      }
      Request::Put { key, value } => {
        let mut batch = ThinWriteBatch::new();
        batch.put(key.clone(), value.clone());
        self.apply_batch_to_sm(&batch)?;
        Ok(Response::Ok)
      }
      Request::Delete { key } => {
        let mut batch = ThinWriteBatch::new();
        batch.delete(key.clone());
        self.apply_batch_to_sm(&batch)?;
        Ok(Response::Ok)
      }
      Request::WriteBatch(batch) => {
        self.apply_batch_to_sm(batch)?;
        Ok(Response::Ok)
      }
    }
  }

  pub fn get_state_machine_value(&self, user_key: &[u8]) -> Result<Option<Vec<u8>>> {
    self
      .db
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
  }
}
