//! gRPC 网络层 — Raft 对等 RPC (vote / append_entries / full_snapshot) 与扩展
//! RPC (get_key / migration tip+tombstone / remote_propose) 的 client / server,
//! 以及按 `group_id` 分发的统一 dispatcher. 一个端口承载 MetaRaft (gid=0) 与
//! 全部数据 Group (gid≥1).
//!
//! # 数据流
//!
//! ```text
//! 本地 openraft::Raft core
//!   └─ RaftNetworkClientFactory::new_client(target, node)
//!        ├─ channel 连接池 (DashMap 按 node_id 缓存, 复用 TCP/HTTP2)
//!        └─ RaftNetworkClient (携带 group_id)
//!             └─ gRPC -> 对端 RaftServiceImpl (RaftServiceDispatcher)
//!                  ├─ dispatcher.get_raft(group_id)   -> vote/append/snapshot
//!                  └─ dispatcher.get_node(group_id)   -> get_key/migration/remote_propose
//! ```
//!
//! # Invariant
//!
//! - Raft RPC 一律发往 `rpc_addr` (BasicNode.address 字段); `client_addr` 仅供
//!   客户端 MOVED 重定向, 不参与 Raft 共识通信.
//! - 连接复用: channel 按 node_id 缓存, 避免 Docker DNS 解析的 50-100ms 开销;
//!   `add_node` 时清除旧 channel 以拾取地址变更.
//! - 超时 / Unavailable 归类为不确定错误 (RPCError), 不伪装成确定性结果.
//! - channel 失效策略: 仅 `Unavailable` (连接级失败) 失效缓存重建连接, 绕开
//!   tonic 指数退避; 超时与应用级错误 (Internal 等) 连接仍完好, 不失效, 避免
//!   选主风暴下重连风暴放大为选举活锁. 详见 `invalidate_on_unavailable`.
//! - 快速失败: 所有 RPC (含 lazy connect) 均受 `request_timeout` 约束, 静默黑洞
//!   (丢包无 RST) 下 connect 不会依赖内核 SYN 重传长时间挂起.

use dashmap::DashMap;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tonic::Request as TonicRequest;

use openraft::{
    error::{NetworkError, RPCError, Unreachable},
    network::RPCOption,
    raft::{
        AppendEntriesRequest, AppendEntriesResponse, SnapshotResponse, VoteRequest, VoteResponse,
    },
    RaftNetworkFactory, RaftNetworkV2,
};

use tracing::instrument;

use crate::cluster::node::OpenRaftNode;
use crate::cluster::storage::OpenRaftStorage;
use crate::cluster::types::{ClusterError, NodeId, Request, Response, TypeConfig};
use crate::error::{Error, Result};

#[allow(clippy::all)]
pub mod raft_rpc {
    include!("aidb.raft.rs");
}

use raft_rpc::raft_service_client::RaftServiceClient;

/// 根据 RPC 中携带的 `committed` 标志构造 `Vote`.
///
/// 注意: 必须尊重对端传来的 committed 状态, 不能无条件 `new_committed`.
/// candidate 发起选举时其 vote 是 uncommitted, 若误标为 committed 会让
/// 其他节点进入 leader-lease 保护而拒绝投票, 导致选举死锁.
fn vote_from_wire<LID>(
    term: LID::Term,
    node_id: LID::NodeId,
    committed: bool,
) -> openraft::Vote<LID>
where
    LID: openraft::vote::RaftLeaderId,
{
    if committed {
        openraft::Vote::new_committed(term, node_id)
    } else {
        openraft::Vote::new(term, node_id)
    }
}

pub struct RaftNetworkClient {
    #[cfg_attr(not(feature = "cluster-test-util"), allow(dead_code))]
    node_id: NodeId,
    /// RPC 目标节点 ID — 连接池缓存的 key, 网络失败时据此失效重建.
    target: NodeId,
    target_addr: String,
    client: Option<RaftServiceClient<tonic::transport::Channel>>,
    /// 共享 gRPC 连接池 (与 `RaftNetworkClientFactory` 同一实例).
    /// 网络失败时移除对应 entry, 使下个 RPC 重建 channel 并重新解析地址.
    channels: Option<Arc<DashMap<NodeId, RaftServiceClient<tonic::transport::Channel>>>>,
    request_timeout: Duration,
    group_id: u64,
    max_message_size: usize,
}

