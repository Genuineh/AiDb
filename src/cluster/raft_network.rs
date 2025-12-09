//! OpenRaft network layer implementation for AiDb
//!
//! This module implements the network communication layer for OpenRaft using gRPC/tonic.
//! It provides RPC client and server implementations for Raft consensus protocol.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::Request as TonicRequest;

#[cfg(feature = "raft-cluster")]
use openraft::{
    error::{NetworkError, RPCError, RaftError, Unreachable},
    network::RPCOption,
    raft::{
        AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest,
        InstallSnapshotResponse, VoteRequest, VoteResponse,
    },
    RaftNetwork, RaftNetworkFactory, SnapshotMeta,
};

use crate::cluster::raft_storage::{NodeId, TypeConfig};

/// Generated protobuf code for Raft RPC
#[cfg(feature = "raft-cluster")]
#[allow(missing_docs)]
pub mod raft_rpc {
    tonic::include_proto!("aidb.raft");
}

#[cfg(feature = "raft-cluster")]
use raft_rpc::raft_service_client::RaftServiceClient;

/// Network client for communicating with other Raft nodes
#[cfg(feature = "raft-cluster")]
pub struct RaftNetworkClient {
    /// Target node address
    target_addr: String,
    /// gRPC client
    client: Option<RaftServiceClient<tonic::transport::Channel>>,
}

#[cfg(feature = "raft-cluster")]
impl RaftNetworkClient {
    /// Create a new network client
    pub fn new(_node_id: NodeId, _target: NodeId, target_addr: String) -> Self {
        Self { target_addr, client: None }
    }

    /// Get or create the gRPC client connection
    async fn get_client(
        &mut self,
    ) -> std::result::Result<&mut RaftServiceClient<tonic::transport::Channel>, NetworkError> {
        if self.client.is_none() {
            let client = RaftServiceClient::connect(self.target_addr.clone())
                .await
                .map_err(|e| NetworkError::new(&Unreachable::new(&e)))?;
            self.client = Some(client);
        }
        Ok(self.client.as_mut().unwrap())
    }
}

