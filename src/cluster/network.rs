//! gRPC network layer for Raft.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tonic::Request as TonicRequest;

use openraft::{
    error::{NetworkError, RPCError, RaftError, Unreachable},
    network::RPCOption,
    raft::{
        AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest,
        InstallSnapshotResponse, VoteRequest, VoteResponse,
    },
    RaftNetwork, RaftNetworkFactory, SnapshotMeta,
};

use tracing::instrument;

use crate::cluster::types::{NodeId, TypeConfig};

#[allow(clippy::all)]
pub mod raft_rpc {
    include!("aidb.raft.rs");
}

use raft_rpc::raft_service_client::RaftServiceClient;

pub struct RaftNetworkClient {
    target_addr: String,
    client: Option<RaftServiceClient<tonic::transport::Channel>>,
    request_timeout: Duration,
    group_id: u64,
    max_message_size: usize,
}

impl RaftNetworkClient {
    pub fn new(
        _node_id: NodeId,
        _target: NodeId,
        target_addr: String,
        group_id: u64,
        rpc_timeout_ms: u64,
        max_message_size: u64,
        channel: Option<RaftServiceClient<tonic::transport::Channel>>,
    ) -> Self {
        Self {
            target_addr,
            client: channel,
            request_timeout: Duration::from_millis(rpc_timeout_ms),
            group_id,
            max_message_size: max_message_size as usize,
        }
    }

    #[cfg(test)]
    pub fn target_addr(&self) -> &str {
        &self.target_addr
    }

    async fn get_client(
        &mut self,
    ) -> std::result::Result<&mut RaftServiceClient<tonic::transport::Channel>, NetworkError> {
        if self.client.is_none() {
            // Fallback: lazy connect (backward compat, 缓存未命中时)
            tracing::debug!(
              target_addr = %self.target_addr,
              group_id = self.group_id,
              "gRPC: lazy connect (channel not cached)",
            );
            let client = RaftServiceClient::connect(self.target_addr.clone())
                .await
                .map_err(|e| {
                    tracing::warn!(
                      target_addr = %self.target_addr,
                      error = %e,
                      "gRPC client connect failed",
                    );
                    NetworkError::new(&Unreachable::new(&e))
                })?
                .max_decoding_message_size(self.max_message_size)
                .max_encoding_message_size(self.max_message_size);
            self.client = Some(client);
        }
        Ok(self.client.as_mut().unwrap())
    }
}

