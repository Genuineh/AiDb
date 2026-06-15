use std::collections::HashMap;

use aidb::cluster::{MetaRequest, Request, TypeConfig};

#[test]
fn test_meta_types_serde_roundtrip() {
  use aidb::cluster::{default_slot_table, ClusterMeta, NodeRole, NodeStatus, SlotStatus};
  let meta = ClusterMeta {
    cluster_id: "test".into(),
    nodes: HashMap::new(),
    groups: HashMap::new(),
    version: 1,
    format_version: 1,
  };
  let bytes = rmp_serde::to_vec(&meta).unwrap();
  let decoded: ClusterMeta = rmp_serde::from_slice(&bytes).unwrap();
  assert_eq!(decoded, meta);

  let table = default_slot_table();
  let bytes = rmp_serde::to_vec(&table).unwrap();
  let decoded: Vec<SlotStatus> = rmp_serde::from_slice(&bytes).unwrap();
  assert_eq!(decoded.len(), table.len());

  let _ = (NodeRole::Voter, NodeStatus::Online);
}

#[test]
fn test_meta_request_in_entry_roundtrip() {
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
  let bytes = rmp_serde::to_vec(&entry).unwrap();
  let decoded: Entry<TypeConfig> = rmp_serde::from_slice(&bytes).unwrap();
  assert!(matches!(
    decoded.payload,
    EntryPayload::Normal(Request::Meta(MetaRequest::RegisterNode { .. }))
  ));
}
