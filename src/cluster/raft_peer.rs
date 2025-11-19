//! Raft-based peer implementation that combines PeerNode with Raft consensus
//!
//! This module provides a complete peer node that uses Raft for consensus
//! while maintaining the P2P routing capabilities of PeerNode.

#[cfg(feature = "raft-cluster")]
use raft::prelude::*;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use super::raft_node::{RaftConfig, RaftNode, RaftStateMachine, StateMachine, encode_put, encode_delete};
use super::raft_storage::RaftStorage;
use super::raft_transport::{RaftTransport, RaftPeer as RaftPeerInternal};
use crate::error::{Error, Result};
use crate::DB;

/// Complete Raft-based peer node
pub struct RaftBasedPeer {
    /// Node ID
    id: u64,
    /// Local database
    db: Arc<DB>,
    /// Raft peer (node + transport)
    raft_peer: Arc<RaftPeerInternal>,
    /// State machine for applying commands
    state_machine: Arc<RwLock<RaftStateMachine>>,
}

impl RaftBasedPeer {
    /// Create a new Raft-based peer
    pub async fn new(
        id: u64,
        db: Arc<DB>,
        peers: HashMap<u64, String>,
        config: RaftConfig,
    ) -> Result<Self> {
        // Create Raft storage
        let storage = RaftStorage::new(db.clone())?;

        // Create Raft node
        let raft_node = Arc::new(RaftNode::new(config, storage, peers.clone())?);

        // Create transport
        let transport = Arc::new(RaftTransport::new(id));

        // Add peers to transport
        for (peer_id, address) in peers {
            if peer_id != id {
                transport.add_peer(peer_id, address).await?;
            }
        }

        // Create Raft peer
        let raft_peer = Arc::new(RaftPeerInternal::new(raft_node, transport));

        // Create state machine
        let state_machine = Arc::new(RwLock::new(RaftStateMachine::new(db.clone())));

        Ok(Self {
            id,
            db,
            raft_peer,
            state_machine,
        })
    }

    /// Get the peer ID
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Start the peer
    pub async fn start(&self) -> Result<()> {
        self.raft_peer.start().await
    }

    /// Stop the peer
    pub fn stop(&self) {
        self.raft_peer.stop();
    }

    /// Check if this peer is the leader
    pub fn is_leader(&self) -> bool {
        self.raft_peer.node().is_leader()
    }

    /// Get the current leader ID
    pub fn leader(&self) -> Option<u64> {
        self.raft_peer.node().leader()
    }

    /// Put a key-value pair (proposes through Raft)
    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        if !self.is_leader() {
            return Err(Error::InvalidState("Not the leader".to_string()));
        }

        let cmd = encode_put(key, value);
        self.raft_peer.node().propose(cmd)?;

        // TODO: Wait for the command to be committed
        // For now, we return immediately after proposing
        Ok(())
    }

    /// Get a value (reads from local state)
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db.get(key)
    }

    /// Delete a key (proposes through Raft)
    pub async fn delete(&self, key: &[u8]) -> Result<()> {
        if !self.is_leader() {
            return Err(Error::InvalidState("Not the leader".to_string()));
        }

        let cmd = encode_delete(key);
        self.raft_peer.node().propose(cmd)?;

        // TODO: Wait for the command to be committed
        Ok(())
    }

    /// Apply a committed entry to the state machine
    pub fn apply_entry(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut sm = self.state_machine.write();
        sm.apply(data)
    }

    /// Get Raft status information
    pub fn status_info(&self) -> (u64, u64, bool) {
        self.raft_peer.node().status_info()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    use tempfile::TempDir;

    async fn create_test_peer(id: u64) -> (RaftBasedPeer, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        let config = RaftConfig {
            id,
            election_tick: 10,
            heartbeat_tick: 3,
            ..Default::default()
        };
        let peer = RaftBasedPeer::new(id, Arc::new(db), HashMap::new(), config)
            .await
            .unwrap();
        (peer, temp_dir)
    }

    #[tokio::test]
    async fn test_raft_based_peer_creation() {
        let (peer, _temp_dir) = create_test_peer(1).await;
        assert_eq!(peer.id(), 1);
        assert!(!peer.is_leader());
    }

    #[tokio::test]
    async fn test_raft_based_peer_start_stop() {
        let (peer, _temp_dir) = create_test_peer(1).await;
        
        peer.start().await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        
        peer.stop();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn test_raft_based_peer_get() {
        let (peer, _temp_dir) = create_test_peer(1).await;
        
        // Write directly to DB for testing
        peer.db.put(b"test_key", b"test_value").unwrap();
        
        let value = peer.get(b"test_key").unwrap();
        assert_eq!(value, Some(b"test_value".to_vec()));
    }

    #[tokio::test]
    async fn test_raft_based_peer_status() {
        let (peer, _temp_dir) = create_test_peer(1).await;
        
        let (term, committed, is_leader) = peer.status_info();
        assert_eq!(term, 0); // Initial term
        assert_eq!(committed, 0); // Nothing committed yet
        assert!(!is_leader); // Not leader initially
    }

    #[tokio::test]
    async fn test_apply_entry() {
        let (peer, _temp_dir) = create_test_peer(1).await;
        
        let cmd = encode_put(b"key1", b"value1");
        let result = peer.apply_entry(&cmd);
        assert!(result.is_ok());
        
        // Verify the data was written
        let value = peer.get(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
    }
}
