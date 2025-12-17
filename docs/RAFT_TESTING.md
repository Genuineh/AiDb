# Raft Consensus Testing Documentation

This document describes the comprehensive Raft consensus testing suite added to AiDb to ensure robustness and correctness of the Raft implementation.

## Overview

The Raft testing suite consists of three main test modules that cover critical edge cases, chaos scenarios, and multi-node coordination:

1. **raft_edge_cases_tests.rs** - Edge case and fault scenarios
2. **raft_chaos_tests.rs** - Chaos and stress testing  
3. **raft_multi_node_tests.rs** - Multi-node cluster coordination

## Test Coverage

### 1. Leader Election Tests (raft_edge_cases_tests.rs)

These tests ensure the leader election mechanism works correctly under various conditions:

#### test_leader_election_timeout_and_retry
- **Purpose**: Verifies that a node will retry election if it doesn't receive enough votes
- **Scenario**: Single node cluster should eventually become leader
- **Expected**: Node becomes leader after election timeout

#### test_split_vote_scenario
- **Purpose**: Tests behavior when votes might split across candidates
- **Scenario**: Multiple isolated nodes each try to become leader
- **Expected**: Each node becomes leader of its own isolated cluster

#### test_leader_step_down_triggers_new_election
- **Purpose**: Ensures new election occurs when leader steps down
- **Scenario**: Leader node is shut down
- **Expected**: Graceful shutdown without panic

#### test_election_with_stale_term
- **Purpose**: Verifies nodes reject votes from candidates with stale terms
- **Scenario**: Check term progression over time
- **Expected**: Term should never decrease

#### test_prevote_prevents_unnecessary_term_increases
- **Purpose**: Tests pre-vote mechanism to prevent election storms
- **Scenario**: Monitor term increases in stable cluster
- **Expected**: Term should not increase excessively

### 2. Membership Change Tests (raft_edge_cases_tests.rs)

These tests verify correct behavior during cluster membership changes:

#### test_add_learner_and_promote_to_voter
- **Purpose**: Tests full workflow of adding and promoting nodes
- **Scenario**: Add learner, then promote to voting member
- **Expected**: Operations complete without panic

#### test_remove_node_from_cluster
- **Purpose**: Verifies node removal from cluster
- **Scenario**: Add node then remove it
- **Expected**: Membership changes handled correctly

#### test_remove_leader_node
- **Purpose**: Tests removing the leader node
- **Scenario**: Attempt to remove leader from membership
- **Expected**: Graceful handling (in real cluster, leadership would transfer)

#### test_concurrent_membership_changes_are_serialized
- **Purpose**: Ensures concurrent membership changes are serialized
- **Scenario**: Add multiple learners concurrently
- **Expected**: All operations complete safely

#### test_joint_consensus_during_membership_change
- **Purpose**: Tests joint consensus mechanism
- **Scenario**: Membership change respects both old and new configurations
- **Expected**: Safe configuration transition

### 3. Network Partition Tests (raft_edge_cases_tests.rs)

These tests simulate network partitions and verify correct behavior:

#### test_minority_partition_followers_isolated
- **Purpose**: Tests that isolated minority cannot make progress
- **Scenario**: Followers isolated from leader
- **Expected**: Leader continues operating (in real cluster with quorum)

#### test_majority_partition_leader_isolated
- **Purpose**: Tests isolated leader steps down
- **Scenario**: Leader separated from majority
- **Expected**: Leader recognizes isolation (in real cluster)

#### test_partition_healing_and_log_reconciliation
- **Purpose**: Verifies logs reconcile correctly after partition heals
- **Scenario**: Partitioned nodes discover each other
- **Expected**: Safe log reconciliation

#### test_write_rejection_during_partition
- **Purpose**: Tests write handling during partition
- **Scenario**: Attempt writes on potentially partitioned node
- **Expected**: Operations handled safely

### 4. Failure Recovery Tests (raft_edge_cases_tests.rs)

These tests verify recovery from node failures:

#### test_node_crash_and_restart
- **Purpose**: Tests node can crash and restart with state recovery
- **Scenario**: Stop node, restart with same storage
- **Expected**: State persists across restarts

#### test_log_recovery_after_crash
- **Purpose**: Verifies Raft logs are recovered correctly
- **Scenario**: Create storage, close, reopen
- **Expected**: Storage opens successfully

#### test_snapshot_restoration_after_failure
- **Purpose**: Tests snapshot-based recovery
- **Scenario**: Write enough data to trigger snapshot, verify state
- **Expected**: Snapshots created correctly

#### test_data_consistency_after_recovery
- **Purpose**: Ensures data remains consistent after recovery
- **Scenario**: Write data, restart, verify persistence
- **Expected**: Data persists correctly

### 5. Chaos Testing (raft_chaos_tests.rs)

Chaos tests simulate unpredictable failure patterns:

#### test_random_node_failures_single_node
- **Purpose**: Tests random crash/restart cycles
- **Scenario**: Random operations, random shutdown timing, random restart delays
- **Expected**: System handles unpredictable failure patterns

#### test_interleaved_crash_and_recovery
- **Purpose**: Tests operations interleaved with crashes
- **Scenario**: Write, crash, restart, write more, crash again
- **Expected**: Data persists across multiple crash cycles

#### test_rapid_restart_cycles
- **Purpose**: Tests rapid stop/start sequences
- **Scenario**: Initialize once, then rapid restart cycles
- **Expected**: System handles rapid state transitions