impl RaftNetworkClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        node_id: NodeId,
        target: NodeId,
        target_addr: String,
        group_id: u64,
        rpc_timeout_ms: u64,
        max_message_size: u64,
        channel: Option<RaftServiceClient<tonic::transport::Channel>>,
        channels: Option<Arc<DashMap<NodeId, RaftServiceClient<tonic::transport::Channel>>>>,
    ) -> Self {
        Self {
            node_id,
            target,
            target_addr,
            client: channel,
            channels,
            request_timeout: Duration::from_millis(rpc_timeout_ms),
            group_id,
            max_message_size: max_message_size as usize,
        }
    }

    /// 对端网络不可达时失效连接池缓存, 迫使下个 RPC 重新解析地址并建连.
    ///
    /// tonic/gRPC channel 内置指数退避 (默认 1s→120s), 长分区结束后若不重建
    /// channel, 节点恢复同步要等退避倒计时跑完 (实测 ~100s). 每次网络失败即
    /// 失效缓存, 自愈收敛到下一个心跳周期 (~500ms).
    fn invalidate_channel(&mut self) {
        if let Some(map) = &self.channels {
            map.remove(&self.target);
        }
        self.client = None;
    }

    /// 仅当连接级失败 (`tonic::Code::Unavailable`) 时失效连接池缓存.
    ///
    /// 分区断网后 tonic 会在连接关闭时返回 `Unavailable`, 失效缓存让下个 RPC
    /// 重建 channel, 绕开指数退避快速自愈; 而**超时**与**应用级错误** (Internal /
    /// FailedPrecondition 等) 通常表示连接完好、只是对端处理慢 (选主风暴 / DB
    /// 阻塞), 失效缓存反而触发 DNS 解析 + TCP/HTTP2 握手的重连风暴, 让健康节点
    /// 之间的 vote 心跳雪上加霜 (实测演化为 ~3min 选举活锁). 因此只有
    /// `Unavailable` 视为连接损坏并失效缓存, 其余一律保留.
    fn invalidate_on_unavailable(&mut self, status: &tonic::Status) {
        if status.code() == tonic::Code::Unavailable {
            self.invalidate_channel();
        }
    }

    #[cfg(test)]
    pub fn target_addr(&self) -> &str {
        &self.target_addr
    }

    /// FIX-0056-A1: 跨节点合并读 — 向本 client 的目标节点 (预期为 group
    /// leader) 请求指定 key 的当前值. 与 `append_entries`/`vote` 共用同一
    /// gRPC 通道/连接池/超时配置.
    ///
    /// 超时映射为 `ClusterError::Timeout`, 调用方 (`MultiRaftNode`) 必须
    /// 将其视为不确定结果 (向客户端 TRYAGAIN), **不得** 静默当作 key 不存在.
    pub async fn get_key(&mut self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let req_timeout = self.request_timeout;
        let group_id = self.group_id;
        let client = self
            .get_client()
            .await
            .map_err(|e| Error::Cluster(ClusterError::Raft(e.to_string())))?;

        let request = raft_rpc::GetKeyRequest {
            group_id,
            key: key.to_vec(),
        };
        let response = match timeout(req_timeout, client.get_key(TonicRequest::new(request))).await
        {
            Ok(Ok(r)) => r,
            Ok(Err(status)) => {
                self.invalidate_on_unavailable(&status);
                return Err(map_rpc_status_error("GetKey", status));
            }
            Err(_) => {
                return Err(Error::Cluster(ClusterError::Timeout(format!(
                    "GetKey timeout after {}ms",
                    req_timeout.as_millis()
                ))));
            }
        };
        let resp = response.into_inner();
        Ok(resp.found.then_some(resp.value))
    }

    /// FIX-0056-A1: 跨节点读取 `mig/{group}/{epoch}/tip` — `read_migration_tip`
    /// 在 group 非本地时的远程 fallback, 语义与 `get_key` 一致.
    pub async fn get_migration_tip(&mut self, epoch: u64) -> Result<u64> {
        let req_timeout = self.request_timeout;
        let group_id = self.group_id;
        let client = self
            .get_client()
            .await
            .map_err(|e| Error::Cluster(ClusterError::Raft(e.to_string())))?;

        let request = raft_rpc::GetMigrationTipRequest { group_id, epoch };
        let response = match timeout(
            req_timeout,
            client.get_migration_tip(TonicRequest::new(request)),
        )
        .await
        {
            Ok(Ok(r)) => r,
            Ok(Err(status)) => {
                self.invalidate_on_unavailable(&status);
                return Err(map_rpc_status_error("GetMigrationTip", status));
            }
            Err(_) => {
                return Err(Error::Cluster(ClusterError::Timeout(format!(
                    "GetMigrationTip timeout after {}ms",
                    req_timeout.as_millis()
                ))));
            }
        };
        Ok(response.into_inner().tip)
    }

    /// FIX-0056-A1 合并读线性点第 1 步: 跨节点读取 target group 上 `epoch`
    /// 内 `key` 的迁移 tombstone. 语义与 `get_key`/`get_migration_tip` 一致
    /// (同一 gRPC 通道, 超时映射为 `ClusterError::Timeout`).
    pub async fn get_migration_tombstone(
        &mut self,
        epoch: u64,
        key: &[u8],
    ) -> Result<Option<crate::cluster::migration_oplog::MigOp>> {
        let req_timeout = self.request_timeout;
        let group_id = self.group_id;
        let client = self
            .get_client()
            .await
            .map_err(|e| Error::Cluster(ClusterError::Raft(e.to_string())))?;

        let request = raft_rpc::GetMigrationTombstoneRequest {
            group_id,
            epoch,
            key: key.to_vec(),
        };
        let response = match timeout(
            req_timeout,
            client.get_migration_tombstone(TonicRequest::new(request)),
        )
        .await
        {
            Ok(Ok(r)) => r,
            Ok(Err(status)) => {
                self.invalidate_on_unavailable(&status);
                return Err(map_rpc_status_error("GetMigrationTombstone", status));
            }
            Err(_) => {
                return Err(Error::Cluster(ClusterError::Timeout(format!(
                    "GetMigrationTombstone timeout after {}ms",
                    req_timeout.as_millis()
                ))));
            }
        };
        let tag = response.into_inner().op_tag;
        Ok(crate::cluster::migration_oplog::MigOp::from_tag(tag as u8))
    }

    /// 跨节点迁移写: 向目标 group leader 节点 propose 一条数据面请求.
    ///
    /// 用于 `propose_group` 在 group 非本地时的远程 fallback (在线 slot
    /// 迁移的 `PutConditional` 全量拷贝 / `MigrationBarrier` 写屏障等必须
    /// 落到持有 target group 的节点). 请求/响应以 bincode 序列化, 与
    /// `GetKey` 共用同一 gRPC 通道与超时语义; 对端 `NotLeader` 映射为
    /// `failed_precondition`, 由调用方决定转发/重试.
    pub async fn remote_propose(&mut self, group_id: u64, request: &Request) -> Result<Response> {
        let req_timeout = self.request_timeout;
        let client = self
            .get_client()
            .await
            .map_err(|e| Error::Cluster(ClusterError::Raft(e.to_string())))?;

        let payload = bincode::serialize(request)
            .map_err(|e| Error::Cluster(ClusterError::Internal(e.to_string())))?;
        let request = raft_rpc::RemoteProposeRequest {
            group_id,
            request: payload,
        };
        let response = match timeout(
            req_timeout,
            client.remote_propose(TonicRequest::new(request)),
        )
        .await
        {
            Ok(Ok(r)) => r,
            Ok(Err(status)) => {
                self.invalidate_on_unavailable(&status);
                return Err(map_rpc_status_error("RemotePropose", status));
            }
            Err(_) => {
                return Err(Error::Cluster(ClusterError::Timeout(format!(
                    "RemotePropose timeout after {}ms",
                    req_timeout.as_millis()
                ))));
            }
        };
        let payload = response.into_inner().response;
        bincode::deserialize(&payload)
            .map_err(|e| Error::Cluster(ClusterError::Internal(e.to_string())))
    }

    async fn get_client(
        &mut self,
    ) -> std::result::Result<
        &mut RaftServiceClient<tonic::transport::Channel>,
        NetworkError<TypeConfig>,
    > {
        if self.client.is_none() {
            // Fallback: lazy connect (backward compat, 缓存未命中时)
            // connect 纳入 request_timeout 保护: 静默黑洞分区 (丢包无 RST) 下
            // tonic connect 依赖内核 SYN 重传, 可能挂起远超 RPC 超时, 破坏
            // 快速失败不变量. 超时后由调用方失效缓存, 下个 RPC 重试.
            tracing::debug!(
              target_addr = %self.target_addr,
              group_id = self.group_id,
              "gRPC: lazy connect (channel not cached)",
            );
            let req_timeout = self.request_timeout;
            let target_addr = self.target_addr.clone();
            let connect_fut = RaftServiceClient::connect(target_addr);
            let client = match tokio::time::timeout(req_timeout, connect_fut).await {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => {
                    tracing::warn!(
                      target_addr = %self.target_addr,
                      error = %e,
                      "gRPC client connect failed",
                    );
                    return Err(NetworkError::<TypeConfig>::new(
                        &Unreachable::<TypeConfig>::new(&e),
                    ));
                }
                Err(_) => {
                    let io_err = std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("gRPC connect timeout after {}ms", req_timeout.as_millis()),
                    );
                    return Err(NetworkError::<TypeConfig>::new(
                        &Unreachable::<TypeConfig>::new(&io_err),
                    ));
                }
            }
            .max_decoding_message_size(self.max_message_size)
            .max_encoding_message_size(self.max_message_size);
            self.client = Some(client);
        }
        Ok(self.client.as_mut().unwrap())
    }
}

