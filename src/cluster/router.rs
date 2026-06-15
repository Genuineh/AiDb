//! AiDb 数据面请求路由 (Phase 14: Multi-Raft).
//!
//! 提供 CRC16 哈希、hash tag 提取、slot 计算和请求路由功能.
//! Router 通过 Arc<RwLock<...>> 持有槽表、Group-Node 映射和节点地址表,
//! 支持运行时原子刷新.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::cluster::meta_types::{SlotStatus, SlotTable, SLOT_COUNT};
use crate::cluster::types::{ClusterError, NodeId, ThinWriteOp};

// ---------------------------------------------------------------------------
// CRC16-CCITT 查找表 (多项式 0x1021)
// ---------------------------------------------------------------------------

/// CRC16-CCITT 查找表, 用于计算 key 的 slot.
const CRC16_TABLE: [u16; 256] = [
  0x0000, 0x1021, 0x2042, 0x3063, 0x4084, 0x50a5, 0x60c6, 0x70e7, 0x8108, 0x9129, 0xa14a, 0xb16b,
  0xc18c, 0xd1ad, 0xe1ce, 0xf1ef, 0x1231, 0x0210, 0x3273, 0x2252, 0x52b5, 0x4294, 0x72f7, 0x62d6,
  0x9339, 0x8318, 0xb37b, 0xa35a, 0xd3bd, 0xc39c, 0xf3ff, 0xe3de, 0x2462, 0x3443, 0x0420, 0x1401,
  0x64e6, 0x74c7, 0x44a4, 0x5485, 0xa56a, 0xb54b, 0x8528, 0x9509, 0xe5ee, 0xf5cf, 0xc5ac, 0xd58d,
  0x3653, 0x2672, 0x1611, 0x0630, 0x76d7, 0x66f6, 0x5695, 0x46b4, 0xb75b, 0xa77a, 0x9719, 0x8738,
  0xf7df, 0xe7fe, 0xd79d, 0xc7bc, 0x48c4, 0x58e5, 0x6886, 0x78a7, 0x0840, 0x1861, 0x2802, 0x3823,
  0xc9cc, 0xd9ed, 0xe98e, 0xf9af, 0x8948, 0x9969, 0xa90a, 0xb92b, 0x5af5, 0x4ad4, 0x7ab7, 0x6a96,
  0x1a71, 0x0a50, 0x3a33, 0x2a12, 0xdbfd, 0xcbdc, 0xfbbf, 0xeb9e, 0x9b79, 0x8b58, 0xbb3b, 0xab1a,
  0x6ca6, 0x7c87, 0x4ce4, 0x5cc5, 0x2c22, 0x3c03, 0x0c60, 0x1c41, 0xedae, 0xfd8f, 0xcdec, 0xddcd,
  0xad2a, 0xbd0b, 0x8d68, 0x9d49, 0x7e97, 0x6eb6, 0x5ed5, 0x4ef4, 0x3e13, 0x2e32, 0x1e51, 0x0e70,
  0xff9f, 0xefbe, 0xdfdd, 0xcffc, 0xbf1b, 0xaf3a, 0x9f59, 0x8f78, 0x9188, 0x81a9, 0xb1ca, 0xa1eb,
  0xd10c, 0xc12d, 0xf14e, 0xe16f, 0x1080, 0x00a1, 0x30c2, 0x20e3, 0x5004, 0x4025, 0x7046, 0x6067,
  0x83b9, 0x9398, 0xa3fb, 0xb3da, 0xc33d, 0xd31c, 0xe37f, 0xf35e, 0x02b1, 0x1290, 0x22f3, 0x32d2,
  0x4235, 0x5214, 0x6277, 0x7256, 0xb5ea, 0xa5cb, 0x95a8, 0x8589, 0xf56e, 0xe54f, 0xd52c, 0xc50d,
  0x34e2, 0x24c3, 0x14a0, 0x0481, 0x7466, 0x6447, 0x5424, 0x4405, 0xa7db, 0xb7fa, 0x8799, 0x97b8,
  0xe75f, 0xf77e, 0xc71d, 0xd73c, 0x26d3, 0x36f2, 0x0691, 0x16b0, 0x6657, 0x7676, 0x4615, 0x5634,
  0xd94c, 0xc96d, 0xf90e, 0xe92f, 0x99c8, 0x89e9, 0xb98a, 0xa9ab, 0x5844, 0x4865, 0x7806, 0x6827,
  0x18c0, 0x08e1, 0x3882, 0x28a3, 0xcb7d, 0xdb5c, 0xeb3f, 0xfb1e, 0x8bf9, 0x9bd8, 0xabbb, 0xbb9a,
  0x4a75, 0x5a54, 0x6a37, 0x7a16, 0x0af1, 0x1ad0, 0x2ab3, 0x3a92, 0xfd2e, 0xed0f, 0xdd6c, 0xcd4d,
  0xbdaa, 0xad8b, 0x9de8, 0x8dc9, 0x7c26, 0x6c07, 0x5c64, 0x4c45, 0x3ca2, 0x2c83, 0x1ce0, 0x0cc1,
  0xef1f, 0xff3e, 0xcf5d, 0xdf7c, 0xaf9b, 0xbfba, 0x8fd9, 0x9ff8, 0x6e17, 0x7e36, 0x4e55, 0x5e74,
  0x2e93, 0x3eb2, 0x0ed1, 0x1ef0,
];

