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
    RaftNetwork, RaftNetworkFactory,
};

use crate::cluster::raft_storage::{NodeId, TypeConfig};

// Include the generated protobuf code
#[cfg(feature = "raft-cluster")]
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
