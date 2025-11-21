//! Replica Allocator for load-balanced replica assignment
//!
//! This module implements algorithms for allocating replicas across nodes in a Multi-Raft cluster.
//! It ensures even distribution of groups and considers node load when assigning new replicas.

use std::collections::HashMap;

use super::raft_storage::NodeId;
use super::sharded_storage::GroupId;
use crate::error::{Error, Result};

/// Replica allocator for load-balanced group assignment
///
/// This allocator implements a simple but effective algorithm:
/// 1. Prefer nodes with fewer groups (load balancing)
/// 2. Ensure replicas are on different nodes (fault tolerance)
/// 3. Maintain target replication factor
pub struct ReplicaAllocator {
    /// Target replication factor (e.g., 3 for triple replication)
    replication_factor: usize,
}

impl ReplicaAllocator {
    /// Create a new replica allocator
    ///
    /// # Arguments
    ///
    /// * `replication_factor` - Number of replicas per group (typically 3 or 5)
    pub fn new(replication_factor: usize) -> Self {
        Self { replication_factor }
    }

    /// Allocate replicas for a new group
    ///
    /// Selects `replication_factor` nodes with the lowest current load.
    ///
    /// # Arguments
    ///
    /// * `group_id` - ID of the group to allocate replicas for
    /// * `available_nodes` - List of available node IDs
    /// * `current_allocation` - Current group-to-replicas mapping
    ///
    /// # Returns
    ///
    /// A vector of node IDs that should host replicas for this group
    ///
    /// # Errors
    ///
    /// Returns an error if there are not enough nodes to satisfy the replication factor
    pub fn allocate_replicas(
        &self,
        _group_id: GroupId,
        available_nodes: &[NodeId],
        current_allocation: &HashMap<GroupId, Vec<NodeId>>,
    ) -> Result<Vec<NodeId>> {
        if available_nodes.len() < self.replication_factor {
            return Err(Error::Internal(format!(
                "Not enough nodes: need {}, have {}",
                self.replication_factor,
                available_nodes.len()
            )));
        }

        // Calculate load for each node (number of groups it participates in)
        let mut node_loads: HashMap<NodeId, usize> = HashMap::new();
        for node_id in available_nodes {
            node_loads.insert(*node_id, 0);
        }

        // Count existing group assignments
        for replicas in current_allocation.values() {
            for &replica in replicas {
                if node_loads.contains_key(&replica) {
                    *node_loads.get_mut(&replica).unwrap() += 1;
                }
            }
        }

        // Sort nodes by load (ascending)
        let mut nodes_by_load: Vec<(NodeId, usize)> =
            node_loads.into_iter().collect();
        nodes_by_load.sort_by_key(|(_, load)| *load);

        // Select the least loaded nodes
        let selected_nodes: Vec<NodeId> = nodes_by_load
            .into_iter()
            .take(self.replication_factor)
            .map(|(node_id, _)| node_id)
            .collect();

        Ok(selected_nodes)
    }