/// CRC16-CCITT 计算.
pub fn crc16(data: &[u8]) -> u16 {
  let mut crc: u16 = 0;
  for &byte in data {
    let idx = ((crc >> 8) ^ u16::from(byte)) as usize;
    crc = (crc << 8) ^ CRC16_TABLE[idx & 0xff];
  }
  crc
}

/// 提取 Redis hash tag (第一个 `{...}` 之间的内容).
///
/// 规则:
/// - 找到第一个 `{` 和它之后第一个 `}`.
/// - 如果两者之间存在内容 (`}` 不在 `{` 之后立即出现), 返回该内容.
/// - 否则返回完整的 key.
pub fn extract_hash_tag(key: &[u8]) -> &[u8] {
  let start = key.iter().position(|&b| b == b'{');
  match start {
    Some(s) => {
      let after_open = &key[s + 1..];
      let end = after_open.iter().position(|&b| b == b'}');
      match end {
        Some(e) if e > 0 => &key[s + 1..s + 1 + e],
        _ => key,
      }
    }
    None => key,
  }
}

/// 计算 key 对应的 CRC16 slot (0..16384).
///
/// 如果 key 包含 hash tag, 只对 tag 内的内容计算 CRC16.
pub fn key_to_slot(key: &[u8]) -> u16 {
  let tag = extract_hash_tag(key);
  crc16(tag) % (SLOT_COUNT as u16)
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// 数据面路由表.
///
/// 持有槽表、Group-ID 到 Node 列表的映射、Node-ID 到地址的映射,
/// 以及每个 Group 的当前 leader.
/// 所有内部状态通过 `Arc<RwLock<...>>` 保护, 支持运行时原子刷新.
#[derive(Clone)]
pub struct Router {
  slot_table: Arc<RwLock<SlotTable>>,
  group_nodes: Arc<RwLock<HashMap<u64, Vec<NodeId>>>>,
  node_addrs: Arc<RwLock<HashMap<NodeId, String>>>,
  /// group_id → current leader node_id (from MetaRaft ReplicaInfo.is_leader).
  group_leaders: Arc<RwLock<HashMap<u64, NodeId>>>,
  /// group_id → leader from local MultiRaft observation (优先于 MetaRaft 缓存).
  observed_group_leaders: Arc<RwLock<HashMap<u64, NodeId>>>,
}

impl Router {
  /// 创建新的 Router.
  pub fn new(
    table: SlotTable,
    group_nodes: HashMap<u64, Vec<NodeId>>,
    node_addrs: HashMap<NodeId, String>,
  ) -> Self {
    Self {
      slot_table: Arc::new(RwLock::new(table)),
      group_nodes: Arc::new(RwLock::new(group_nodes)),
      node_addrs: Arc::new(RwLock::new(node_addrs)),
      group_leaders: Arc::new(RwLock::new(HashMap::new())),
      observed_group_leaders: Arc::new(RwLock::new(HashMap::new())),
    }
  }

  /// 路由一个 key 到对应的 Group.
  ///
  /// 返回 `(group_id, slot_status)`, 其中 slot_status 反映槽的当前状态
  /// (Assigned / Migrating).
  pub fn route_key(&self, key: &[u8]) -> Result<(u64, SlotStatus), ClusterError> {
    let slot = key_to_slot(key);
    self.route_slot(slot)
  }

  /// 路由一个 slot 到对应的 Group.
  ///
  /// 如果 slot 未分配 (Unallocated) 或越界, 返回 `ClusterError::InvalidState`.
  pub fn route_slot(&self, slot: u16) -> Result<(u64, SlotStatus), ClusterError> {
    let idx = slot as usize;
    if idx >= SLOT_COUNT {
      return Err(ClusterError::InvalidState(format!(
        "slot {} out of range (max {})",
        slot, SLOT_COUNT
      )));
    }
    let table = self.slot_table.read();
    match &table[idx] {
      SlotStatus::Unallocated => Err(ClusterError::InvalidState(format!(
        "slot {} is not allocated to any group",
        slot
      ))),
      SlotStatus::Assigned(gid) => Ok((*gid, SlotStatus::Assigned(*gid))),
      SlotStatus::Migrating(gid) => Ok((*gid, SlotStatus::Migrating(*gid))),
    }
  }

  /// 将多个 key 按所属 Group 分组.
  ///
  /// 返回值: `HashMap<group_id, Vec<key>>`.
  /// 未分配槽上的 key 会被跳过.
  pub fn route_keys(&self, keys: &[Vec<u8>]) -> HashMap<u64, Vec<Vec<u8>>> {
    let mut groups: HashMap<u64, Vec<Vec<u8>>> = HashMap::new();
    for key in keys {
      if let Ok((gid, _)) = self.route_key(key) {
        groups.entry(gid).or_default().push(key.clone());
      }
    }
    groups
  }

  /// 将多个 slot 按所属 Group 分组.
  ///
  /// 未分配 slot 会被跳过.
  pub fn group_slots(&self, slots: Vec<u16>) -> HashMap<u64, Vec<u16>> {
    let mut groups: HashMap<u64, Vec<u16>> = HashMap::new();
    for slot in slots {
      if let Ok((gid, _)) = self.route_slot(slot) {
        groups.entry(gid).or_default().push(slot);
      }
    }
    groups
  }

  /// 将多个 key 按所属 Group 分组.
  ///
  /// 与 `route_keys` 语义相同, 为 MGET/MSET 等批量操作提供便利.
  pub fn group_keys(&self, keys: &[Vec<u8>]) -> HashMap<u64, Vec<Vec<u8>>> {
    self.route_keys(keys)
  }

  /// 将多个写操作按所属 Group 分组.
  ///
  /// 用于 WriteBatch 分发到不同 Raft Group.
  pub fn group_ops(&self, ops: &[ThinWriteOp]) -> HashMap<u64, Vec<ThinWriteOp>> {
    let mut groups: HashMap<u64, Vec<ThinWriteOp>> = HashMap::new();
    for op in ops {
      let key = match op {
        ThinWriteOp::Put { key, .. } => key.as_slice(),
        ThinWriteOp::Delete { key } => key.as_slice(),
      };
      if let Ok((gid, _)) = self.route_key(key) {
        groups.entry(gid).or_default().push(op.clone());
      }
    }
    groups
  }

  /// 获取 Group 的当前 leader (从 MetaRaft ReplicaInfo.is_leader 缓存).
  ///
  /// 回退到 group 节点列表的第一个节点 (兼容未设置 leader 的情况).
  pub fn get_group_leader(&self, group_id: u64) -> Option<NodeId> {
    if let Some(&leader) = self.observed_group_leaders.read().get(&group_id) {
      return Some(leader);
    }
    // 优先从 group_leaders 缓存读取 (由 LifecycleManager tick 刷新).
    if let Some(&leader) = self.group_leaders.read().get(&group_id) {
      return Some(leader);
    }
    // 回退: 节点列表的第一个.
    let groups = self.group_nodes.read();
    groups
      .get(&group_id)
      .and_then(|nodes| nodes.first().copied())
  }

  /// 获取 Group 的所有节点.
  pub fn get_group_nodes(&self, group_id: u64) -> Option<Vec<NodeId>> {
    let groups = self.group_nodes.read();
    groups.get(&group_id).cloned()
  }

  /// 获取节点的网络地址.
  pub fn get_node_addr(&self, node_id: NodeId) -> Option<String> {
    let addrs = self.node_addrs.read();
    addrs.get(&node_id).cloned()
  }

  /// 更新单个 group 的 leader 缓存 (来自本地 Raft 观测, 不等待 MetaRaft 提交).
  pub fn update_group_leader(&self, group_id: u64, leader_id: NodeId) {
    self
      .observed_group_leaders
      .write()
      .insert(group_id, leader_id);
  }

  /// 原子刷新路由表的所有数据.
  ///
  /// 同时更新 slot table、group->nodes 映射、group->leader 映射和 node->addr 映射.
  pub fn refresh_from_data(
    &self,
    table: SlotTable,
    group_nodes: HashMap<u64, Vec<NodeId>>,
    node_addrs: HashMap<NodeId, String>,
    group_leaders: HashMap<u64, NodeId>,
  ) {
    *self.slot_table.write() = table;
    *self.group_nodes.write() = group_nodes;
    *self.node_addrs.write() = node_addrs;
    *self.group_leaders.write() = group_leaders.clone();
    // MetaRaft 已追平时清除过期的本地观测.
    self
      .observed_group_leaders
      .write()
      .retain(|gid, leader| group_leaders.get(gid) != Some(leader));
  }

  /// 返回 slot table 的只读引用 (Arc).
  pub fn slot_table(&self) -> Arc<RwLock<SlotTable>> {
    Arc::clone(&self.slot_table)
  }

  /// 返回 group->nodes 映射的只读引用 (Arc).
  pub fn group_nodes_map(&self) -> Arc<RwLock<HashMap<u64, Vec<NodeId>>>> {
    Arc::clone(&self.group_nodes)
  }

  /// 返回 node->addr 映射的只读引用 (Arc).
  pub fn node_addrs_map(&self) -> Arc<RwLock<HashMap<NodeId, String>>> {
    Arc::clone(&self.node_addrs)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cluster::meta_types::default_slot_table;

  #[test]
  fn test_crc16_standard_vector() {
    assert_eq!(crc16(b"123456789"), 0x31C3);
  }

  #[test]
  fn test_crc16_empty() {
    assert_eq!(crc16(b""), 0x0000);
  }

  #[test]
  fn test_extract_hash_tag_basic() {
    assert_eq!(extract_hash_tag(b"{user}.name"), b"user");
  }

  #[test]
  fn test_extract_hash_tag_no_braces() {
    assert_eq!(extract_hash_tag(b"username"), b"username");
  }

  #[test]
  fn test_extract_hash_tag_empty_braces() {
    assert_eq!(extract_hash_tag(b"{}.name"), b"{}.name");
  }

  #[test]
  fn test_extract_hash_tag_only_open_brace() {
    assert_eq!(extract_hash_tag(b"key{missing"), b"key{missing");
  }

  #[test]
  fn test_key_to_slot_consistency() {
    let key = b"consistent-key";
    assert_eq!(key_to_slot(key), key_to_slot(key));
  }

  #[test]
  fn test_router_route_slot_unallocated() {
    let table = {
      let mut t = default_slot_table();
      t[0] = SlotStatus::Assigned(1);
      t[1] = SlotStatus::Unallocated;
      t
    };
    let router = Router::new(table, HashMap::new(), HashMap::new());
    assert!(router.route_slot(0).is_ok());
    assert!(router.route_slot(1).is_err());
    assert!(router.route_slot(65535).is_err());
  }

  #[test]
  fn test_router_get_group_leader_no_nodes() {
    let router = Router::new(default_slot_table(), HashMap::new(), HashMap::new());
    assert_eq!(router.get_group_leader(1), None);
  }

  #[test]
  fn test_router_refresh_empty() {
    let router = Router::new(default_slot_table(), HashMap::new(), HashMap::new());
    router.refresh_from_data(
      default_slot_table(),
      HashMap::new(),
      HashMap::new(),
      HashMap::new(),
    );
    assert_eq!(router.get_group_leader(99), None);
  }
}
