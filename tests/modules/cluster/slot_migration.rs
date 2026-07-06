//! stage2-migration 回归测试.
//!
//! 覆盖修复前的两个真实 bug:
//! 1. `commit_migration` 曾经只检查 "executor 存不存在 / 有没有被取消",
//!    在 `run_pending_migration` 从未被调用过的情况下 (对应
//!    `CLUSTER SETSLOT MIGRATING` 之后直接 `STABLE` 的手动流程) 也会放行,
//!    导致 target 上一个 key 都没有就把 slot 所有权切过去, 数据静默丢失。
//! 2. `cancel_migration` 从不清理 target 上已经拷贝过去的残留副本。
//!
//! 使用真实的单节点 `MetaRaftNode` + 双 group 的 `MultiRaftNode`
//! (source=1, target=2) 驱动 `SlotMigrationManager`, 不使用任何 mock。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use aidb::cluster::multi_raft_node::LifecycleConfig;
use aidb::cluster::slot_migration::{ActiveMigration, SlotMigrationManager};
use aidb::cluster::{
    ClusterMeta, MetaRaftNode, MetaRequest, MultiRaftNode, RaftNetworkClientFactory,
    RaftServiceDispatcher, Router, SlotMigrationState, SlotTable,
};
use aidb::config::{MigrationConfig, Options};
use aidb::cluster::lifecycle_manager::{LifecycleManager, MetaRaftProvider};
use aidb::DB;
use tempfile::TempDir;

struct MetaRaftProv(Arc<MetaRaftNode>);

impl MetaRaftProvider for MetaRaftProv {
    fn get_cluster_meta(&self) -> ClusterMeta {
        self.0.get_cluster_meta()
    }
    fn get_slot_table(&self) -> SlotTable {
        self.0.get_slot_table()
    }
    fn get_migration_state(&self) -> Option<SlotMigrationState> {
        self.0.get_migration_state()
    }
}

const TEST_KEY: &[u8] = b"k-slot0";

