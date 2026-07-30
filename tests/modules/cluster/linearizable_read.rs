//! @component aidb-cluster
//! `linearizable_read` (F-007) 集成回归: `RaftNodeConfig.linearizable_read = true`
//! 时, `OpenRaftNode::get` 经 `ensure_leader_for_linear_read` 走
//! `raft.ensure_linearizable()`; follower 上的 `NotLeader` 错误携带
//! `leader_addr: Some(_)` (openraft `ForwardToLeader` 映射). 与默认路径
//! (`linearizable_read = false`) 的本地 leader check —— follower 上恒为
//! `leader_addr: None` —— 形成可区分对照.

use std::time::Duration;

use aidb::error::{ClusterError, Error};
use tokio::time::sleep;

use super::harness::ClusterTestHarness;

const KEY: &[u8] = b"linearizable-k1";
const VALUE: &[u8] = b"linearizable-v1";

/// 启用 linearizable 后, leader 上 put/get 仍然成功 (走 quorum 确认读, 而非本地
/// leader check).
#[tokio::test]
async fn test_linearizable_get_on_leader_returns_value() {
    let harness = ClusterTestHarness::new_3node_linearizable().await;
    harness.bootstrap().await;
    let leader = harness.leader().await;

    leader.put(KEY.to_vec(), VALUE.to_vec()).await.unwrap();

    let got = leader.get(KEY.to_vec()).await.unwrap();
    assert_eq!(got, Some(VALUE.to_vec()));

    harness.shutdown_all().await;
}

/// 启用后 follower 上 `get` 必须返回 `NotLeader`, 且 `leader_addr` 为
/// `Some(_)` (openraft `ensure_linearizable` 的 `ForwardToLeader` 携带真实
/// leader 地址). `ensure_linearizable` 在 leader 刚选出/正在过渡时可能先返回
/// `Internal`, 故短重试 (最多约 1s) 再断言.
#[tokio::test]
async fn test_linearizable_follower_not_leader_has_leader_addr() {
    let harness = ClusterTestHarness::new_3node_linearizable().await;
    harness.bootstrap().await;
    assert!(harness.wait_leader().await.is_some());

    let leader_id = harness.wait_leader().await.expect("leader must be elected");
    let follower = harness
        .nodes
        .iter()
        .find(|n| n.node_id() != leader_id)
        .expect("3 节点集群必有非 leader 节点")
        .clone();

    let mut leader_addr = None;
    for _ in 0..10 {
        match follower.get(KEY.to_vec()).await {
            Err(Error::Cluster(ClusterError::NotLeader {
                leader_addr: addr, ..
            })) => {
                leader_addr = Some(addr);
                break;
            }
            Err(Error::Cluster(ClusterError::Internal(_))) => {
                sleep(Duration::from_millis(100)).await;
                continue;
            }
            other => panic!("follower get 期望 NotLeader 或过渡期 Internal, 实际: {other:?}"),
        }
    }

    let leader_addr = leader_addr.expect("重试约 1s 后仍未得到 NotLeader, 集群未稳定");
    assert!(
        leader_addr.is_some(),
        "linearizable_read=true 时 follower NotLeader.leader_addr 应为 Some(_), 实际: {leader_addr:?}"
    );

    harness.shutdown_all().await;
}

/// 对照: 默认 `linearizable_read = false` 时, follower 走本地 leader check
/// (`ensure_leader_for_linear_read` else 分支), `NotLeader.leader_addr` 恒为
/// `None`.
#[tokio::test]
async fn test_default_follower_not_leader_addr_is_none() {
    let harness = ClusterTestHarness::new_3node().await;
    harness.bootstrap().await;
    let leader_id = harness.wait_leader().await.expect("leader must be elected");

    let follower = harness
        .nodes
        .iter()
        .find(|n| n.node_id() != leader_id)
        .expect("3 节点集群必有非 leader 节点");

    match follower.get(KEY.to_vec()).await {
        Err(Error::Cluster(ClusterError::NotLeader { leader_addr, .. })) => {
            assert_eq!(
                leader_addr, None,
                "linearizable_read=false 时 follower NotLeader.leader_addr 应为 None, 实际: {leader_addr:?}"
            );
        }
        other => panic!("follower get 期望 NotLeader, 实际: {other:?}"),
    }

    harness.shutdown_all().await;
}

/// 对照: 默认路径下 leader put/get 仍成功 (关闭 linearizable_read 时行为不变).
#[tokio::test]
async fn test_default_leader_get_still_works() {
    let harness = ClusterTestHarness::new_3node().await;
    harness.bootstrap().await;
    let leader = harness.leader().await;

    leader.put(KEY.to_vec(), VALUE.to_vec()).await.unwrap();

    let got = leader.get(KEY.to_vec()).await.unwrap();
    assert_eq!(got, Some(VALUE.to_vec()));

    harness.shutdown_all().await;
}
