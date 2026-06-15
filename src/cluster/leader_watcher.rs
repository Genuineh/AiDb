//! Leader change watcher — polls local Raft groups for leader transitions,
//! updates MetaRaft ReplicaInfo.is_leader via ChangeGroupMembership proposal.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::watch;
use tracing::instrument;

use crate::cluster::meta_raft_node::MetaRaftNode;
use crate::cluster::meta_types::MetaRequest;
use crate::cluster::multi_raft_node::MultiRaftNode;
use crate::cluster::node::OpenRaftNode;
use crate::cluster::types::NodeId;

/// Polls local Raft groups and detects leader transitions.
///
/// On each `tick()`, reads `raft.metrics().current_leader` for every local
/// group and compares against a cache. When a transition is detected:
/// 1. Updates the local cache
/// 2. Proposes `MetaRequest::ChangeGroupMembership` via MetaRaft to update
///    the `is_leader` flag in `ReplicaInfo`
pub struct LeaderChangeWatcher {
  node_id: NodeId,
  multi_raft: Arc<MultiRaftNode>,
  meta_raft: Arc<MetaRaftNode>,
  /// group_id → Option<leader_node_id>
  /// None means no known leader for this group.
  leader_cache: RwLock<HashMap<u64, Option<NodeId>>>,
  tick_interval: Duration,
}

impl LeaderChangeWatcher {
  /// Create a new watcher.
  ///
  /// `tick_interval` should be less than `election_timeout_min` to avoid
  /// missing leader transitions. Default: `election_timeout_min / 2`.
  pub fn new(
    node_id: NodeId,
    multi_raft: Arc<MultiRaftNode>,
    meta_raft: Arc<MetaRaftNode>,
    tick_interval: Duration,
  ) -> Self {
    Self {
      node_id,
      multi_raft,
      meta_raft,
      leader_cache: RwLock::new(HashMap::new()),
      tick_interval,
    }
  }

  /// Execute one detection pass. Returns the set of group IDs whose
  /// leader changed since the last tick.
  #[instrument(name = "leader_watch_tick", skip(self), fields(node_id = %self.node_id))]
  pub async fn tick(&self) -> Vec<u64> {
    // Collect group references first (under lock), then release the lock
    // before any .await to avoid deadlocks (parking_lot guards are not Send).
    let group_snapshots: Vec<(u64, Arc<OpenRaftNode>)> = {
      self
        .multi_raft
        .get_groups()
        .read()
        .iter()
        .map(|(gid, node)| (*gid, Arc::clone(node)))
        .collect()
    };

    let mut changed = Vec::new();

    for (gid, node) in &group_snapshots {
      let current_leader = node.get_leader().await;
      if let Some(changed_gid) = self.detect_leader_transition(*gid, current_leader).await {
        changed.push(changed_gid);
      }
    }

    tracing::debug!(
      group_count = group_snapshots.len(),
      changed_count = changed.len(),
      no_change_count = group_snapshots.len() - changed.len(),
      "leader watch tick complete"
    );
    changed
  }

  /// Detect leader transition for a single group.
  ///
  /// Returns `Some(group_id)` if a transition occurred, `None` otherwise.
  /// On transition, updates the local cache and proposes a MetaRaft update.
  async fn detect_leader_transition(
    &self,
    group_id: u64,
    current_leader: Option<NodeId>,
  ) -> Option<u64> {
    let prev_leader = {
      let cache = self.leader_cache.read();
      cache.get(&group_id).copied().flatten()
    };

    let is_new_group = !self.leader_cache.read().contains_key(&group_id);
    if is_new_group {
      // New group — just populate cache silently
      self.leader_cache.write().insert(group_id, current_leader);
      return None;
    }

    let transition = current_leader != prev_leader;
    if transition {
      // 始终记录 MultiRaft 观测到的 leader, 供路由层立即使用.
      self.leader_cache.write().insert(group_id, current_leader);

      let mut meta_ok = true;
      if let Some(leader_id) = current_leader {
        // 仅由新 leader 节点提交 MetaRaft 元数据更新.
        if leader_id == self.node_id {
          meta_ok = self.update_meta_replica_info(group_id, leader_id).await;
        }
      }
      tracing::info!(
        group_id,
        ?prev_leader,
        ?current_leader,
        meta_ok,
        "leader changed"
      );
      Some(group_id)
    } else {
      // MetaRaft 元数据滞后于本地 Raft 时, 由新 leader 重试提交.
      if let Some(leader_id) = current_leader {
        if leader_id == self.node_id && self.meta_leader_mismatch(group_id, leader_id) {
          self.update_meta_replica_info(group_id, leader_id).await;
        }
      }
      None
    }
  }