    /// Rebalance replicas when nodes join or leave
    ///
    /// This method examines the current allocation and proposes changes to
    /// achieve better load balance across all nodes.
    ///
    /// # Arguments
    ///
    /// * `available_nodes` - Current list of available nodes
    /// * `current_allocation` - Current group-to-replicas mapping
    ///
    /// # Returns
    ///
    /// A new allocation map with proposed changes for better balance
    ///
    /// # Algorithm
    ///
    /// 1. Calculate current load per node
    /// 2. For under-replicated groups, add new replicas
    /// 3. For over-replicated groups, remove excess replicas
    /// 4. Redistribute to minimize load imbalance
    pub fn rebalance(
        &self,
        available_nodes: &[NodeId],
        current_allocation: HashMap<GroupId, Vec<NodeId>>,
    ) -> Result<HashMap<GroupId, Vec<NodeId>>> {
        if available_nodes.is_empty() {
            return Ok(current_allocation);
        }

        let mut new_allocation = HashMap::new();

        // Process each group
        for (group_id, current_replicas) in &current_allocation {
            // Filter out nodes that are no longer available
            let valid_replicas: Vec<NodeId> = current_replicas
                .iter()
                .filter(|&node| available_nodes.contains(node))
                .copied()
                .collect();

            // Determine if we need to add or remove replicas
            if valid_replicas.len() < self.replication_factor {
                // Under-replicated: add new replicas
                let needed = self.replication_factor - valid_replicas.len();
                let mut updated_replicas = valid_replicas.clone();

                // Calculate node loads
                let mut node_loads = self.calculate_node_loads(
                    available_nodes,
                    &new_allocation,
                );

                // Add load from current group's valid replicas
                for &replica in &valid_replicas {
                    *node_loads.entry(replica).or_insert(0) += 1;
                }

                // Find candidates (nodes not already in this group)
                let mut candidates: Vec<(NodeId, usize)> = node_loads
                    .into_iter()
                    .filter(|(node_id, _)| !valid_replicas.contains(node_id))
                    .collect();
                candidates.sort_by_key(|(_, load)| *load);

                // Add the least loaded candidates
                for (node_id, _) in candidates.into_iter().take(needed) {
                    updated_replicas.push(node_id);
                }

                new_allocation.insert(*group_id, updated_replicas);
            } else if valid_replicas.len() > self.replication_factor {
                // Over-replicated: remove excess replicas
                // Keep the replicas on most loaded nodes (they might have fewer other groups)
                let mut replicas_with_load: Vec<(NodeId, usize)> =
                    valid_replicas
                        .iter()
                        .map(|&node| {
                            let load = self.count_node_load(node, &current_allocation);
                            (node, load)
                        })
                        .collect();

                // Sort by load (descending) and keep the top replication_factor
                replicas_with_load.sort_by_key(|(_, load)| std::cmp::Reverse(*load));
                let kept_replicas: Vec<NodeId> = replicas_with_load
                    .into_iter()
                    .take(self.replication_factor)
                    .map(|(node, _)| node)
                    .collect();

                new_allocation.insert(*group_id, kept_replicas);
            } else {
                // Correctly replicated: keep as is
                new_allocation.insert(*group_id, valid_replicas);
            }
        }

        Ok(new_allocation)
    }

    /// Calculate current load for each node
    fn calculate_node_loads(
        &self,
        available_nodes: &[NodeId],
        allocation: &HashMap<GroupId, Vec<NodeId>>,
    ) -> HashMap<NodeId, usize> {
        let mut node_loads: HashMap<NodeId, usize> = HashMap::new();

        // Initialize all nodes with 0 load
        for &node_id in available_nodes {
            node_loads.insert(node_id, 0);
        }

        // Count group assignments
        for replicas in allocation.values() {
            for &replica in replicas {
                if node_loads.contains_key(&replica) {
                    *node_loads.get_mut(&replica).unwrap() += 1;
                }
            }
        }

        node_loads
    }

    /// Count how many groups a specific node participates in
    fn count_node_load(
        &self,
        node_id: NodeId,
        allocation: &HashMap<GroupId, Vec<NodeId>>,
    ) -> usize {
        allocation
            .values()
            .filter(|replicas| replicas.contains(&node_id))
            .count()
    }