/// 搭好一个单节点、两个数据 group (1=source, 2=target) 的最小集群, 把
/// `TEST_KEY` 所在的 slot 分给 group 1。返回
/// (meta_raft, multi_raft, migration_manager, migrated_slots, _dirs)。
async fn setup(
    _unused: u16,
) -> (
    Arc<MetaRaftNode>,
    Arc<MultiRaftNode>,
    SlotMigrationManager,
    Vec<u16>,
    TempDir,
) {
    use aidb::cluster::types::RaftNodeConfig;

    let meta_dir = TempDir::new().unwrap();
    let meta_db = DB::open(meta_dir.path().join("meta"), Options::for_testing()).unwrap();
    let meta_factory = RaftNetworkClientFactory::new(1, aidb::cluster::METARAFT_GROUP_ID, 30, 65 * 1024 * 1024);
    let meta_cfg = RaftNodeConfig {
        node_id: 1,
        group_id: aidb::cluster::METARAFT_GROUP_ID,
        election_timeout_min: 150,
        election_timeout_max: 300,
        heartbeat_interval: 30,
        rpc_timeout_ms: 30,
        snapshot_logs_since_last: 200,
        ..Default::default()
    };
    let meta_raft = Arc::new(MetaRaftNode::new(meta_cfg, meta_db, meta_factory).await.unwrap());
    meta_raft
        .initialize(vec![(1, "http://127.0.0.1:1".into())])
        .await
        .unwrap();
    for _ in 0..50 {
        if meta_raft.is_leader().await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(meta_raft.is_leader().await, "meta raft should elect itself as leader");

    meta_raft
        .propose(MetaRequest::RegisterNode {
            node_id: 1,
            rpc_addr: "http://127.0.0.1:19100".into(),
            client_addr: None,
            tags: HashMap::new(),
        })
        .await
        .unwrap();
    meta_raft
        .propose(MetaRequest::CreateGroup {
            group_id: 1,
            initial_replicas: vec![(1, true)],
        })
        .await
        .unwrap();
    meta_raft
        .propose(MetaRequest::CreateGroup {
            group_id: 2,
            initial_replicas: vec![(1, true)],
        })
        .await
        .unwrap();
    let migrated_slots = vec![aidb::cluster::router::key_to_slot(TEST_KEY)];
    meta_raft
        .propose(MetaRequest::AssignSlots {
            group_id: 1,
            slots: migrated_slots.clone(),
        })
        .await
        .unwrap();

    let router = Arc::new(Router::new(
        aidb::cluster::default_slot_table(),
        HashMap::new(),
        HashMap::new(),
    ));
    let dispatcher = Arc::new(RaftServiceDispatcher::new());
    let lifecycle = LifecycleManager::new(1, router.clone(), Arc::new(MetaRaftProv(meta_raft.clone())))
        .with_tick_interval(Duration::from_millis(30));
    let multi_raft = Arc::new(MultiRaftNode::new_with_lifecycle(
        1,
        router.clone(),
        dispatcher,
        lifecycle,
    ));

    let data_dir = TempDir::new().unwrap();
    let _shutdown_rx = multi_raft.start_lifecycle_with_data(LifecycleConfig {
        data_dir: data_dir.path().to_path_buf(),
        raft_node_config: RaftNodeConfig {
            node_id: 1,
            election_timeout_min: 150,
            election_timeout_max: 300,
            heartbeat_interval: 30,
            rpc_timeout_ms: 30,
            snapshot_logs_since_last: 200,
            ..Default::default()
        },
        options: Options::for_testing(),
        compaction_filter: None,
    });

    // 等两个 group 都在本地创建出来并选出 leader.
    for _ in 0..100 {
        let ids = multi_raft.local_group_ids();
        if ids.len() == 2 && multi_raft.is_elected_leader_sync(1) && multi_raft.is_elected_leader_sync(2) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(multi_raft.local_group_ids().len(), 2, "both groups must be created locally");
    assert!(multi_raft.is_elected_leader_sync(1) && multi_raft.is_elected_leader_sync(2));

    let migration_manager = SlotMigrationManager::new(
        meta_raft.clone(),
        multi_raft.clone(),
        router,
        1,
        data_dir.path().join("slot_migration"),
        MigrationConfig::default(),
    );

    (meta_raft, multi_raft, migration_manager, migrated_slots, data_dir)
}

fn active_migration_from_state(meta_raft: &MetaRaftNode, migration_id: u64) -> ActiveMigration {
    match meta_raft.get_migration_state().expect("migration should be active") {
        SlotMigrationState::Prepare {
            source_group,
            target_group,
            slots,
        } => ActiveMigration {
            migration_id,
            source_group,
            target_group,
            slots,
            checkpoint: Vec::new(),
        },
        SlotMigrationState::Migrating {
            source_group,
            target_group,
            slots,
            ..
        } => ActiveMigration {
            migration_id,
            source_group,
            target_group,
            slots,
            checkpoint: Vec::new(),
        },
    }
}

#[tokio::test]
async fn commit_without_running_migration_is_rejected() {
    let (meta_raft, multi_raft, sm, slots, _dirs) = setup(0).await;

    multi_raft
        .propose_key(TEST_KEY.to_vec(), Some(b"v1".to_vec()))
        .await
        .unwrap();

    let migration_id = sm.start_migration(1, 2, slots.clone()).await.unwrap();
    assert!(meta_raft.get_migration_state().is_some());

    // Bug 修复前: 这里会静默成功, slot 所有权直接切给 target, 但 target 上
    // 什么数据都没有 —— 数据静默丢失。修复后必须返回 Err。
    let err = sm.commit_migration().await;
    assert!(
        err.is_err(),
        "commit_migration must reject when run_pending_migration was never called"
    );

    // Slot 状态必须原封不动留在 Migrating(source), 不能被误提交.
    assert_eq!(
        meta_raft.get_slot_table()[slots[0] as usize],
        aidb::cluster::SlotStatus::Migrating(1)
    );
    // 数据必须还在 source, 没有丢.
    assert_eq!(
        multi_raft.get_key_from_group(1, TEST_KEY).await.unwrap(),
        Some(b"v1".to_vec())
    );
    let _ = migration_id;
}

#[tokio::test]
async fn commit_after_full_run_succeeds_and_moves_data() {
    let (meta_raft, multi_raft, sm, slots, _dirs) = setup(0).await;

    multi_raft
        .propose_key(TEST_KEY.to_vec(), Some(b"v1".to_vec()))
        .await
        .unwrap();

    let migration_id = sm.start_migration(1, 2, slots.clone()).await.unwrap();
    let active = active_migration_from_state(&meta_raft, migration_id);

    let result = sm.run_pending_migration(active).await.unwrap();
    assert!(result.is_completed, "migration should fully complete in one pass");

    sm.commit_migration()
        .await
        .expect("commit must succeed once run_pending_migration has fully completed");

    assert_eq!(
        meta_raft.get_slot_table()[slots[0] as usize],
        aidb::cluster::SlotStatus::Assigned(2),
        "slot ownership should have moved to target group"
    );
    assert_eq!(
        multi_raft.get_key_from_group(2, TEST_KEY).await.unwrap(),
        Some(b"v1".to_vec()),
        "data must have been copied to target before commit"
    );
    assert!(meta_raft.get_migration_state().is_none());
}

#[tokio::test]
async fn cancel_migration_cleans_up_target_residuals() {
    let (meta_raft, multi_raft, sm, slots, _dirs) = setup(0).await;

    multi_raft
        .propose_key(TEST_KEY.to_vec(), Some(b"v1".to_vec()))
        .await
        .unwrap();

    let migration_id = sm.start_migration(1, 2, slots.clone()).await.unwrap();
    let active = active_migration_from_state(&meta_raft, migration_id);
    let result = sm.run_pending_migration(active).await.unwrap();
    assert!(result.is_completed);

    // 拷贝确实发生了 (取消前 target 上应该已经有这份数据).
    assert_eq!(
        multi_raft.get_key_from_group(2, TEST_KEY).await.unwrap(),
        Some(b"v1".to_vec())
    );

    sm.cancel_migration().await.expect("cancel should succeed");

    // slot 所有权回滚到 source.
    assert_eq!(
        meta_raft.get_slot_table()[slots[0] as usize],
        aidb::cluster::SlotStatus::Assigned(1)
    );
    assert!(meta_raft.get_migration_state().is_none());
    // 数据仍在 source, 完好无损.
    assert_eq!(
        multi_raft.get_key_from_group(1, TEST_KEY).await.unwrap(),
        Some(b"v1".to_vec())
    );
    // target 上的残留副本必须被清理干净, 否则这批 slot 未来再迁入同一个
    // target 时, PutConditional 会因为 key 已存在而跳过拷贝.
    assert_eq!(
        multi_raft.get_key_from_group(2, TEST_KEY).await.unwrap(),
        None,
        "cancel_migration must clean up residual copies left on target"
    );
}