impl RaftNetworkV2<TypeConfig> for RaftNetworkClient {
    type SnapshotData = std::io::Cursor<Vec<u8>>;

    #[instrument(level = "debug", name = "raft_rpc_ae", skip(self, rpc, _option))]
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> std::result::Result<AppendEntriesResponse<TypeConfig>, RPCError<TypeConfig>> {
        #[cfg(feature = "cluster-test-util")]
        if crate::cluster::failpoint::blackhole_active(self.node_id, &self.target_addr) {
            return Err(RPCError::Network(NetworkError::<TypeConfig>::from_string(
                "blackhole",
            )));
        }
        #[cfg(feature = "monitoring")]
        crate::cluster::metrics::record_raft_rpc("append_entries", "outgoing");
        let req_timeout = self.request_timeout;
        let group_id = self.group_id;
        let client = match self.get_client().await {
            Ok(c) => c,
            Err(e) => {
                self.invalidate_channel();
                return Err(RPCError::Network(e));
            }
        };

        let mut entries = Vec::new();
        for entry in rpc.entries {
            let payload = rmp_serde::to_vec(&entry.payload).map_err(|e| {
                RPCError::Network(NetworkError::<TypeConfig>::new(
                    &Unreachable::<TypeConfig>::new(&e),
                ))
            })?;
            entries.push(raft_rpc::LogEntry {
                log_index: entry.log_id.index,
                log_term: entry.log_id.leader_id.term,
                log_leader_id: 0, // v0.10 std mode CommittedLeaderId only has term
                payload,
                is_blank: matches!(entry.payload, openraft::EntryPayload::Blank),
                is_membership: matches!(entry.payload, openraft::EntryPayload::Membership(_)),
            });
        }

        let request = raft_rpc::AppendEntriesRequest {
            group_id,
            vote_term: rpc.vote.leader_id.term,
            vote_node_id: rpc.vote.leader_id.voted_for,
            vote_committed: rpc.vote.committed,
            prev_log_index: rpc.prev_log_id.map(|id| id.index),
            prev_log_term: rpc.prev_log_id.map(|id| id.leader_id.term),
            prev_log_leader_id: rpc.prev_log_id.map(|_id| 0), // v0.10 CommittedLeaderId has no node_id
            entries,
            leader_commit_index: rpc.leader_commit.map(|id| id.index),
            leader_commit_term: rpc.leader_commit.map(|id| id.leader_id.term),
            leader_commit_leader_id: rpc.leader_commit.map(|_id| 0), // v0.10 CommittedLeaderId has no node_id
        };

        let rpc_future = client.append_entries(TonicRequest::new(request));
        let t0 = std::time::Instant::now();
        let response = match timeout(req_timeout, rpc_future).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                self.invalidate_on_unavailable(&e);
                return Err(RPCError::Network(NetworkError::<TypeConfig>::new(
                    &Unreachable::<TypeConfig>::new(&e),
                )));
            }
            Err(_) => {
                let io_err = std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("AppendEntries timeout after {}ms", req_timeout.as_millis()),
                );
                return Err(RPCError::Network(NetworkError::<TypeConfig>::new(
                    &Unreachable::<TypeConfig>::new(&io_err),
                )));
            }
        };
        tracing::info!(
            target: "perf",
            group_id,
            grpc_us = t0.elapsed().as_micros(),
            "raft_rpc_ae_done"
        );

        let resp = response.into_inner();
        if resp.success {
            Ok(AppendEntriesResponse::Success)
        } else if resp.vote_term > 0 || resp.vote_node_id > 0 {
            // HigherVote: the follower has seen a higher term. The leader must step down.
            tracing::info!(
                target = display(&self.target_addr),
                vote_term = resp.vote_term,
                vote_node_id = resp.vote_node_id,
                "received HigherVote from follower, leader must step down"
            );
            let vote = vote_from_wire(resp.vote_term, resp.vote_node_id, resp.vote_committed);
            Ok(AppendEntriesResponse::HigherVote(vote))
        } else {
            Ok(AppendEntriesResponse::Conflict)
        }
    }

    #[instrument(
        name = "raft_rpc_full_snapshot",
        skip(self, vote, snapshot, cancel, _option)
    )]
    async fn full_snapshot(
        &mut self,
        vote: openraft::alias::VoteOf<TypeConfig>,
        snapshot: openraft::alias::SnapshotOf<TypeConfig, Self::SnapshotData>,
        cancel: impl std::future::Future<Output = openraft::error::ReplicationClosed>
            + openraft::OptionalSend
            + 'static,
        _option: RPCOption,
    ) -> std::result::Result<
        SnapshotResponse<TypeConfig>,
        openraft::error::StreamingError<TypeConfig>,
    > {
        #[cfg(feature = "cluster-test-util")]
        if crate::cluster::failpoint::blackhole_active(self.node_id, &self.target_addr) {
            return Err(openraft::error::StreamingError::Network(NetworkError::<
                TypeConfig,
            >::from_string(
                "blackhole"
            )));
        }
        drop(cancel); // Not using cancellation in this implementation
        let req_timeout = self.request_timeout;
        let group_id = self.group_id;
        let client = match self.get_client().await {
            Ok(c) => c,
            Err(e) => {
                self.invalidate_channel();
                return Err(openraft::error::StreamingError::Network(e));
            }
        };

        let meta = raft_rpc::SnapshotMeta {
            last_log_index: snapshot.meta.last_log_id.map(|id| id.index),
            last_log_term: snapshot.meta.last_log_id.map(|id| id.leader_id.term),
            last_log_leader_id: snapshot.meta.last_log_id.map(|id| id.leader_id.term),
            last_membership: bincode::serialize(&snapshot.meta.last_membership).map_err(|e| {
                openraft::error::StreamingError::Network(NetworkError::<TypeConfig>::new(
                    &Unreachable::<TypeConfig>::new(&e),
                ))
            })?,
            snapshot_id: snapshot
                .meta
                .last_log_id
                .map(|id| format!("snap-{}", id.index))
                .unwrap_or_default(),
        };

        let data = snapshot.snapshot.into_inner();
        let request = raft_rpc::InstallSnapshotRequest {
            group_id,
            vote_term: vote.leader_id.term,
            vote_node_id: vote.leader_id.voted_for,
            vote_committed: vote.committed,
            meta: Some(meta),
            snapshot_data: data,
        };

        let rpc_future = client.install_snapshot(TonicRequest::new(request));
        let response = match timeout(req_timeout, rpc_future).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                self.invalidate_on_unavailable(&e);
                if e.code() == tonic::Code::Unavailable {
                    return Err(openraft::error::StreamingError::Unreachable(Unreachable::<
                        TypeConfig,
                    >::new(
                        &e
                    )));
                }
                return Err(openraft::error::StreamingError::Network(NetworkError::<
                    TypeConfig,
                >::new(
                    &e
                )));
            }
            Err(_) => {
                let io_err = std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "InstallSnapshot timeout after {}ms",
                        req_timeout.as_millis()
                    ),
                );
                return Err(openraft::error::StreamingError::Unreachable(Unreachable::<
                    TypeConfig,
                >::new(
                    &io_err
                )));
            }
        };

        let resp = response.into_inner();
        Ok(SnapshotResponse {
            vote: vote_from_wire(resp.vote_term, resp.vote_node_id, resp.vote_committed),
        })
    }

    #[instrument(level = "debug", name = "raft_rpc_vote", skip(self, rpc, _option))]
    async fn vote(
        &mut self,
        rpc: VoteRequest<TypeConfig>,
        _option: RPCOption,
    ) -> std::result::Result<VoteResponse<TypeConfig>, RPCError<TypeConfig>> {
        #[cfg(feature = "cluster-test-util")]
        if crate::cluster::failpoint::blackhole_active(self.node_id, &self.target_addr) {
            return Err(RPCError::Network(NetworkError::<TypeConfig>::from_string(
                "blackhole",
            )));
        }
        #[cfg(feature = "monitoring")]
        crate::cluster::metrics::record_raft_rpc("vote", "outgoing");
        let req_timeout = self.request_timeout;
        let group_id = self.group_id;
        let client = match self.get_client().await {
            Ok(c) => c,
            Err(e) => {
                self.invalidate_channel();
                return Err(RPCError::Network(e));
            }
        };

        let request = raft_rpc::VoteRequest {
            group_id,
            vote_term: rpc.vote.leader_id.term,
            vote_node_id: rpc.vote.leader_id.voted_for,
            vote_committed: rpc.vote.committed,
            last_log_index: rpc.last_log_id.map(|id| id.index).unwrap_or(0),
            last_log_term: rpc.last_log_id.map(|id| id.leader_id.term).unwrap_or(0),
            last_log_leader_id: rpc.last_log_id.map(|_id| 0).unwrap_or(0),
        };

        let rpc_future = client.vote(TonicRequest::new(request));
        let response = match timeout(req_timeout, rpc_future).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                self.invalidate_on_unavailable(&e);
                return Err(RPCError::Network(NetworkError::<TypeConfig>::new(&e)));
            }
            Err(_) => {
                let io_err = std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("Vote timeout after {}ms", req_timeout.as_millis()),
                );
                return Err(RPCError::Network(NetworkError::<TypeConfig>::new(
                    &Unreachable::<TypeConfig>::new(&io_err),
                )));
            }
        };

        let resp = response.into_inner();
        Ok(VoteResponse {
            vote: vote_from_wire(resp.vote_term, resp.vote_node_id, resp.vote_committed),
            vote_granted: resp.vote_granted,
            last_log_id: None,
        })
    }
}

