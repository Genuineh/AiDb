//! Raft / state-machine key encoding for AiDb cluster mode.

/// P12 单 Group 测试默认 group_id (0 预留给 MetaRaft).
pub const DEFAULT_GROUP_ID: u64 = 1;

pub fn gid_bytes(group_id: u64) -> [u8; 8] {
    group_id.to_be_bytes()
}

pub fn last_log_id_key(group_id: u64) -> Vec<u8> {
    let gid = gid_bytes(group_id);
    [b"\x00raft/", gid.as_slice(), b"/last_log_id"].concat()
}

pub fn last_purged_log_id_key(group_id: u64) -> Vec<u8> {
    let gid = gid_bytes(group_id);
    [b"\x00raft/", gid.as_slice(), b"/last_purged_log_id"].concat()
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

/// FIX-0056-A1: 当前活跃迁移的 oplog epoch, 与 `meta_migration_state_key`
/// 同生命周期 (`MetaStateMachine::migration_epoch`).
pub fn meta_migration_epoch_key() -> Vec<u8> {
    b"\x00meta_raft/migration_epoch".to_vec()
}

#[allow(dead_code)]
pub fn meta_range_start() -> Vec<u8> {
    b"\x00meta_raft/".to_vec()
}

#[allow(dead_code)]
pub fn meta_range_end() -> Vec<u8> {
    b"\x00meta_raft0".to_vec()
}

pub fn user_key_from_sm_key(group_id: u64, sm_key: &[u8]) -> Option<Vec<u8>> {
    let prefix = sm_range_start(group_id);
    sm_key.strip_prefix(prefix.as_slice()).map(|k| k.to_vec())
}

pub fn epoch_bytes(epoch: u64) -> [u8; 8] {
    epoch.to_be_bytes()
}

/// FIX-0056-A1: 迁移 tombstone/oplog 前缀, 独立于 `\x01sm/` — 不会出现在
/// SCAN/COUNTKEYSINSLOT (只扫 `sm_range_start..sm_range_end`), 也不会与用户
/// key 混淆.
fn mig_prefix(group_id: u64, epoch: u64) -> Vec<u8> {
    let gid = gid_bytes(group_id);
    let ep = epoch_bytes(epoch);
    [b"\x02mig/", gid.as_slice(), b"/", ep.as_slice()].concat()
}

/// 单个 user key 的迁移 tombstone: 记录该 key 在本 epoch 内最后一次是被
/// Put 还是 Del, 以及 apply 时分配的单调 seq (见 `migration_oplog.rs`).
pub fn mig_tombstone_key(group_id: u64, epoch: u64, user_key: &[u8]) -> Vec<u8> {
    [
        mig_prefix(group_id, epoch),
        b"/ts/".to_vec(),
        user_key.to_vec(),
    ]
    .concat()
}

/// 本 epoch 内已分配的最大 seq (tip), 随 target group Raft apply 单调递增.
pub fn mig_tip_key(group_id: u64, epoch: u64) -> Vec<u8> {
    [mig_prefix(group_id, epoch), b"/tip".to_vec()].concat()
}

pub fn mig_range_start(group_id: u64, epoch: u64) -> Vec<u8> {
    [mig_prefix(group_id, epoch), b"/".to_vec()].concat()
}

/// Sort upper bound for `\x02mig/{gid}/{epoch}/*` — `'0'` (0x30) > `'/'` (0x2f),
/// 与 `sm_range_end` 同一模式.
pub fn mig_range_end(group_id: u64, epoch: u64) -> Vec<u8> {
    [mig_prefix(group_id, epoch), b"0".to_vec()].concat()
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

    #[test]
    fn test_mig_keys_disjoint_from_sm_and_snapshot_temp() {
        // \x01sm/ < \x02mig/ < \x02snapshot_temp/ — 三者互不相交, mig key
        // 绝不会落进 sm_range_start..sm_range_end (SCAN/COUNTKEYSINSLOT 只扫这个区间).
        let gid = 1u64;
        let epoch = 7u64;
        assert!(sm_range_end(gid) <= mig_range_start(gid, epoch));
        assert!(mig_range_end(gid, epoch) <= snapshot_temp_global_start().to_vec());
    }

    #[test]
    fn test_mig_tip_and_tombstone_within_range() {
        let gid = 2u64;
        let epoch = 3u64;
        let start = mig_range_start(gid, epoch);
        let end = mig_range_end(gid, epoch);
        let tip = mig_tip_key(gid, epoch);
        let ts = mig_tombstone_key(gid, epoch, b"user-key");
        assert!(
            start <= tip && tip < end,
            "tip key must fall within [start, end)"
        );
        assert!(
            start <= ts && ts < end,
            "tombstone key must fall within [start, end)"
        );
    }

    #[test]
    fn test_mig_keys_scoped_by_group_and_epoch() {
        // 不同 group / 不同 epoch 的区间不重叠, 且互不包含对方的 key.
        let a_start = mig_range_start(1, 1);
        let a_end = mig_range_end(1, 1);
        let b_tip = mig_tip_key(1, 2);
        let c_tip = mig_tip_key(2, 1);
        assert!(
            !(a_start <= b_tip && b_tip < a_end),
            "different epoch must not collide"
        );
        assert!(
            !(a_start <= c_tip && c_tip < a_end),
            "different group must not collide"
        );
    }

    #[test]
    fn test_mig_tombstone_key_distinguishes_user_keys() {
        let a = mig_tombstone_key(1, 1, b"a");
        let b = mig_tombstone_key(1, 1, b"b");
        assert_ne!(a, b);
    }
}