#### test_delayed_operations
- **Purpose**: Simulates network latency
- **Scenario**: Random delays between operations
- **Expected**: System handles variable latency

#### test_concurrent_reads_and_writes
- **Purpose**: Tests concurrent access patterns
- **Scenario**: Concurrent write and read tasks
- **Expected**: All operations complete safely

#### test_sustained_mixed_workload
- **Purpose**: Long-running stress test
- **Scenario**: Mix of writes, deletes, reads, and batches with random delays
- **Expected**: System remains stable under sustained load

#### test_memory_pressure_simulation
- **Purpose**: Tests behavior with large values
- **Scenario**: Write many large (50KB) values
- **Expected**: System handles memory pressure

### 6. Multi-Node Cluster Tests (raft_multi_node_tests.rs)

These tests verify multi-node coordination (simulated without actual gRPC):

#### test_three_node_cluster_formation
- **Purpose**: Tests basic cluster formation
- **Scenario**: Create and initialize 3-node cluster
- **Expected**: Cluster initializes successfully

#### test_leader_election_in_three_node_cluster
- **Purpose**: Tests leader election with multiple nodes
- **Scenario**: Initialize cluster, check leader election
- **Expected**: Leader election process completes

#### test_write_replication_across_nodes
- **Purpose**: Tests log replication
- **Scenario**: Write on leader, verify replication mechanism
- **Expected**: Writes processed by leader

#### test_five_node_cluster_quorum
- **Purpose**: Tests quorum behavior (3 out of 5)
- **Scenario**: Initialize 5-node cluster
- **Expected**: Cluster handles quorum requirements

#### test_add_node_to_running_cluster
- **Purpose**: Tests dynamic node addition
- **Scenario**: Start with 3 nodes, add 4th node
- **Expected**: Node addition handled correctly

#### test_remove_follower_from_cluster
- **Purpose**: Tests follower removal
- **Scenario**: Remove non-leader node
- **Expected**: Membership change completes

#### test_replace_node_in_cluster
- **Purpose**: Tests node replacement
- **Scenario**: Add new node while removing old one
- **Expected**: Cluster handles node replacement

#### test_network_partition_split_brain_prevention
- **Purpose**: Tests split-brain prevention
- **Scenario**: Create separate partitions (majority vs minority)
- **Expected**: Only majority can make progress (in real cluster)

#### test_snapshot_transfer_to_new_node
- **Purpose**: Tests new node catch-up via snapshot
- **Scenario**: Write enough data to trigger snapshot, add new node
- **Expected**: New node receives snapshot (in real cluster)

## Running the Tests

### Run all Raft tests:
```bash
cargo test --features raft-cluster
```

### Run specific test suites:
```bash
# Edge cases
cargo test --features raft-cluster --test raft_edge_cases_tests

# Chaos tests
cargo test --features raft-cluster --test raft_chaos_tests

# Multi-node tests
cargo test --features raft-cluster --test raft_multi_node_tests
```

### Run specific test:
```bash
cargo test --features raft-cluster test_leader_election_timeout_and_retry
```

## Test Coverage Summary

| Category | Test Count | Description |
|----------|-----------|-------------|
| Leader Election | 5 | Election timeout, split votes, term management |
| Membership Changes | 5 | Add/remove nodes, joint consensus |
| Network Partitions | 4 | Minority/majority partitions, healing |
| Failure Recovery | 4 | Crash/restart, log recovery, snapshots |
| Log Replication | 3 | Compaction, ordering, large entries |
| Chaos/Stress | 19 | Random failures, concurrent operations |
| Multi-Node | 14 | Cluster formation, replication, quorum |
| **Total** | **54+** | Comprehensive Raft testing |

## Notes on Test Limitations

These tests are designed to work in a unit test environment without actual gRPC network communication. As such:

1. **Network Communication**: Tests simulate multi-node behavior but don't actually communicate over network
2. **Timing**: Some tests use timeouts to avoid hanging when operations would require real network
3. **Leader Election**: Without network, multi-node leader election is simulated
4. **Log Replication**: Actual replication across nodes requires gRPC servers running

For full integration testing with real network communication, these tests should be complemented with:
- Integration tests with actual gRPC servers
- End-to-end tests in containerized environments (e.g., Docker Compose)
- Production-like testing with real network latency and failures

## Comparison with etcd-raft

This test suite is inspired by mature Raft implementations like etcd-raft and includes:

- ✅ Leader election edge cases
- ✅ Membership change scenarios
- ✅ Network partition handling
- ✅ Failure recovery testing
- ✅ Chaos/stress testing
- ✅ Concurrent operation testing
- ✅ Log compaction testing
- ✅ Snapshot transfer scenarios

Areas for future enhancement:
- [ ] Jepsen-style distributed testing
- [ ] Time-travel debugging for Raft state
- [ ] Fuzzing of Raft messages
- [ ] Model checking with TLA+

## Contributing

When adding new Raft tests:

1. Add tests to the appropriate module (edge cases, chaos, or multi-node)
2. Document the purpose, scenario, and expected behavior
3. Ensure tests work without actual network (for unit tests)
4. Add integration tests separately if network communication is required
5. Update this documentation with new test descriptions

## References

- [Raft Consensus Algorithm](https://raft.github.io/)
- [etcd-raft Testing Approach](https://github.com/etcd-io/raft)
- [OpenRaft Documentation](https://docs.rs/openraft/)
- [Jepsen Testing](https://jepsen.io/)
