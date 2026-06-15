//! Raft / state-machine key encoding for AiDb cluster mode.

/// P12 单 Group 测试默认 group_id (0 预留给 MetaRaft).
pub const DEFAULT_GROUP_ID: u64 = 1;

pub fn gid_bytes(group_id: u64) -> [u8; 8] {
  group_id.to_be_bytes()
}

pub fn vote_key(group_id: u64) -> Vec<u8> {
  let gid = gid_bytes(group_id);
  [b"\x00raft/", gid.as_slice(), b"/vote"].concat()
}

pub fn last_applied_key(group_id: u64) -> Vec<u8> {
  let gid = gid_bytes(group_id);
  [b"\x00raft/", gid.as_slice(), b"/last_applied"].concat()
}

pub fn membership_key(group_id: u64) -> Vec<u8> {
  let gid = gid_bytes(group_id);
  [b"\x00raft/", gid.as_slice(), b"/membership"].concat()
}

pub fn snapshot_meta_key(group_id: u64) -> Vec<u8> {
  let gid = gid_bytes(group_id);
  [b"\x00raft/", gid.as_slice(), b"/snapshot_meta"].concat()
}

pub fn log_key(group_id: u64, index: u64) -> Vec<u8> {
  let gid = gid_bytes(group_id);
  let idx = index.to_be_bytes();
  [b"\x00raft/", gid.as_slice(), b"/log/", idx.as_slice()].concat()
}

pub fn log_prefix(group_id: u64) -> Vec<u8> {
  let gid = gid_bytes(group_id);
  [b"\x00raft/", gid.as_slice(), b"/log/"].concat()
}

/// Sort upper bound for `\x00raft/{gid}/log/*` — `'0'` (0x30) > `'/'` (0x2f).
pub fn log_range_end(group_id: u64) -> Vec<u8> {
  let gid = gid_bytes(group_id);
  [b"\x00raft/", gid.as_slice(), b"/log0"].concat()
}

pub fn sm_key(group_id: u64, user_key: &[u8]) -> Vec<u8> {
  let gid = gid_bytes(group_id);
  [b"\x01sm/", gid.as_slice(), b"/", user_key].concat()
}

pub fn sm_range_start(group_id: u64) -> Vec<u8> {
  let gid = gid_bytes(group_id);
  [b"\x01sm/", gid.as_slice(), b"/"].concat()
}

pub fn sm_range_end(group_id: u64) -> Vec<u8> {
  let gid = gid_bytes(group_id);
  [b"\x01sm/", gid.as_slice(), b"0"].concat()
}

// reserved for per-temp snapshot install (09-raft § snapshot_temp)
#[expect(dead_code)]
pub fn snapshot_temp_prefix(temp_id: &str) -> Vec<u8> {
  [b"\x02snapshot_temp/", temp_id.as_bytes(), b"/"].concat()
}

// reserved for per-temp snapshot install — pairing prefix
#[expect(dead_code)]
pub fn snapshot_temp_range_end(temp_id: &str) -> Vec<u8> {
  [b"\x02snapshot_temp/", temp_id.as_bytes(), b"0"].concat()
}

pub fn snapshot_temp_global_start() -> &'static [u8] {
  b"\x02snapshot_temp/"
}

pub fn snapshot_temp_global_end() -> &'static [u8] {
  b"\x02snapshot_temp0"
}

pub fn meta_cluster_meta_key() -> Vec<u8> {
  b"\x00meta_raft/cluster_meta".to_vec()
}

pub fn meta_slot_table_key() -> Vec<u8> {
  b"\x00meta_raft/slot_table".to_vec()
}

pub fn meta_migration_state_key() -> Vec<u8> {
  b"\x00meta_raft/migration_state".to_vec()
}

pub fn meta_range_start() -> Vec<u8> {
  b"\x00meta_raft/".to_vec()
}

pub fn meta_range_end() -> Vec<u8> {
  b"\x00meta_raft0".to_vec()
}

pub fn user_key_from_sm_key(group_id: u64, sm_key: &[u8]) -> Option<Vec<u8>> {
  let prefix = sm_range_start(group_id);
  sm_key.strip_prefix(prefix.as_slice()).map(|k| k.to_vec())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cluster::meta_types::METARAFT_GROUP_ID;

  #[test]
  fn test_raft_key_encoding() {
    let gid = 1u64;
    let vote = vote_key(gid);
    assert!(vote.starts_with(b"\x00raft/"));
    let log = log_key(gid, 5);
    assert!(log.starts_with(&log_prefix(gid)));
    let sm = sm_key(gid, b"user");
    assert_eq!(sm, b"\x01sm/\0\0\0\0\0\0\0\x01/user");
  }

  #[test]
  fn test_meta_key_sort_order() {
    assert!(meta_range_start() < vote_key(0));
    assert!(meta_cluster_meta_key() < vote_key(METARAFT_GROUP_ID));
  }
}