/// 将 `GetKey`/`GetMigrationTip` 的 `tonic::Status` 映射为 `ClusterError`.
/// `FailedPrecondition` (对端非 leader / linearizable 检查失败) 映射为
/// `NotLeader`, 其余映射为 `Raft` (保留原始 gRPC 错误信息, 不伪装成"确定性"
/// 结果, 呼应 A1"超时/失败禁止静默 fallback"不变式).
fn map_rpc_status_error(rpc_name: &str, status: tonic::Status) -> Error {
    match status.code() {
        tonic::Code::FailedPrecondition => Error::Cluster(ClusterError::NotLeader {
            leader: None,
            leader_addr: None,
            is_ask: false,
        }),
        tonic::Code::NotFound => Error::Cluster(ClusterError::Raft(format!(
            "{rpc_name}: {}",
            status.message()
        ))),
        _ => Error::Cluster(ClusterError::Raft(format!(
            "{rpc_name} failed: {}",
            status.message()
        ))),
    }
}

#[derive(Clone)]
pub struct RaftNetworkClientFactory {
    node_id: NodeId,
    group_id: u64,
    rpc_timeout_ms: u64,
    max_message_size: u64,
    nodes: Arc<RwLock<HashMap<NodeId, String>>>,
    /// gRPC channel 连接池 — 按 node_id 缓存, 避免每次 RPC 都重新 TCP + HTTP/2 握手.
    /// Docker 容器 DNS 解析 (aikv-N) 每次 50-100ms, 复用 channel 可消除此开销.
    channels: Arc<DashMap<NodeId, RaftServiceClient<tonic::transport::Channel>>>,
}

