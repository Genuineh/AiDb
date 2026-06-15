//! Replica allocation and rebalancing (Phase 15).

use std::collections::HashMap;

use tracing::{instrument, warn};

use crate::cluster::meta_types::{ClusterMeta, SlotStatus, SLOT_COUNT};
use crate::cluster::types::{ClusterError, NodeId};

pub type WeightMap = HashMap<NodeId, f64>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocationStrategy {
  Balanced,
  Weighted,
}

#[derive(Debug, Clone)]
pub struct AllocationResult {
  pub group_id: u64,
  pub primary: NodeId,
  pub replicas: Vec<NodeId>,
  pub slots: Vec<(u16, u16)>,
}

#[derive(Debug, Clone)]
pub struct ReplicaRebalancePlan {
  pub group_id: u64,
  pub source_node: NodeId,
  pub target_node: NodeId,
  pub slot_ranges: Vec<(u16, u16)>,
}

pub struct ReplicaAllocator {
  weights: WeightMap,
}

impl Default for ReplicaAllocator {
  fn default() -> Self {
    Self::new()
  }
}

impl ReplicaAllocator {
  pub fn new() -> Self {
    Self {
      weights: HashMap::new(),
    }
  }

  pub fn set_weight(&mut self, node_id: NodeId, weight: f64) {
    self.weights.insert(node_id, weight);
  }

  pub fn get_weight(&self, node_id: NodeId) -> f64 {
    self.weights.get(&node_id).copied().unwrap_or(1.0)
  }