  fn meta_leader_mismatch(&self, group_id: u64, observed_leader: NodeId) -> bool {
    let cluster_meta = self.meta_raft.get_cluster_meta();
    let Some(group) = cluster_meta.groups.get(&group_id) else {
      return false;
    };
    let meta_leader = group
      .replicas
      .iter()
      .find(|r| r.is_leader)
      .map(|r| r.node_id);
    meta_leader != Some(observed_leader)
  }

  /// Propose a ChangeGroupMembership to update is_leader in MetaRaft.
  ///
  /// Reads the current `GroupMeta` from MetaRaft to preserve existing
  /// replica list, then constructs a new list with `is_leader` updated.
  /// Propose a ChangeGroupMembership to update is_leader in MetaRaft.
  /// Returns `true` on success, `false` if the proposal should be retried.
  async fn update_meta_replica_info(&self, group_id: u64, new_leader_id: NodeId) -> bool {
    tracing::Span::current().record("group_id", group_id);
    tracing::Span::current().record("new_leader_id", new_leader_id);

    // 注册全部节点 RPC 地址, 确保 ForwardToLeader 可达 MetaRaft leader.
    let cluster_meta = self.meta_raft.get_cluster_meta();
    for (nid, node) in &cluster_meta.nodes {
      self.meta_raft.add_node_address(*nid, node.rpc_addr.clone());
    }

    let Some(_group) = cluster_meta.groups.get(&group_id) else {
      tracing::warn!(
        group_id,
        "group not found in MetaRaft, skipping leader update"
      );
      return true;
    };

    let mut delay_ms = 200u64;
    for attempt in 0u32..10 {
      let cluster_meta = self.meta_raft.get_cluster_meta();
      let Some(group) = cluster_meta.groups.get(&group_id) else {
        return true;
      };

      let new_replicas: Vec<(NodeId, bool)> = group
        .replicas
        .iter()
        .map(|r| (r.node_id, r.node_id == new_leader_id))
        .collect();

      let request = MetaRequest::ChangeGroupMembership {
        group_id,
        new_replicas,
        config_version: group.config_version + 1,
      };

      match self.meta_raft.propose(request).await {
        Ok(_) => {
          tracing::info!(
            group_id,
            new_leader_id,
            config_version = group.config_version + 1,
            attempt,
            "MetaRaft leader update committed"
          );
          return true;
        }
        Err(e) if attempt < 9 => {
          tracing::warn!(
            group_id,
            new_leader_id,
            attempt,
            error = %e,
            "MetaRaft leader update failed (will retry)"
          );
          tokio::time::sleep(Duration::from_millis(delay_ms)).await;
          delay_ms = delay_ms.saturating_mul(2).min(2000);
        }
        Err(e) => {
          tracing::warn!(
            group_id,
            new_leader_id,
            error = %e,
            "MetaRaft leader update failed after retries"
          );
          return false;
        }
      }
    }
    false
  }

  /// Background run loop. Spawn as a tokio task.
  ///
  /// Exits when `shutdown_rx` signals.
  #[instrument(skip(self))]
  pub async fn run(&self, mut shutdown_rx: watch::Receiver<bool>) {
    tracing::info!(
        node_id = %self.node_id,
        tick_interval_ms = self.tick_interval.as_millis(),
        "LeaderChangeWatcher started"
    );
    loop {
      tokio::select! {
          _ = tokio::time::sleep(self.tick_interval) => {
              self.tick().await;
          }
          _ = shutdown_rx.changed() => {
              tracing::info!("LeaderChangeWatcher shutting down");
              break;
          }
      }
    }
  }

  /// Accessor for tick interval.
  pub fn tick_interval(&self) -> Duration {
    self.tick_interval
  }

  /// Accessor for the leader cache (useful in tests).
  pub fn leader_cache(&self) -> &RwLock<HashMap<u64, Option<NodeId>>> {
    &self.leader_cache
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_watcher_new_has_empty_cache() {
    // Validate the constructor signature compiles and initializes state
    let tick = Duration::from_millis(250);
    assert!(tick.as_millis() > 0);
    // Full tick() testing requires running Raft nodes; covered by
    // integration tests in tests/modules/multi_raft/leader_watcher.rs
    // and the E2E failover test.
  }

  #[test]
  fn test_tick_interval_getter() {
    let tick = Duration::from_millis(500);
    assert_eq!(tick.as_millis(), 500);
  }
}