impl RaftNetworkClientFactory {
    pub fn new(node_id: NodeId, group_id: u64, rpc_timeout_ms: u64, max_message_size: u64) -> Self {
        Self {
            node_id,
            group_id,
            rpc_timeout_ms,
            max_message_size,
            nodes: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(DashMap::new()),
        }
    }

    pub fn with_group_id(&self, group_id: u64) -> Self {
        Self {
            node_id: self.node_id,
            group_id,
            rpc_timeout_ms: self.rpc_timeout_ms,
            max_message_size: self.max_message_size,
            nodes: Arc::clone(&self.nodes),
            channels: Arc::clone(&self.channels),
        }
    }

    pub fn add_node(&self, node_id: NodeId, address: String) {
        self.nodes.write().insert(node_id, address);
        // 地址变更时清除旧 channel, 下次 RPC 自动重连
        self.channels.remove(&node_id);
    }

    pub fn remove_node(&self, node_id: NodeId) {
        self.nodes.write().remove(&node_id);
        self.channels.remove(&node_id);
    }

    pub fn list_nodes(&self) -> Vec<(NodeId, String)> {
        self.nodes
            .read()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    /// 获取或创建 gRPC channel — 缓存复用避免频繁 TCP 握手.
    async fn get_or_create_channel(
        &self,
        target: NodeId,
        addr: &str,
        max_message_size: usize,
    ) -> std::result::Result<RaftServiceClient<tonic::transport::Channel>, NetworkError<TypeConfig>>
    {
        // DashMap 内置分片锁, get/insert 无全局锁竞争
        if let Some(client) = self.channels.get(&target) {
            return Ok(client.clone());
        }
        // Slow path: connect + cache (多个并发 miss 可能重复 connect,
        // 后 insert 覆盖前 insert; tonic Channel 基于 Arc, 旧的 clone 不受影响)
        let normalized = normalize_grpc_addr(addr);
        tracing::debug!(%target, %normalized, "gRPC: establishing new channel for peer");
        let req_timeout = Duration::from_millis(self.rpc_timeout_ms);
        let connect_fut = RaftServiceClient::connect(normalized);
        let client = match tokio::time::timeout(req_timeout, connect_fut).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                return Err(NetworkError::<TypeConfig>::new(
                    &Unreachable::<TypeConfig>::new(&e),
                ))
            }
            Err(_) => {
                let io_err = std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("gRPC connect timeout after {}ms", req_timeout.as_millis()),
                );
                return Err(NetworkError::<TypeConfig>::new(
                    &Unreachable::<TypeConfig>::new(&io_err),
                ));
            }
        }
        .max_decoding_message_size(max_message_size)
        .max_encoding_message_size(max_message_size);
        self.channels.insert(target, client.clone());
        Ok(client)
    }
}

/// Ensure a gRPC address has a proper URI scheme (http://) for tonic.
/// Tonic's `Channel::from_shared` requires a valid URI; bare addresses
/// like `127.0.0.1:20349` are rejected.
fn normalize_grpc_addr(addr: &str) -> String {
    if addr.is_empty() {
        return String::new();
    }
    if addr.contains("://") {
        addr.to_string()
    } else {
        format!("http://{}", addr)
    }
}

impl RaftNetworkFactory<TypeConfig> for RaftNetworkClientFactory {
    type Network = RaftNetworkClient;