  #[instrument(skip(self, cluster_meta))]
  pub fn allocate_group(
    &self,
    group_id: u64,
    replication_factor: usize,
    strategy: AllocationStrategy,
    cluster_meta: &ClusterMeta,
    slot_table: &[SlotStatus],
  ) -> Result<AllocationResult, ClusterError> {
    use crate::cluster::NodeStatus;

    // 1. Filter available Online nodes
    let mut available: Vec<NodeId> = cluster_meta
      .nodes
      .iter()
      .filter(|(_, n)| n.status == NodeStatus::Online)
      .map(|(id, _)| *id)
      .collect();
    available.sort();

    if available.is_empty() {
      return Err(ClusterError::InvalidConfig("no available nodes".into()));
    }

    // 2. Handle replication factor
    let actual_rf = if available.len() < replication_factor {
      warn!(
        "replication_factor {} exceeds available nodes {}, using {} replicas",
        replication_factor,
        available.len(),
        available.len()
      );
      available.len()
    } else {
      replication_factor
    };

    // 3. Compute load per node
    let mut loads: HashMap<NodeId, f64> = available
      .iter()
      .map(|id| {
        let count = count_groups_on_node(cluster_meta, *id);
        let load = if strategy == AllocationStrategy::Weighted {
          let w = self.get_weight(*id);
          if w <= 0.0 {
            f64::MAX
          } else {
            count as f64 / w
          }
        } else {
          count as f64
        };
        (*id, load)
      })
      .collect();

    // 4. Pick primary (lowest load)
    let mut sorted: Vec<NodeId> = available.clone();
    sorted.sort_by(|a, b| {
      loads[a]
        .partial_cmp(&loads[b])
        .unwrap_or(std::cmp::Ordering::Equal)
    });
    let primary = sorted[0];
    *loads.get_mut(&primary).unwrap() += 1.0;

    // 5. Pick replicas
    sorted.sort_by(|a, b| {
      loads[a]
        .partial_cmp(&loads[b])
        .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut replicas: Vec<NodeId> = Vec::new();
    for &node in &sorted {
      if node == primary {
        continue;
      }
      if replicas.len() >= actual_rf - 1 {
        break;
      }
      replicas.push(node);
    }

    // 6. Allocate slots
    let slots = suggest_slot_allocation_for_group(cluster_meta, slot_table)
      .ok_or_else(|| ClusterError::InvalidConfig("no free slots available".into()))?;

    Ok(AllocationResult {
      group_id,
      primary,
      replicas,
      slots,
    })
  }

  #[instrument(skip(self, cluster_meta))]
  pub fn rebalance_replicas(
    &self,
    cluster_meta: &ClusterMeta,
    threshold: f64,
  ) -> Vec<ReplicaRebalancePlan> {
    use crate::cluster::NodeStatus;

    let nodes: Vec<NodeId> = cluster_meta
      .nodes
      .iter()
      .filter(|(_, n)| n.status == NodeStatus::Online)
      .map(|(id, _)| *id)
      .collect();

    if nodes.len() < 2 {
      return vec![];
    }

    let loads: HashMap<NodeId, f64> = nodes
      .iter()
      .map(|id| (*id, count_groups_on_node(cluster_meta, *id) as f64))
      .collect();

    let avg: f64 = loads.values().sum::<f64>() / nodes.len() as f64;
    let variance: f64 = loads.values().map(|l| (l - avg).powi(2)).sum::<f64>() / nodes.len() as f64;
    let stddev = variance.sqrt();

    if stddev <= threshold {
      return vec![];
    }

    let overloaded: Vec<NodeId> = nodes
      .iter()
      .filter(|id| loads[id] > avg + threshold)
      .copied()
      .collect();
    let underloaded: Vec<NodeId> = nodes
      .iter()
      .filter(|id| loads[id] < avg - threshold)
      .copied()
      .collect();

    let mut plans = Vec::new();
    for src in &overloaded {
      for dst in &underloaded {
        for (gid, group) in &cluster_meta.groups {
          if group.replicas.iter().any(|r| r.node_id == *src)
            && !group.replicas.iter().any(|r| r.node_id == *dst)
          {
            plans.push(ReplicaRebalancePlan {
              group_id: *gid,
              source_node: *src,
              target_node: *dst,
              slot_ranges: group.slot_ranges.clone(),
            });
            break;
          }
        }
      }
    }
    plans
  }

  pub fn suggest_slot_allocation(group_count: usize) -> Vec<Vec<(u16, u16)>> {
    let slots_per_group = SLOT_COUNT / group_count;
    let remainder = SLOT_COUNT % group_count;
    let mut result = Vec::with_capacity(group_count);
    let mut start = 0u16;
    for i in 0..group_count {
      let count = if i < remainder {
        slots_per_group + 1
      } else {
        slots_per_group
      };
      let end = start + count as u16 - 1;
      result.push(vec![(start, end)]);
      start = end + 1;
    }
    result
  }
}

fn count_groups_on_node(cluster_meta: &ClusterMeta, node_id: NodeId) -> usize {
  cluster_meta
    .groups
    .values()
    .filter(|g| g.replicas.iter().any(|r| r.node_id == node_id))
    .count()
}

#[allow(unused_variables)]
fn suggest_slot_allocation_for_group(
  cluster_meta: &ClusterMeta,
  slot_table: &[SlotStatus],
) -> Option<Vec<(u16, u16)>> {
  let mut ranges = Vec::new();
  let mut i = 0usize;
  while i < SLOT_COUNT {
    if slot_table[i] == SlotStatus::Unallocated {
      let start = i;
      while i < SLOT_COUNT && slot_table[i] == SlotStatus::Unallocated {
        i += 1;
      }
      ranges.push((start as u16, (i - 1) as u16));
    } else {
      i += 1;
    }
  }
  if ranges.is_empty() {
    return None;
  }
  Some(vec![ranges[0]])
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::cluster::meta_types::{
    default_slot_table, ClusterMeta, GroupMeta, NodeInfo, NodeRole, NodeStatus, ReplicaInfo,
  };
  use std::collections::HashMap;

  fn make_meta(nodes: Vec<(u64, NodeStatus)>) -> ClusterMeta {
    ClusterMeta {
      nodes: nodes
        .into_iter()
        .map(|(id, status)| {
          (
            id,
            NodeInfo {
              node_id: id,
              rpc_addr: format!("127.0.0.1:{}", 7000 + id),
              client_addr: None,
              role: NodeRole::Voter,
              status,
              registered_at: 0,
              tags: HashMap::new(),
            },
          )
        })
        .collect(),
      groups: HashMap::new(),
      cluster_id: "test".into(),
      version: 0,
      format_version: 1,
    }
  }

  #[test]
  fn test_allocator_no_nodes() {
    let alloc = ReplicaAllocator::new();
    let meta = make_meta(vec![]);
    let table = default_slot_table();
    let result = alloc.allocate_group(1, 3, AllocationStrategy::Balanced, &meta, &table);
    assert!(result.is_err());
  }

  #[test]
  fn test_allocator_single_node() {
    let alloc = ReplicaAllocator::new();
    let meta = make_meta(vec![(1, NodeStatus::Online)]);
    let table = default_slot_table();
    let result = alloc
      .allocate_group(1, 3, AllocationStrategy::Balanced, &meta, &table)
      .unwrap();
    assert_eq!(result.primary, 1);
    assert!(result.replicas.is_empty());
  }

  #[test]
  fn test_allocator_balanced() {
    let alloc = ReplicaAllocator::new();
    let meta = make_meta(vec![
      (1, NodeStatus::Online),
      (2, NodeStatus::Online),
      (3, NodeStatus::Online),
    ]);
    let table = default_slot_table();
    let result = alloc
      .allocate_group(1, 3, AllocationStrategy::Balanced, &meta, &table)
      .unwrap();
    assert_eq!(result.group_id, 1);
    assert_eq!(result.replicas.len(), 2);
  }

  #[test]
  fn test_allocator_slot_distribution() {
    let result = ReplicaAllocator::suggest_slot_allocation(4);
    assert_eq!(result.len(), 4);
    assert_eq!(result[0][0], (0, 4095));
    assert_eq!(result[3][0], (12288, 16383));
  }

  #[test]
  fn test_rebalance_even() {
    let alloc = ReplicaAllocator::new();
    let meta = make_meta(vec![(1, NodeStatus::Online), (2, NodeStatus::Online)]);
    let plans = alloc.rebalance_replicas(&meta, 1.0);
    assert!(plans.is_empty());
  }

  #[test]
  fn test_rebalance_trigger() {
    let alloc = ReplicaAllocator::new();
    let mut meta = make_meta(vec![(1, NodeStatus::Online), (2, NodeStatus::Online)]);
    meta.groups.insert(
      10,
      GroupMeta {
        group_id: 10,
        replicas: vec![ReplicaInfo {
          node_id: 1,
          is_leader: true,
        }],
        slot_ranges: vec![(0, 100)],
        config_version: 1,
      },
    );
    let plans = alloc.rebalance_replicas(&meta, 0.4);
    assert!(!plans.is_empty());
    assert_eq!(plans[0].source_node, 1);
    assert_eq!(plans[0].target_node, 2);
  }

  #[test]
  fn test_count_groups_on_node() {
    let mut meta = make_meta(vec![(1, NodeStatus::Online), (2, NodeStatus::Online)]);
    meta.groups.insert(
      10,
      GroupMeta {
        group_id: 10,
        replicas: vec![
          ReplicaInfo {
            node_id: 1,
            is_leader: true,
          },
          ReplicaInfo {
            node_id: 2,
            is_leader: false,
          },
        ],
        slot_ranges: vec![],
        config_version: 1,
      },
    );
    meta.groups.insert(
      20,
      GroupMeta {
        group_id: 20,
        replicas: vec![ReplicaInfo {
          node_id: 1,
          is_leader: true,
        }],
        slot_ranges: vec![],
        config_version: 1,
      },
    );
    assert_eq!(count_groups_on_node(&meta, 1), 2);
    assert_eq!(count_groups_on_node(&meta, 2), 1);
  }
}