impl RaftNetwork<TypeConfig> for RaftNetworkClient {
    #[instrument(name = "raft_rpc_ae", skip(self, rpc, _option))]
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> std::result::Result<
        AppendEntriesResponse<NodeId>,
        RPCError<NodeId, openraft::BasicNode, RaftError<NodeId>>,
    > {
        #[cfg(feature = "monitoring")]
        crate::cluster::metrics::record_raft_rpc("append_entries", "outgoing");
        let req_timeout = self.request_timeout;
        let group_id = self.group_id;
        let client = self.get_client().await.map_err(RPCError::Network)?;

        let mut entries = Vec::new();
        for entry in rpc.entries {
            let payload = rmp_serde::to_vec(&entry.payload)
                .map_err(|e| RPCError::Network(NetworkError::new(&Unreachable::new(&e))))?;
            entries.push(raft_rpc::LogEntry {
                log_index: entry.log_id.index,
                log_term: entry.log_id.leader_id.term,
                log_leader_id: entry.log_id.leader_id.node_id,
                payload,
                is_blank: matches!(entry.payload, openraft::EntryPayload::Blank),
                is_membership: matches!(entry.payload, openraft::EntryPayload::Membership(_)),
            });
        }

        let request = raft_rpc::AppendEntriesRequest {
            group_id,
            vote_term: rpc.vote.leader_id.term,
            vote_node_id: rpc.vote.leader_id.node_id,
            vote_committed: rpc.vote.committed,
            prev_log_index: rpc.prev_log_id.map(|id| id.index),
            prev_log_term: rpc.prev_log_id.map(|id| id.leader_id.term),
            prev_log_leader_id: rpc.prev_log_id.map(|id| id.leader_id.node_id),
            entries,
            leader_commit_index: rpc.leader_commit.map(|id| id.index),
            leader_commit_term: rpc.leader_commit.map(|id| id.leader_id.term),
            leader_commit_leader_id: rpc.leader_commit.map(|id| id.leader_id.node_id),
        };

        let rpc_future = client.append_entries(TonicRequest::new(request));
        let response = match timeout(req_timeout, rpc_future).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) if e.code() == tonic::Code::Unavailable => {
                return Err(RPCError::Network(NetworkError::new(&Unreachable::new(&e))));
            }
            Ok(Err(e)) => return Err(RPCError::Network(NetworkError::new(&e))),
            Err(_) => {
                let io_err = std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("AppendEntries timeout after {}ms", req_timeout.as_millis()),
                );
                return Err(RPCError::Network(NetworkError::new(&Unreachable::new(
                    &io_err,
                ))));
            }
        };

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
            let vote = openraft::Vote {
                leader_id: openraft::LeaderId::new(resp.vote_term, resp.vote_node_id),
                committed: resp.vote_committed,
            };
            Ok(AppendEntriesResponse::HigherVote(vote))
        } else {
            Ok(AppendEntriesResponse::Conflict)
        }
    }

    #[instrument(name = "raft_rpc_is", skip(self, rpc, _option))]
    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<TypeConfig>,
        _option: RPCOption,
    ) -> std::result::Result<
        InstallSnapshotResponse<NodeId>,
        RPCError<
            NodeId,
            openraft::BasicNode,
            RaftError<NodeId, openraft::error::InstallSnapshotError>,
        >,
    > {
        #[cfg(feature = "monitoring")]
        crate::cluster::metrics::record_raft_rpc("install_snapshot", "outgoing");
        let req_timeout = self.request_timeout;
        let group_id = self.group_id;
        let client = self.get_client().await.map_err(RPCError::Network)?;

        let meta = raft_rpc::SnapshotMeta {
            last_log_index: rpc.meta.last_log_id.map(|id| id.index),
            last_log_term: rpc.meta.last_log_id.map(|id| id.leader_id.term),
            last_log_leader_id: rpc.meta.last_log_id.map(|id| id.leader_id.node_id),
            last_membership: bincode::serialize(&rpc.meta.last_membership)
                .map_err(|e| RPCError::Network(NetworkError::new(&Unreachable::new(&e))))?,
            snapshot_id: rpc.meta.snapshot_id.clone(),
        };

        let request = raft_rpc::InstallSnapshotRequest {
            group_id,
            vote_term: rpc.vote.leader_id.term,
            vote_node_id: rpc.vote.leader_id.node_id,
            vote_committed: rpc.vote.committed,
            meta: Some(meta),
            snapshot_data: rpc.data,
        };

        let rpc_future = client.install_snapshot(TonicRequest::new(request));
        let response = match timeout(req_timeout, rpc_future).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) if e.code() == tonic::Code::Unavailable => {
                return Err(RPCError::Network(NetworkError::new(&Unreachable::new(&e))));
            }
            Ok(Err(e)) => return Err(RPCError::Network(NetworkError::new(&e))),
            Err(_) => {
                let io_err = std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "InstallSnapshot timeout after {}ms",
                        req_timeout.as_millis()
                    ),
                );
                return Err(RPCError::Network(NetworkError::new(&Unreachable::new(
                    &io_err,
                ))));
            }
        };

        let resp = response.into_inner();
        Ok(InstallSnapshotResponse {
            vote: openraft::Vote {
                leader_id: openraft::LeaderId::new(resp.vote_term, resp.vote_node_id),
                committed: resp.vote_committed,
            },
        })
    }

    #[instrument(name = "raft_rpc_vote", skip(self, rpc, _option))]
    async fn vote(
        &mut self,
        rpc: VoteRequest<NodeId>,
        _option: RPCOption,
    ) -> std::result::Result<
        VoteResponse<NodeId>,
        RPCError<NodeId, openraft::BasicNode, RaftError<NodeId>>,
    > {
        #[cfg(feature = "monitoring")]
        crate::cluster::metrics::record_raft_rpc("vote", "outgoing");
        let req_timeout = self.request_timeout;
        let group_id = self.group_id;
        let client = self.get_client().await.map_err(RPCError::Network)?;

        let request = raft_rpc::VoteRequest {
            group_id,
            vote_term: rpc.vote.leader_id.term,
            vote_node_id: rpc.vote.leader_id.node_id,
            vote_committed: rpc.vote.committed,
            last_log_index: rpc.last_log_id.map(|id| id.index).unwrap_or(0),
            last_log_term: rpc.last_log_id.map(|id| id.leader_id.term).unwrap_or(0),
            last_log_leader_id: rpc.last_log_id.map(|id| id.leader_id.node_id).unwrap_or(0),
        };

        let rpc_future = client.vote(TonicRequest::new(request));
        let response = match timeout(req_timeout, rpc_future).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) if e.code() == tonic::Code::Unavailable => {
                return Err(RPCError::Network(NetworkError::new(&Unreachable::new(&e))));
            }
            Ok(Err(e)) => return Err(RPCError::Network(NetworkError::new(&e))),
            Err(_) => {
                let io_err = std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("Vote timeout after {}ms", req_timeout.as_millis()),
                );
                return Err(RPCError::Network(NetworkError::new(&Unreachable::new(
                    &io_err,
                ))));
            }
        };

        let resp = response.into_inner();
        Ok(VoteResponse {
            vote: openraft::Vote {
                leader_id: openraft::LeaderId::new(resp.vote_term, resp.vote_node_id),
                committed: resp.vote_committed,
            },
            vote_granted: resp.vote_granted,
            last_log_id: None,
        })
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
    channels: Arc<RwLock<HashMap<NodeId, RaftServiceClient<tonic::transport::Channel>>>>,
}

