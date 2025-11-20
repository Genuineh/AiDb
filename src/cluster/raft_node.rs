//! Raft node implementation for AiDb
//!
//! This module provides a wrapper around raft-rs RawNode to manage
//! the Raft consensus protocol for the cluster.

use parking_lot::RwLock;
#[cfg(feature = "raft-cluster")]
use raft::{prelude::*, RawNode, StateRole};
#[cfg(feature = "raft-cluster")]
use slog::Drain;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::raft_storage::RaftStorage;
use crate::error::{Error, Result};
use crate::DB;

/// Configuration for Raft node
#[derive(Debug, Clone, Copy)]
pub struct RaftConfig {
    /// Unique node ID
    pub id: u64,
    /// Number of ticks for election timeout
    pub election_tick: usize,
    /// Number of ticks for heartbeat
    pub heartbeat_tick: usize,
    /// Maximum size per message
    pub max_size_per_msg: u64,
    /// Maximum number of inflight messages
    pub max_inflight_msgs: usize,
}

impl Default for RaftConfig {
    fn default() -> Self {
        Self {
            id: 1,
            election_tick: 10,
            heartbeat_tick: 3,
            max_size_per_msg: 1024 * 1024, // 1MB
            max_inflight_msgs: 256,
        }
    }
}

/// Raft message for communication between nodes
#[derive(Debug, Clone)]
pub enum RaftMessage {
    /// Raft protocol message
    Raft(Message),
    /// Proposal for state machine
    Proposal(Vec<u8>),
    /// Configuration change
    ConfigChange(ConfChange),
}

/// Raft node that wraps raft-rs RawNode
pub struct RaftNode {
    /// Node ID
    id: u64,
    /// Raft raw node
    raw_node: Arc<RwLock<RawNode<RaftStorage>>>,
    /// Message sender for outgoing messages
    msg_tx: mpsc::UnboundedSender<(u64, RaftMessage)>,
    /// Message receiver for incoming messages
    msg_rx: Arc<RwLock<mpsc::UnboundedReceiver<(u64, RaftMessage)>>>,
    /// Peers in the cluster (peer_id -> address)
    peers: Arc<RwLock<HashMap<u64, String>>>,
}

impl RaftNode {
    /// Create a new Raft node
    pub fn new(
        config: RaftConfig,
        storage: RaftStorage,
        peers: HashMap<u64, String>,
    ) -> Result<Self> {
        // Create Raft config
        let cfg = Config {
            id: config.id,
            election_tick: config.election_tick,
            heartbeat_tick: config.heartbeat_tick,
            max_size_per_msg: config.max_size_per_msg,
            max_inflight_msgs: config.max_inflight_msgs,
            ..Default::default()
        };

        // Validate config
        cfg.validate()
            .map_err(|e| Error::ClusterError(format!("Invalid Raft config: {}", e)))?;

        // Create logger (use slog for raft-rs)
        let logger = slog::Logger::root(slog_stdlog::StdLog.fuse(), slog::o!());

        // Create raw node
        let raw_node = RawNode::new(&cfg, storage, &logger)
            .map_err(|e| Error::ClusterError(format!("Failed to create RawNode: {}", e)))?;

        // Create message channels
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();

        Ok(Self {
            id: config.id,
            raw_node: Arc::new(RwLock::new(raw_node)),
            msg_tx,
            msg_rx: Arc::new(RwLock::new(msg_rx)),
            peers: Arc::new(RwLock::new(peers)),
        })
    }

    /// Get node ID
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Check if this node is the leader
    pub fn is_leader(&self) -> bool {
        let raw_node = self.raw_node.read();
        raw_node.raft.state == StateRole::Leader
    }

    /// Get the current leader ID
    pub fn leader(&self) -> Option<u64> {
        let raw_node = self.raw_node.read();
        let leader = raw_node.raft.leader_id;
        if leader == 0 {
            None
        } else {
            Some(leader)
        }
    }

    /// Propose a change to the state machine
    pub fn propose(&self, data: Vec<u8>) -> Result<()> {
        let mut raw_node = self.raw_node.write();
        raw_node
            .propose(vec![], data)
            .map_err(|e| Error::ClusterError(format!("Failed to propose: {}", e)))?;
        Ok(())
    }