    /// Get the replication factor
    pub fn replication_factor(&self) -> usize {
        self.replication_factor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_replicas_basic() {
        let allocator = ReplicaAllocator::new(3);
        let available_nodes = vec![1, 2, 3, 4, 5];
        let current_allocation = HashMap::new();

        let result = allocator.allocate_replicas(100, &available_nodes, &current_allocation);
        assert!(result.is_ok());

        let replicas = result.unwrap();
        assert_eq!(replicas.len(), 3);

        // All replicas should be unique
        let unique_replicas: std::collections::HashSet<_> = replicas.iter().collect();
        assert_eq!(unique_replicas.len(), 3);
    }

    #[test]
    fn test_allocate_replicas_insufficient_nodes() {
        let allocator = ReplicaAllocator::new(3);
        let available_nodes = vec![1, 2]; // Only 2 nodes, need 3
        let current_allocation = HashMap::new();

        let result = allocator.allocate_replicas(100, &available_nodes, &current_allocation);
        assert!(result.is_err());
    }

    #[test]
    fn test_allocate_replicas_load_balancing() {
        let allocator = ReplicaAllocator::new(3);
        let available_nodes = vec![1, 2, 3, 4, 5];

        // Create allocation where nodes 1 and 2 are heavily loaded
        let mut current_allocation = HashMap::new();
        for group_id in 0..5 {
            current_allocation.insert(group_id, vec![1, 2, 3]);
        }

        // Allocate for a new group - should prefer less loaded nodes
        let result = allocator.allocate_replicas(100, &available_nodes, &current_allocation);
        assert!(result.is_ok());

        let replicas = result.unwrap();
        assert_eq!(replicas.len(), 3);

        // Should include nodes 4 and 5 (least loaded)
        assert!(replicas.contains(&4) || replicas.contains(&5));
    }

    #[test]
    fn test_rebalance_under_replicated() {
        let allocator = ReplicaAllocator::new(3);
        let available_nodes = vec![1, 2, 3, 4, 5];

        // Group with only 2 replicas (under-replicated)
        let mut current_allocation = HashMap::new();
        current_allocation.insert(100, vec![1, 2]);

        let result = allocator.rebalance(&available_nodes, current_allocation);
        assert!(result.is_ok());

        let new_allocation = result.unwrap();
        let replicas = new_allocation.get(&100).unwrap();

        // Should now have 3 replicas
        assert_eq!(replicas.len(), 3);
        assert!(replicas.contains(&1));
        assert!(replicas.contains(&2));
    }

    #[test]
    fn test_rebalance_over_replicated() {
        let allocator = ReplicaAllocator::new(3);
        let available_nodes = vec![1, 2, 3, 4, 5];

        // Group with 5 replicas (over-replicated)
        let mut current_allocation = HashMap::new();
        current_allocation.insert(100, vec![1, 2, 3, 4, 5]);

        let result = allocator.rebalance(&available_nodes, current_allocation);
        assert!(result.is_ok());

        let new_allocation = result.unwrap();
        let replicas = new_allocation.get(&100).unwrap();

        // Should now have exactly 3 replicas
        assert_eq!(replicas.len(), 3);
    }

    #[test]
    fn test_rebalance_node_removal() {
        let allocator = ReplicaAllocator::new(3);

        // Create allocation with all 5 nodes
        let mut current_allocation = HashMap::new();
        current_allocation.insert(100, vec![1, 2, 3]);
        current_allocation.insert(101, vec![2, 3, 4]);
        current_allocation.insert(102, vec![3, 4, 5]);

        // Node 5 leaves
        let available_nodes = vec![1, 2, 3, 4];

        let result = allocator.rebalance(&available_nodes, current_allocation);
        assert!(result.is_ok());

        let new_allocation = result.unwrap();

        // Group 102 should have a new replica (replacing node 5)
        let group_102_replicas = new_allocation.get(&102).unwrap();
        assert_eq!(group_102_replicas.len(), 3);
        assert!(!group_102_replicas.contains(&5));
    }

    #[test]
    fn test_rebalance_empty_nodes() {
        let allocator = ReplicaAllocator::new(3);
        let available_nodes = vec![];
        let current_allocation = HashMap::new();

        let result = allocator.rebalance(&available_nodes, current_allocation);
        assert!(result.is_ok());

        let new_allocation = result.unwrap();
        assert_eq!(new_allocation.len(), 0);
    }

    #[test]
    fn test_multiple_allocations_balance() {
        let allocator = ReplicaAllocator::new(3);
        let available_nodes = vec![1, 2, 3, 4, 5];
        let mut current_allocation = HashMap::new();

        // Allocate 10 groups sequentially
        for group_id in 0..10 {
            let replicas = allocator
                .allocate_replicas(group_id, &available_nodes, &current_allocation)
                .unwrap();
            current_allocation.insert(group_id, replicas);
        }

        // Count load per node
        let mut node_counts: HashMap<NodeId, usize> = HashMap::new();
        for replicas in current_allocation.values() {
            for &replica in replicas {
                *node_counts.entry(replica).or_insert(0) += 1;
            }
        }

        // Each node should have roughly equal load
        let min_load = *node_counts.values().min().unwrap();
        let max_load = *node_counts.values().max().unwrap();

        // Load difference should be at most 2 (10 groups * 3 replicas = 30 assignments / 5 nodes = 6 each)
        assert!(max_load - min_load <= 2);
    }
}