#[cfg(feature = "raft-cluster")]
impl RaftNetwork<TypeConfig> for RaftNetworkClient {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> std::result::Result<
        AppendEntriesResponse<NodeId>,
        RPCError<NodeId, openraft::BasicNode, RaftError<NodeId>>,
    > {
        let client = self.get_client().await.map_err(RPCError::Network)?;

        // Convert request to protobuf
        let mut entries = Vec::new();
        for entry in rpc.entries {
            let payload = bincode::serialize(&entry.payload)
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
            group_id: 0, // Default group for single-Raft mode
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

        let response = client.append_entries(TonicRequest::new(request)).await.map_err(|e| {
            if e.code() == tonic::Code::Unavailable {
                RPCError::Network(NetworkError::new(&Unreachable::new(&e)))
            } else {
                RPCError::Network(NetworkError::new(&e))
            }
        })?;

        let resp = response.into_inner();

        // In openraft 0.9, AppendEntriesResponse is an enum
        if resp.success {
            Ok(AppendEntriesResponse::Success)
        } else if resp.conflict_index.is_some() {
            // For now, return Conflict - in production you'd check vote differences
            Ok(AppendEntriesResponse::Conflict)
        } else {
            Ok(AppendEntriesResponse::Conflict)
        }
    }

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
        let client = self.get_client().await.map_err(RPCError::Network)?;

        // Convert metadata to protobuf
        let meta = raft_rpc::SnapshotMeta {
            last_log_index: rpc.meta.last_log_id.map(|id| id.index),
            last_log_term: rpc.meta.last_log_id.map(|id| id.leader_id.term),
            last_log_leader_id: rpc.meta.last_log_id.map(|id| id.leader_id.node_id),
            last_membership: bincode::serialize(&rpc.meta.last_membership)
                .map_err(|e| RPCError::Network(NetworkError::new(&Unreachable::new(&e))))?,
            snapshot_id: rpc.meta.snapshot_id,
        };

        // In openraft 0.9, snapshots are sent in chunks
        let request = raft_rpc::InstallSnapshotRequest {
            group_id: 0, // Default group for single-Raft mode
            vote_term: rpc.vote.leader_id.term,
            vote_node_id: rpc.vote.leader_id.node_id,
            vote_committed: rpc.vote.committed,
            meta: Some(meta),
            snapshot_data: rpc.data,
        };

        let response = client.install_snapshot(TonicRequest::new(request)).await.map_err(|e| {
            if e.code() == tonic::Code::Unavailable {
                RPCError::Network(NetworkError::new(&Unreachable::new(&e)))
            } else {
                RPCError::Network(NetworkError::new(&e))
            }
        })?;

        let resp = response.into_inner();

        Ok(InstallSnapshotResponse {
            vote: openraft::Vote {
                leader_id: openraft::LeaderId::new(resp.vote_term, resp.vote_node_id),
                committed: resp.vote_committed,
            },
        })
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<NodeId>,
        _option: RPCOption,
    ) -> std::result::Result<
        VoteResponse<NodeId>,
        RPCError<NodeId, openraft::BasicNode, RaftError<NodeId>>,
    > {
        let client = self.get_client().await.map_err(RPCError::Network)?;

        let request = raft_rpc::VoteRequest {
            group_id: 0, // Default group for single-Raft mode
            vote_term: rpc.vote.leader_id.term,
            vote_node_id: rpc.vote.leader_id.node_id,
            vote_committed: rpc.vote.committed,
            last_log_index: rpc.last_log_id.map(|id| id.index).unwrap_or(0),
            last_log_term: rpc.last_log_id.map(|id| id.leader_id.term).unwrap_or(0),
            last_log_leader_id: rpc.last_log_id.map(|id| id.leader_id.node_id).unwrap_or(0),
        };

        let response = client.vote(TonicRequest::new(request)).await.map_err(|e| {
            if e.code() == tonic::Code::Unavailable {
                RPCError::Network(NetworkError::new(&Unreachable::new(&e)))
            } else {
                RPCError::Network(NetworkError::new(&e))
            }
        })?;

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

/// Factory for creating network clients
#[cfg(feature = "raft-cluster")]
pub struct RaftNetworkClientFactory {
    /// Current node ID
    node_id: NodeId,
    /// Map of node IDs to addresses
    nodes: Arc<RwLock<HashMap<NodeId, String>>>,
}

#[cfg(feature = "raft-cluster")]
impl RaftNetworkClientFactory {
    /// Create a new network factory
    pub fn new(node_id: NodeId) -> Self {
        Self { node_id, nodes: Arc::new(RwLock::new(HashMap::new())) }
    }

    /// Add a node address
    pub fn add_node(&self, node_id: NodeId, address: String) {
        self.nodes.write().insert(node_id, address);
    }

    /// Remove a node
    pub fn remove_node(&self, node_id: NodeId) {
        self.nodes.write().remove(&node_id);
    }
}

#[cfg(feature = "raft-cluster")]
impl RaftNetworkFactory<TypeConfig> for RaftNetworkClientFactory {
    type Network = RaftNetworkClient;

    async fn new_client(&mut self, target: NodeId, _node: &openraft::BasicNode) -> Self::Network {
        let target_addr = self
            .nodes
            .read()
            .get(&target)
            .cloned()
            .unwrap_or_else(|| format!("http://127.0.0.1:{}", 50000 + target));

        RaftNetworkClient::new(self.node_id, target, target_addr)
    }
}

/// RPC server implementation for handling Raft requests from other nodes
#[cfg(feature = "raft-cluster")]
pub struct RaftServiceImpl {
    /// The Raft instance
    raft: Arc<openraft::Raft<TypeConfig>>,
}

#[cfg(feature = "raft-cluster")]
impl RaftServiceImpl {
    /// Create a new RaftServiceImpl
    pub fn new(raft: Arc<openraft::Raft<TypeConfig>>) -> Self {
        Self { raft }
    }
}

#[cfg(feature = "raft-cluster")]
use raft_rpc::raft_service_server::RaftService;

#[cfg(feature = "raft-cluster")]
#[tonic::async_trait]
impl RaftService for RaftServiceImpl {
    async fn vote(
        &self,
        request: TonicRequest<raft_rpc::VoteRequest>,
    ) -> Result<tonic::Response<raft_rpc::VoteResponse>, tonic::Status> {
        let req = request.into_inner();

        // Convert protobuf request to openraft VoteRequest
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

        // Call raft.vote()
        let vote_resp = self
            .raft
            .vote(vote_req)
            .await
            .map_err(|e| tonic::Status::internal(format!("Vote failed: {}", e)))?;

        // Convert response to protobuf
        let response = raft_rpc::VoteResponse {
            vote_term: vote_resp.vote.leader_id.term,
            vote_node_id: vote_resp.vote.leader_id.node_id,
            vote_committed: vote_resp.vote.committed,
            vote_granted: vote_resp.vote_granted,
            is_in_membership: true, // Simplified for now
        };

        Ok(tonic::Response::new(response))
    }

    async fn append_entries(
        &self,
        request: TonicRequest<raft_rpc::AppendEntriesRequest>,
    ) -> Result<tonic::Response<raft_rpc::AppendEntriesResponse>, tonic::Status> {
        let req = request.into_inner();

        // Convert protobuf entries to openraft entries
        let mut entries = Vec::new();
        for entry in req.entries {
            let payload: openraft::EntryPayload<TypeConfig> = bincode::deserialize(&entry.payload)
                .map_err(|e| tonic::Status::internal(format!("Failed to deserialize entry: {}", e)))?;

            entries.push(openraft::Entry {
                log_id: openraft::LogId::new(
                    openraft::LeaderId::new(entry.log_term, entry.log_leader_id),
                    entry.log_index,
                ),
                payload,
            });
        }

        // Convert prev_log_id
        let prev_log_id = if let (Some(index), Some(term), Some(leader_id)) =
            (req.prev_log_index, req.prev_log_term, req.prev_log_leader_id)
        {
            Some(openraft::LogId::new(openraft::LeaderId::new(term, leader_id), index))
        } else {
            None
        };

        // Convert leader_commit
        let leader_commit = if let (Some(index), Some(term), Some(leader_id)) = (
            req.leader_commit_index,
            req.leader_commit_term,
            req.leader_commit_leader_id,
        ) {
            Some(openraft::LogId::new(openraft::LeaderId::new(term, leader_id), index))
        } else {
            None
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

        // Call raft.append_entries()
        let append_resp = self
            .raft
            .append_entries(append_req)
            .await
            .map_err(|e| tonic::Status::internal(format!("AppendEntries failed: {}", e)))?;

        // Convert response to protobuf
        let response = match append_resp {
            AppendEntriesResponse::Success => raft_rpc::AppendEntriesResponse {
                vote_term: 0,
                vote_node_id: 0,
                vote_committed: false,
                success: true,
                conflict_index: None,
                conflict_term: None,
            },
            AppendEntriesResponse::PartialSuccess(_) => raft_rpc::AppendEntriesResponse {
                vote_term: 0,
                vote_node_id: 0,
                vote_committed: false,
                success: true,
                conflict_index: None,
                conflict_term: None,
            },
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
    ) -> Result<tonic::Response<raft_rpc::InstallSnapshotResponse>, tonic::Status> {
        let req = request.into_inner();

        let meta = req.meta.ok_or_else(|| tonic::Status::invalid_argument("Missing snapshot meta"))?;

        // Convert metadata
        let last_log_id = if let (Some(index), Some(term), Some(leader_id)) =
            (meta.last_log_index, meta.last_log_term, meta.last_log_leader_id)
        {
            Some(openraft::LogId::new(openraft::LeaderId::new(term, leader_id), index))
        } else {
            None
        };

        let last_membership: openraft::StoredMembership<NodeId, openraft::BasicNode> =
            bincode::deserialize(&meta.last_membership).map_err(|e| {
                tonic::Status::internal(format!("Failed to deserialize membership: {}", e))
            })?;

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

        // Call raft.install_snapshot()
        let install_resp = self
            .raft
            .install_snapshot(install_req)
            .await
            .map_err(|e| tonic::Status::internal(format!("InstallSnapshot failed: {}", e)))?;

        // Convert response to protobuf
        let response = raft_rpc::InstallSnapshotResponse {
            vote_term: install_resp.vote.leader_id.term,
            vote_node_id: install_resp.vote.leader_id.node_id,
            vote_committed: install_resp.vote.committed,
        };

        Ok(tonic::Response::new(response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_factory_creation() {
        let factory = RaftNetworkClientFactory::new(1);
        assert_eq!(factory.node_id, 1);
    }

    #[test]
    fn test_add_remove_node() {
        let factory = RaftNetworkClientFactory::new(1);
        factory.add_node(2, "http://localhost:50002".to_string());

        assert_eq!(factory.nodes.read().len(), 1);

        factory.remove_node(2);
        assert_eq!(factory.nodes.read().len(), 0);
    }
}