    /// Propose a configuration change
    pub fn propose_conf_change(&self, cc: ConfChange) -> Result<()> {
        let mut raw_node = self.raw_node.write();
        raw_node
            .propose_conf_change(vec![], cc)
            .map_err(|e| Error::ClusterError(format!("Failed to propose conf change: {}", e)))?;
        Ok(())
    }

    /// Step the Raft state machine with a message
    pub fn step(&self, msg: Message) -> Result<()> {
        let mut raw_node = self.raw_node.write();
        raw_node
            .step(msg)
            .map_err(|e| Error::ClusterError(format!("Failed to step: {}", e)))?;
        Ok(())
    }

    /// Tick the Raft state machine
    pub fn tick(&self) -> Result<()> {
        let mut raw_node = self.raw_node.write();
        raw_node.tick();
        Ok(())
    }

    /// Check if there are ready messages to process
    pub fn has_ready(&self) -> bool {
        let raw_node = self.raw_node.read();
        raw_node.has_ready()
    }

    /// Get ready messages and process them
    pub fn ready(&self) -> Option<Ready> {
        let mut raw_node = self.raw_node.write();
        if !raw_node.has_ready() {
            return None;
        }
        Some(raw_node.ready())
    }

    /// Advance the Raft state machine
    pub fn advance(&self, rd: Ready) {
        let mut raw_node = self.raw_node.write();
        raw_node.advance(rd);
    }

    /// Send a message to a peer
    pub fn send_message(&self, to: u64, msg: RaftMessage) -> Result<()> {
        self.msg_tx
            .send((to, msg))
            .map_err(|e| Error::ClusterError(format!("Failed to send message: {}", e)))?;
        Ok(())
    }

    /// Receive a message from the channel
    pub fn recv_message(&self) -> Option<(u64, RaftMessage)> {
        let mut rx = self.msg_rx.write();
        rx.try_recv().ok()
    }

    /// Add a peer to the cluster
    pub fn add_peer(&self, peer_id: u64, address: String) {
        let mut peers = self.peers.write();
        peers.insert(peer_id, address);
    }

    /// Remove a peer from the cluster
    pub fn remove_peer(&self, peer_id: u64) {
        let mut peers = self.peers.write();
        peers.remove(&peer_id);
    }

    /// Get all peers
    pub fn peers(&self) -> HashMap<u64, String> {
        let peers = self.peers.read();
        peers.clone()
    }

    /// Get Raft status information
    pub fn status_info(&self) -> (u64, u64, bool) {
        let raw_node = self.raw_node.read();
        let is_leader = raw_node.raft.state == StateRole::Leader;
        (raw_node.raft.term, raw_node.raft.raft_log.committed, is_leader)
    }

    /// Get the current term
    pub fn term(&self) -> u64 {
        let raw_node = self.raw_node.read();
        raw_node.raft.term
    }

    /// Get the current committed index
    pub fn committed(&self) -> u64 {
        let raw_node = self.raw_node.read();
        raw_node.raft.raft_log.committed
    }
}

/// State machine interface for Raft
pub trait StateMachine {
    /// Apply a committed log entry
    fn apply(&mut self, data: &[u8]) -> Result<Vec<u8>>;

    /// Create a snapshot of the current state
    fn snapshot(&self) -> Result<Vec<u8>>;

    /// Restore state from a snapshot
    fn restore(&mut self, snapshot: &[u8]) -> Result<()>;
}

/// Raft state machine implementation for AiDb
pub struct RaftStateMachine {
    /// Local database
    db: Arc<DB>,
}

impl RaftStateMachine {
    /// Create a new Raft state machine
    pub fn new(db: Arc<DB>) -> Self {
        Self { db }
    }
}