    async fn new_client(&mut self, target: NodeId, node: &openraft::BasicNode) -> Self::Network {
        // 解析目标地址
        let raw_addr = if !node.addr.is_empty() {
            let addr = node.addr.clone();
            self.nodes.write().insert(target, addr.clone());
            addr
        } else {
            self.nodes.read().get(&target).cloned().unwrap_or_else(|| {
                tracing::error!(
                    target_node_id = target,
                    "gRPC address for node {} is not registered",
                    target,
                );
                String::new()
            })
        };

        let target_addr = normalize_grpc_addr(&raw_addr);

        // 从连接池获取或创建 channel, 避免每次 RPC 都重新 TCP + HTTP/2 握手
        let channel = match self
            .get_or_create_channel(target, &raw_addr, self.max_message_size as usize)
            .await
        {
            Ok(ch) => Some(ch),
            Err(e) => {
                tracing::warn!(%target, error = %e, "gRPC: channel creation failed, will lazy connect on RPC");
                None
            }
        };

        RaftNetworkClient::new(
            self.node_id,
            target,
            target_addr,
            self.group_id,
            self.rpc_timeout_ms,
            self.max_message_size,
            channel,
            Some(self.channels.clone()),
        )
    }
}

/// 多 Group Raft gRPC 服务调度器
/// 所有数据 Group 共享此实例, 通过 group_id 分发 RPC
pub struct RaftServiceDispatcher {
    #[allow(clippy::type_complexity)]
    groups: Arc<RwLock<HashMap<u64, Arc<openraft::Raft<TypeConfig, OpenRaftStorage>>>>>,
    /// FIX-0056-A1: `GetKey`/`GetMigrationTip` 需要 `OpenRaftNode` 级别的
    /// leader-check / linearizable 读语义 (`get()`/`get_migration_tip()`),
    /// 而不仅是裸 `openraft::Raft`. 与 `groups` 分开维护是为了不影响现有
    /// Vote/AppendEntries/InstallSnapshot 调用点 (`register_group` 签名不变);
    /// 仅 `MultiRaftNode` 组装 group 时额外调用 `register_node`.
    nodes: Arc<RwLock<HashMap<u64, Arc<OpenRaftNode>>>>,
}

