//! Raft message transport implementation using gRPC
//!
//! This module implements the transport layer for Raft messages,
//! enabling communication between Raft nodes in the cluster.

use parking_lot::RwLock;
#[cfg(feature = "raft-cluster")]
use raft::prelude::Message as RaftMessage;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tonic::transport::Channel;

use super::raft_node::RaftNode;
use crate::cluster::rpc::proto::storage_client::StorageClient;
use crate::error::{Error, Result};

/// Transport for sending Raft messages between nodes
pub struct RaftTransport {
    /// Local node ID
    node_id: u64,
    /// Connections to peer nodes
    peers: Arc<RwLock<HashMap<u64, StorageClient<Channel>>>>,
    /// Channel for receiving incoming messages
    incoming_rx: Arc<RwLock<mpsc::UnboundedReceiver<(u64, RaftMessage)>>>,
    /// Channel for sending messages to be processed
    incoming_tx: mpsc::UnboundedSender<(u64, RaftMessage)>,
}

impl RaftTransport {
    /// Create a new Raft transport
    pub fn new(node_id: u64) -> Self {
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
        Self {
            node_id,
            peers: Arc::new(RwLock::new(HashMap::new())),
            incoming_rx: Arc::new(RwLock::new(incoming_rx)),
            incoming_tx,
        }
    }

    /// Add a peer to the transport
    pub async fn add_peer(&self, peer_id: u64, address: String) -> Result<()> {
        let client = StorageClient::connect(address.clone()).await.map_err(|e| {
            Error::ClusterError(format!("Failed to connect to peer {}: {}", peer_id, e))
        })?;

        let mut peers = self.peers.write();
        peers.insert(peer_id, client);
        log::info!("Added peer {} at {} to Raft transport", peer_id, address);
        Ok(())
    }

    /// Remove a peer from the transport
    pub fn remove_peer(&self, peer_id: u64) {
        let mut peers = self.peers.write();
        peers.remove(&peer_id);
        log::info!("Removed peer {} from Raft transport", peer_id);
    }

    /// Send a Raft message to a peer
    pub async fn send_message(&self, to: u64, msg: RaftMessage) -> Result<()> {
        if to == self.node_id {
            // Local message, deliver directly
            self.incoming_tx.send((to, msg)).map_err(|e| {
                Error::ClusterError(format!("Failed to deliver local message: {}", e))
            })?;
            return Ok(());
        }

        // For now, we'll log that we would send this message
        // In a full implementation, we'd serialize and send via gRPC
        log::debug!(
            "Would send Raft message from {} to {}: msg_type={:?}, term={}, index={}",
            self.node_id,
            to,
            msg.get_msg_type(),
            msg.term,
            msg.index
        );

        // TODO: Implement actual RPC sending
        // This would involve:
        // 1. Serializing the Raft message to protobuf
        // 2. Sending via gRPC to the peer
        // 3. Handling errors and retries

        Ok(())
    }

    /// Receive incoming Raft messages
    pub fn recv_message(&self) -> Option<(u64, RaftMessage)> {
        let mut rx = self.incoming_rx.write();
        rx.try_recv().ok()
    }

    /// Deliver a message from a peer (called by RPC handler)
    pub fn deliver_message(&self, from: u64, msg: RaftMessage) -> Result<()> {
        self.incoming_tx.send((from, msg)).map_err(|e| {
            Error::ClusterError(format!("Failed to deliver message from {}: {}", from, e))
        })
    }
}

/// Raft peer that integrates RaftNode with transport
pub struct RaftPeer {
    /// Raft node
    node: Arc<RaftNode>,
    /// Transport layer
    transport: Arc<RaftTransport>,
    /// Flag to indicate if the peer is running
    running: Arc<RwLock<bool>>,
}

impl RaftPeer {
    /// Create a new Raft peer
    pub fn new(node: Arc<RaftNode>, transport: Arc<RaftTransport>) -> Self {
        Self { node, transport, running: Arc::new(RwLock::new(false)) }
    }

    /// Start the Raft peer event loop
    pub async fn start(&self) -> Result<()> {
        {
            let mut running = self.running.write();
            if *running {
                return Err(Error::InvalidState("Raft peer already running".to_string()));
            }
            *running = true;
        }

        log::info!("Starting Raft peer {}", self.node.id());

        // Clone Arc references for the task
        let node = self.node.clone();
        let transport = self.transport.clone();
        let running = self.running.clone();

        // Spawn the main event loop
        tokio::spawn(async move {
            let tick_duration = tokio::time::Duration::from_millis(100);
            let mut interval = tokio::time::interval(tick_duration);

            while *running.read() {
                interval.tick().await;

                // Tick the Raft node
                if let Err(e) = node.tick() {
                    log::error!("Raft tick error: {}", e);
                    continue;
                }

                // Process incoming messages
                while let Some((from, msg)) = transport.recv_message() {
                    log::debug!("Received Raft message from {}", from);
                    if let Err(e) = node.step(msg) {
                        log::error!("Failed to step Raft with message: {}", e);
                    }
                }

                // Handle ready messages
                if node.has_ready() {
                    if let Some(ready) = node.ready() {
                        // Send messages to peers
                        for msg in ready.messages() {
                            let to = msg.to;
                            if let Err(e) = transport.send_message(to, msg.clone()).await {
                                log::error!("Failed to send message to {}: {}", to, e);
                            }
                        }

                        // TODO: Apply committed entries to state machine
                        // TODO: Persist hard state and entries
                        // TODO: Apply snapshot if present

                        // Advance the Raft state machine
                        node.advance(ready);
                    }
                }
            }

            log::info!("Raft peer event loop stopped");
        });

        Ok(())
    }

    /// Stop the Raft peer
    pub fn stop(&self) {
        let mut running = self.running.write();
        *running = false;
        log::info!("Stopping Raft peer {}", self.node.id());
    }

    /// Check if the peer is running
    pub fn is_running(&self) -> bool {
        *self.running.read()
    }

    /// Get the underlying Raft node
    pub fn node(&self) -> &Arc<RaftNode> {
        &self.node
    }

    /// Get the transport
    pub fn transport(&self) -> &Arc<RaftTransport> {
        &self.transport
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_creation() {
        let transport = RaftTransport::new(1);
        assert_eq!(transport.node_id, 1);
    }

    #[test]
    fn test_remove_peer() {
        let transport = RaftTransport::new(1);
        transport.remove_peer(2);
        // Should not panic even if peer doesn't exist
    }

    #[tokio::test]
    async fn test_raft_peer_lifecycle() {
        use crate::cluster::{RaftConfig, RaftNode, RaftStorage};
        use crate::{Options, DB};
        use std::collections::HashMap;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let db = DB::open(temp_dir.path(), Options::default()).unwrap();
        let storage = RaftStorage::new(Arc::new(db)).unwrap();
        let config = RaftConfig { id: 1, ..Default::default() };
        let node = Arc::new(RaftNode::new(config, storage, HashMap::new()).unwrap());
        let transport = Arc::new(RaftTransport::new(1));
        let peer = RaftPeer::new(node, transport);

        assert!(!peer.is_running());

        peer.start().await.unwrap();
        assert!(peer.is_running());

        peer.stop();
        // Give it a moment to stop
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }
}