impl StateMachine for RaftStateMachine {
    fn apply(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        // Parse the command from data
        // Format: [op_type: u8][key_len: u32][key][value_len: u32][value]

        if data.is_empty() {
            return Err(Error::InvalidArgument("Empty command".to_string()));
        }

        let op_type = data[0];
        let mut offset = 1;

        match op_type {
            0 => {
                // PUT operation
                if data.len() < 9 {
                    return Err(Error::InvalidArgument("Invalid PUT command".to_string()));
                }

                let key_len = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
                offset += 4;

                if data.len() < offset + key_len + 4 {
                    return Err(Error::InvalidArgument("Invalid PUT command length".to_string()));
                }

                let key = &data[offset..offset + key_len];
                offset += key_len;

                let value_len = u32::from_be_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]) as usize;
                offset += 4;

                if data.len() < offset + value_len {
                    return Err(Error::InvalidArgument(
                        "Invalid PUT command value length".to_string(),
                    ));
                }

                let value = &data[offset..offset + value_len];

                self.db.put(key, value)?;
                Ok(b"OK".to_vec())
            }
            1 => {
                // DELETE operation
                if data.len() < 5 {
                    return Err(Error::InvalidArgument("Invalid DELETE command".to_string()));
                }

                let key_len = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;
                offset += 4;

                if data.len() < offset + key_len {
                    return Err(Error::InvalidArgument(
                        "Invalid DELETE command length".to_string(),
                    ));
                }

                let key = &data[offset..offset + key_len];

                self.db.delete(key)?;
                Ok(b"OK".to_vec())
            }
            _ => Err(Error::InvalidArgument(format!("Unknown operation type: {}", op_type))),
        }
    }

    fn snapshot(&self) -> Result<Vec<u8>> {
        // For now, return empty snapshot
        // TODO: Implement proper snapshot generation
        Ok(Vec::new())
    }

    fn restore(&mut self, _snapshot: &[u8]) -> Result<()> {
        // For now, do nothing
        // TODO: Implement proper snapshot restoration
        Ok(())
    }
}

/// Helper functions to encode commands
/// Encode a PUT command
pub fn encode_put(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.push(0); // PUT operation
    data.extend_from_slice(&(key.len() as u32).to_be_bytes());
    data.extend_from_slice(key);
    data.extend_from_slice(&(value.len() as u32).to_be_bytes());
    data.extend_from_slice(value);
    data
}

/// Encode a DELETE command
pub fn encode_delete(key: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.push(1); // DELETE operation
    data.extend_from_slice(&(key.len() as u32).to_be_bytes());
    data.extend_from_slice(key);
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Options;
    use tempfile::TempDir;

    fn create_test_db() -> (Arc<DB>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        (Arc::new(db), temp_dir)
    }

    #[test]
    fn test_encode_put() {
        let key = b"test_key";
        let value = b"test_value";
        let encoded = encode_put(key, value);

        assert_eq!(encoded[0], 0); // PUT op
        let key_len = u32::from_be_bytes([encoded[1], encoded[2], encoded[3], encoded[4]]);
        assert_eq!(key_len, key.len() as u32);
    }

    #[test]
    fn test_encode_delete() {
        let key = b"test_key";
        let encoded = encode_delete(key);

        assert_eq!(encoded[0], 1); // DELETE op
        let key_len = u32::from_be_bytes([encoded[1], encoded[2], encoded[3], encoded[4]]);
        assert_eq!(key_len, key.len() as u32);
    }

    #[test]
    fn test_state_machine_apply_put() {
        let (db, _temp_dir) = create_test_db();
        let mut sm = RaftStateMachine::new(db.clone());

        let key = b"key1";
        let value = b"value1";
        let cmd = encode_put(key, value);

        let result = sm.apply(&cmd);
        assert!(result.is_ok());

        // Verify the data was written
        let retrieved = db.get(key).unwrap();
        assert_eq!(retrieved, Some(value.to_vec()));
    }

    #[test]
    fn test_state_machine_apply_delete() {
        let (db, _temp_dir) = create_test_db();
        let mut sm = RaftStateMachine::new(db.clone());

        // First put a value
        let key = b"key1";
        let value = b"value1";
        db.put(key, value).unwrap();

        // Then delete it through state machine
        let cmd = encode_delete(key);
        let result = sm.apply(&cmd);
        assert!(result.is_ok());

        // Verify the data was deleted
        let retrieved = db.get(key).unwrap();
        assert_eq!(retrieved, None);
    }

    #[test]
    fn test_raft_config_default() {
        let config = RaftConfig::default();
        assert_eq!(config.id, 1);
        assert_eq!(config.election_tick, 10);
        assert_eq!(config.heartbeat_tick, 3);
    }
}