impl RaftServiceDispatcher {
    pub fn new() -> Self {
        Self {
            groups: Arc::new(RwLock::new(HashMap::new())),
            nodes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register_group(
        &self,
        group_id: u64,
        raft: Arc<openraft::Raft<TypeConfig, OpenRaftStorage>>,
    ) {
        self.groups.write().insert(group_id, raft);
    }

    /// 注册 `OpenRaftNode` 句柄, 供 `GetKey`/`GetMigrationTip` 服务端处理
    /// 复用其 leader-check / linearizable 读逻辑 (与 `register_group` 分开
    /// 调用, 通常紧跟其后).
    pub fn register_node(&self, group_id: u64, node: Arc<OpenRaftNode>) {
        self.nodes.write().insert(group_id, node);
    }

    pub fn unregister_group(&self, group_id: u64) {
        self.groups.write().remove(&group_id);
        self.nodes.write().remove(&group_id);
    }

    pub fn get_raft(
        &self,
        group_id: u64,
    ) -> Option<Arc<openraft::Raft<TypeConfig, OpenRaftStorage>>> {
        self.groups.read().get(&group_id).cloned()
    }

    pub fn get_node(&self, group_id: u64) -> Option<Arc<OpenRaftNode>> {
        self.nodes.read().get(&group_id).cloned()
    }

    pub fn group_count(&self) -> usize {
        self.groups.read().len()
    }
}

impl Default for RaftServiceDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RaftServiceImpl {
    dispatcher: Arc<RaftServiceDispatcher>,
}

impl RaftServiceImpl {
    pub fn new(dispatcher: Arc<RaftServiceDispatcher>) -> Self {
        Self { dispatcher }
    }
}

use raft_rpc::raft_service_server::RaftService;

#[tonic::async_trait]
impl RaftService for RaftServiceImpl {
    async fn vote(
        &self,
        request: TonicRequest<raft_rpc::VoteRequest>,
    ) -> std::result::Result<tonic::Response<raft_rpc::VoteResponse>, tonic::Status> {
        #[cfg(feature = "monitoring")]
        crate::cluster::metrics::record_raft_rpc("vote", "incoming");
        let req = request.into_inner();
        let last_log_id = if req.last_log_index > 0 {
            Some(openraft::LogId::new(
                openraft::vote::leader_id_std::CommittedLeaderId::new(req.last_log_term),
                req.last_log_index,
            ))
        } else {
            None
        };

        let vote_req = VoteRequest {
            vote: vote_from_wire(req.vote_term, req.vote_node_id, req.vote_committed),
            last_log_id,
            leadership_transfer: false,
        };

        let raft = self
            .dispatcher
            .get_raft(req.group_id)
            .ok_or_else(|| tonic::Status::not_found(format!("group {} not found", req.group_id)))?;
        let vote_resp = raft
            .vote(vote_req)
            .await
            .map_err(|e| tonic::Status::internal(format!("Vote failed: {e}")))?;

        Ok(tonic::Response::new(raft_rpc::VoteResponse {
            vote_term: vote_resp.vote.leader_id.term,
            vote_node_id: vote_resp.vote.leader_id.voted_for,
            vote_committed: vote_resp.vote.committed,
            vote_granted: vote_resp.vote_granted,
            is_in_membership: true,
        }))
    }

    async fn append_entries(
        &self,
        request: TonicRequest<raft_rpc::AppendEntriesRequest>,
    ) -> std::result::Result<tonic::Response<raft_rpc::AppendEntriesResponse>, tonic::Status> {
        #[cfg(feature = "monitoring")]
        crate::cluster::metrics::record_raft_rpc("append_entries", "incoming");
        let req = request.into_inner();
        #[cfg(feature = "monitoring")]
        crate::cluster::metrics::record_raft_log_entries(req.entries.len() as u64);

        tracing::debug!(
            group_id = req.group_id,
            entry_count = req.entries.len(),
            leader_commit = req.leader_commit_index,
            prev_log_index = req.prev_log_index,
            "gRPC: received AppendEntries",
        );

        let mut entries = Vec::new();
        for entry in req.entries {
            let payload: openraft::EntryPayload<Request, NodeId, openraft::BasicNode> =
                rmp_serde::from_slice(&entry.payload)
                    .map_err(|e| tonic::Status::internal(format!("deserialize entry: {e}")))?;
            entries.push(crate::cluster::types::LogEntry {
                log_id: openraft::LogId::new(
                    openraft::vote::leader_id_std::CommittedLeaderId::new(entry.log_term),
                    entry.log_index,
                ),
                payload,
            });
        }

        let prev_log_id = match (
            req.prev_log_index,
            req.prev_log_term,
            req.prev_log_leader_id,
        ) {
            (Some(index), Some(term), Some(_leader_id)) => Some(openraft::LogId::new(
                openraft::vote::leader_id_std::CommittedLeaderId::new(term),
                index,
            )),
            _ => None,
        };

        let leader_commit = match (
            req.leader_commit_index,
            req.leader_commit_term,
            req.leader_commit_leader_id,
        ) {
            (Some(index), Some(term), Some(_leader_id)) => Some(openraft::LogId::new(
                openraft::vote::leader_id_std::CommittedLeaderId::new(term),
                index,
            )),
            _ => None,
        };

        let append_req = AppendEntriesRequest {
            vote: vote_from_wire(req.vote_term, req.vote_node_id, req.vote_committed),
            prev_log_id,
            entries,
            leader_commit,
        };

        let raft = self
            .dispatcher
            .get_raft(req.group_id)
            .ok_or_else(|| tonic::Status::not_found(format!("group {} not found", req.group_id)))?;
        let append_resp = raft
            .append_entries(append_req)
            .await
            .map_err(|e| tonic::Status::internal(format!("AppendEntries failed: {e}")))?;

        let response = match append_resp {
            AppendEntriesResponse::Success | AppendEntriesResponse::PartialSuccess(_) => {
                raft_rpc::AppendEntriesResponse {
                    vote_term: 0,
                    vote_node_id: 0,
                    vote_committed: false,
                    success: true,
                    conflict_index: None,
                    conflict_term: None,
                }
            }
            AppendEntriesResponse::Conflict => raft_rpc::AppendEntriesResponse {
                vote_term: 0,
                vote_node_id: 0,
                vote_committed: false,
                success: false,
                conflict_index: Some(0),
                conflict_term: Some(0),
            },
            AppendEntriesResponse::HigherVote(vote) => raft_rpc::AppendEntriesResponse {
                vote_term: vote.leader_id.term,
                vote_node_id: vote.leader_id.voted_for,
                vote_committed: vote.committed,
                success: false,
                conflict_index: None,
                conflict_term: None,
            },
        };

        Ok(tonic::Response::new(response))
    }

    async fn install_snapshot(
        &self,
        request: TonicRequest<raft_rpc::InstallSnapshotRequest>,
    ) -> std::result::Result<tonic::Response<raft_rpc::InstallSnapshotResponse>, tonic::Status>
    {
        #[cfg(feature = "monitoring")]
        crate::cluster::metrics::record_raft_rpc("install_snapshot", "incoming");
        let req = request.into_inner();
        let meta = req
            .meta
            .ok_or_else(|| tonic::Status::invalid_argument("missing snapshot meta"))?;

        let last_log_id = match (
            meta.last_log_index,
            meta.last_log_term,
            meta.last_log_leader_id,
        ) {
            (Some(index), Some(term), Some(_leader_id)) => Some(openraft::LogId::new(
                openraft::vote::leader_id_std::CommittedLeaderId::new(term),
                index,
            )),
            _ => None,
        };

        let last_membership: openraft::StoredMembership<
            openraft::vote::leader_id_std::CommittedLeaderId<u64>,
            NodeId,
            openraft::BasicNode,
        > = bincode::deserialize(&meta.last_membership)
            .map_err(|e| tonic::Status::internal(format!("membership decode: {e}")))?;

        let snapshot_meta = openraft::SnapshotMeta {
            last_log_id,
            last_membership,
        };

        let vote = vote_from_wire(req.vote_term, req.vote_node_id, req.vote_committed);
        let snapshot = openraft::Snapshot {
            meta: snapshot_meta,
            snapshot: std::io::Cursor::new(req.snapshot_data),
        };

        let raft = self
            .dispatcher
            .get_raft(req.group_id)
            .ok_or_else(|| tonic::Status::not_found(format!("group {} not found", req.group_id)))?;
        let install_resp = raft
            .install_full_snapshot(vote, snapshot)
            .await
            .map_err(|e| tonic::Status::internal(format!("InstallSnapshot failed: {e}")))?;

        Ok(tonic::Response::new(raft_rpc::InstallSnapshotResponse {
            vote_term: install_resp.vote.leader_id.term,
            vote_node_id: install_resp.vote.leader_id.voted_for,
            vote_committed: install_resp.vote.committed,
        }))
    }

    async fn get_key(
        &self,
        request: TonicRequest<raft_rpc::GetKeyRequest>,
    ) -> std::result::Result<tonic::Response<raft_rpc::GetKeyResponse>, tonic::Status> {
        let req = request.into_inner();
        let node = self
            .dispatcher
            .get_node(req.group_id)
            .ok_or_else(|| tonic::Status::not_found(format!("group {} not found", req.group_id)))?;

        match node.get(req.key).await {
            Ok(Some(value)) => Ok(tonic::Response::new(raft_rpc::GetKeyResponse {
                found: true,
                value,
            })),
            Ok(None) => Ok(tonic::Response::new(raft_rpc::GetKeyResponse {
                found: false,
                value: Vec::new(),
            })),
            Err(e) => Err(map_node_error_to_status(e)),
        }
    }

    async fn get_migration_tip(
        &self,
        request: TonicRequest<raft_rpc::GetMigrationTipRequest>,
    ) -> std::result::Result<tonic::Response<raft_rpc::GetMigrationTipResponse>, tonic::Status>
    {
        let req = request.into_inner();
        let node = self
            .dispatcher
            .get_node(req.group_id)
            .ok_or_else(|| tonic::Status::not_found(format!("group {} not found", req.group_id)))?;

        match node.get_migration_tip(req.epoch).await {
            Ok(tip) => Ok(tonic::Response::new(raft_rpc::GetMigrationTipResponse {
                tip,
            })),
            Err(e) => Err(map_node_error_to_status(e)),
        }
    }

    async fn get_migration_tombstone(
        &self,
        request: TonicRequest<raft_rpc::GetMigrationTombstoneRequest>,
    ) -> std::result::Result<tonic::Response<raft_rpc::GetMigrationTombstoneResponse>, tonic::Status>
    {
        let req = request.into_inner();
        let node = self
            .dispatcher
            .get_node(req.group_id)
            .ok_or_else(|| tonic::Status::not_found(format!("group {} not found", req.group_id)))?;

        match node.get_migration_tombstone(req.epoch, req.key).await {
            Ok(op) => Ok(tonic::Response::new(
                raft_rpc::GetMigrationTombstoneResponse {
                    op_tag: op.map(|o| o.tag()).unwrap_or(0) as u32,
                },
            )),
            Err(e) => Err(map_node_error_to_status(e)),
        }
    }

    async fn remote_propose(
        &self,
        request: TonicRequest<raft_rpc::RemoteProposeRequest>,
    ) -> std::result::Result<tonic::Response<raft_rpc::RemoteProposeResponse>, tonic::Status> {
        let req = request.into_inner();
        let node = self
            .dispatcher
            .get_node(req.group_id)
            .ok_or_else(|| tonic::Status::not_found(format!("group {} not found", req.group_id)))?;

        let inner: Request = bincode::deserialize(&req.request)
            .map_err(|e| tonic::Status::invalid_argument(e.to_string()))?;
        match node.propose(inner).await {
            Ok(resp) => {
                let payload = bincode::serialize(&resp)
                    .map_err(|e| tonic::Status::internal(e.to_string()))?;
                Ok(tonic::Response::new(raft_rpc::RemoteProposeResponse {
                    response: payload,
                }))
            }
            Err(e) => Err(map_node_error_to_status(e)),
        }
    }
}

/// 将 `OpenRaftNode::get`/`get_migration_tip` 的错误映射为 `tonic::Status`.
/// `NotLeader` (含 linearizable 检查失败时的 `ForwardToLeader`) 映射为
/// `failed_precondition`, 供客户端 (`RaftNetworkClient::get_key` 等) 区分
/// "对端不是 leader" 与其它内部错误.
fn map_node_error_to_status(e: Error) -> tonic::Status {
    match e {
        Error::Cluster(ClusterError::NotLeader { .. }) => {
            tonic::Status::failed_precondition(e.to_string())
        }
        _ => tonic::Status::internal(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_network_factory_unknown_node_returns_empty_addr() {
        let mut factory = RaftNetworkClientFactory::new(1, 0, 100, 1024);
        let node = openraft::BasicNode { addr: "".into() };
        let client = factory.new_client(99, &node).await;
        assert!(
            client.target_addr().is_empty(),
            "expected empty addr for unknown node without cached address, but got: {}",
            client.target_addr(),
        );
    }

    /// 连接池失效策略: 仅 `Unavailable` (连接级失败) 失效缓存重建连接;
    /// 超时与应用级错误 (Internal) 视为连接仍完好, 不失效, 避免选主风暴下
    /// 重连风暴放大为选举活锁 (FIX-0065-A1 连接池策略).
    ///
    /// 通过 `channels` 池 + `client` 两个观察点验证:
    /// - `Unavailable` → 池 entry 被移除, 本地 client 置 None
    /// - `DeadlineExceeded` (超时) → 池 entry 保留, 本地 client 保留
    /// - `Internal` (应用级) → 池 entry 保留, 本地 client 保留
    #[tokio::test]
    async fn invalidate_on_unavailable_only_for_connection_failures() {
        let channels: Arc<DashMap<NodeId, RaftServiceClient<tonic::transport::Channel>>> =
            Arc::new(DashMap::new());
        // 占位 channel (lazy, 不建立真实连接): 仅用于观察池 entry 生命周期,
        // 验证失效判定逻辑, 不发起任何网络 I/O.
        let placeholder = RaftServiceClient::new(
            tonic::transport::Channel::from_static("http://127.0.0.1:1").connect_lazy(),
        );
        channels.insert(2, placeholder);

        let mut client = RaftNetworkClient::new(
            1,
            2,
            "http://127.0.0.1:1".to_string(),
            0,
            100,
            1024,
            None,
            Some(Arc::clone(&channels)),
        );

        // 基线: 池与本地 client 状态
        assert!(channels.contains_key(&2));

        // Unavailable → 失效
        client.invalidate_on_unavailable(&tonic::Status::unavailable("conn reset"));
        assert!(!channels.contains_key(&2), "Unavailable 应移除连接池 entry");
        assert!(client.client.is_none(), "Unavailable 应置本地 client None");

        // DeadlineExceeded (超时) → 不失效
        channels.insert(
            2,
            RaftServiceClient::new(
                tonic::transport::Channel::from_static("http://127.0.0.1:1").connect_lazy(),
            ),
        );
        client.invalidate_on_unavailable(&tonic::Status::deadline_exceeded("slow peer"));
        assert!(channels.contains_key(&2), "超时不应移除连接池 entry");

        // Internal (应用级) → 不失效
        client.invalidate_on_unavailable(&tonic::Status::internal("internal error"));
        assert!(channels.contains_key(&2), "应用级错误不应移除连接池 entry");
    }
}