impl RaftNetworkClientFactory {
    pub fn new(node_id: NodeId, group_id: u64, rpc_timeout_ms: u64, max_message_size: u64) -> Self {
        Self {
            node_id,
            group_id,
            rpc_timeout_ms,
            max_message_size,
            nodes: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(RwLock::new(HashMap::new())),
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
        self.channels.write().remove(&node_id);
    }

    pub fn remove_node(&self, node_id: NodeId) {
        self.nodes.write().remove(&node_id);
        self.channels.write().remove(&node_id);
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
    ) -> std::result::Result<RaftServiceClient<tonic::transport::Channel>, NetworkError> {
        // Fast path: read lock check
        {
            let cache = self.channels.read();
            if let Some(client) = cache.get(&target) {
                return Ok(client.clone());
            }
        }
        // Slow path: connect + cache
        let normalized = normalize_grpc_addr(addr);
        tracing::debug!(%target, %normalized, "gRPC: establishing new channel for peer");
        let client = RaftServiceClient::connect(normalized)
            .await
            .map_err(|e| NetworkError::new(&Unreachable::new(&e)))?
            .max_decoding_message_size(max_message_size)
            .max_encoding_message_size(max_message_size);
        self.channels.write().insert(target, client.clone());
        Ok(client)
    }
}

/// Ensure a gRPC address has a proper URI scheme (http://) for tonic.
/// Tonic's `Channel::from_shared` requires a valid URI; bare addresses
/// like `127.0.0.1:20349` are rejected.
fn normalize_grpc_addr(addr: &str) -> String {
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
        )
    }
}

/// 多 Group Raft gRPC 服务调度器
/// 所有数据 Group 共享此实例, 通过 group_id 分发 RPC
pub struct RaftServiceDispatcher {
    groups: Arc<RwLock<HashMap<u64, Arc<openraft::Raft<TypeConfig>>>>>,
}

impl RaftServiceDispatcher {
    pub fn new() -> Self {
        Self {
            groups: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register_group(&self, group_id: u64, raft: Arc<openraft::Raft<TypeConfig>>) {
        self.groups.write().insert(group_id, raft);
    }

    pub fn unregister_group(&self, group_id: u64) {
        self.groups.write().remove(&group_id);
    }

    pub fn get_raft(&self, group_id: u64) -> Option<Arc<openraft::Raft<TypeConfig>>> {
        self.groups.read().get(&group_id).cloned()
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
                openraft::LeaderId::new(req.last_log_term, req.last_log_leader_id),
                req.last_log_index,
            ))
        } else {
            None
        };

        let vote_req = VoteRequest {
            vote: openraft::Vote {
                leader_id: openraft::LeaderId::new(req.vote_term, req.vote_node_id),
                committed: req.vote_committed,
            },
            last_log_id,
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
            vote_node_id: vote_resp.vote.leader_id.node_id,
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
            let payload: openraft::EntryPayload<TypeConfig> = rmp_serde::from_slice(&entry.payload)
                .map_err(|e| tonic::Status::internal(format!("deserialize entry: {e}")))?;
            entries.push(openraft::Entry {
                log_id: openraft::LogId::new(
                    openraft::LeaderId::new(entry.log_term, entry.log_leader_id),
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
            (Some(index), Some(term), Some(leader_id)) => Some(openraft::LogId::new(
                openraft::LeaderId::new(term, leader_id),
                index,
            )),
            _ => None,
        };

        let leader_commit = match (
            req.leader_commit_index,
            req.leader_commit_term,
            req.leader_commit_leader_id,
        ) {
            (Some(index), Some(term), Some(leader_id)) => Some(openraft::LogId::new(
                openraft::LeaderId::new(term, leader_id),
                index,
            )),
            _ => None,
        };

        let append_req = AppendEntriesRequest {
            vote: openraft::Vote {
                leader_id: openraft::LeaderId::new(req.vote_term, req.vote_node_id),
                committed: req.vote_committed,
            },
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
                vote_node_id: vote.leader_id.node_id,
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
            (Some(index), Some(term), Some(leader_id)) => Some(openraft::LogId::new(
                openraft::LeaderId::new(term, leader_id),
                index,
            )),
            _ => None,
        };

        let last_membership: openraft::StoredMembership<NodeId, openraft::BasicNode> =
            bincode::deserialize(&meta.last_membership)
                .map_err(|e| tonic::Status::internal(format!("membership decode: {e}")))?;

        let snapshot_meta = SnapshotMeta {
            last_log_id,
            last_membership,
            snapshot_id: meta.snapshot_id,
        };

        let install_req = InstallSnapshotRequest {
            vote: openraft::Vote {
                leader_id: openraft::LeaderId::new(req.vote_term, req.vote_node_id),
                committed: req.vote_committed,
            },
            meta: snapshot_meta,
            offset: 0,
            data: req.snapshot_data,
            done: true,
        };

        let raft = self
            .dispatcher
            .get_raft(req.group_id)
            .ok_or_else(|| tonic::Status::not_found(format!("group {} not found", req.group_id)))?;
        let install_resp = raft
            .install_snapshot(install_req)
            .await
            .map_err(|e| tonic::Status::internal(format!("InstallSnapshot failed: {e}")))?;

        Ok(tonic::Response::new(raft_rpc::InstallSnapshotResponse {
            vote_term: install_resp.vote.leader_id.term,
            vote_node_id: install_resp.vote.leader_id.node_id,
            vote_committed: install_resp.vote.committed,
        }))
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
}
