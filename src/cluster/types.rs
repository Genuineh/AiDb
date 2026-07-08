//! Raft 类型配置与请求/响应.

use serde::{Deserialize, Serialize};

pub use crate::error::ClusterError;

pub use crate::cluster::meta_types::MetaRequest;

pub type NodeId = u64;

/// OpenRaft 类型配置
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TypeConfig;

impl openraft::RaftTypeConfig for TypeConfig {
    type D = Request;
    type R = Response;
    type NodeId = NodeId;
    type Node = openraft::BasicNode;
    type Entry = openraft::Entry<TypeConfig>;
    type SnapshotData = std::io::Cursor<Vec<u8>>;
    type AsyncRuntime = openraft::TokioRuntime;
    type Responder = openraft::impls::OneshotResponder<TypeConfig>;
}

pub type LogEntry = openraft::Entry<TypeConfig>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThinWriteOp {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThinWriteBatch {
    pub ops: Vec<ThinWriteOp>,
}

impl ThinWriteBatch {
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.ops.push(ThinWriteOp::Put { key, value });
    }

    pub fn delete(&mut self, key: Vec<u8>) {
        self.ops.push(ThinWriteOp::Delete { key });
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// 估算 rmp_serde 序列化后的大小.
    pub fn estimated_serialized_size(&self) -> usize {
        self.ops
            .iter()
            .map(|op| match op {
                ThinWriteOp::Put { key, value } => key.len() + value.len() + 16,
                ThinWriteOp::Delete { key } => key.len() + 10,
            })
            .sum::<usize>()
            + 10 // vec header overhead
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// Note: intentionally without adjacently tagged serde — rmp_serde cannot roundtrip
// tagged enums nested in openraft::Entry; see test_request_serde_roundtrip.
pub enum Request {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
    PutConditional { key: Vec<u8>, value: Vec<u8> },
    WriteBatch(ThinWriteBatch),
    Meta(MetaRequest),
}

impl Request {
    pub fn to_batch(self) -> ThinWriteBatch {
        match self {
            Request::Put { key, value } => {
                let mut batch = ThinWriteBatch::new();
                batch.put(key, value);
                batch
            }
            Request::Delete { key } => {
                let mut batch = ThinWriteBatch::new();
                batch.delete(key);
                batch
            }
            Request::PutConditional { .. } | Request::Meta(_) => ThinWriteBatch::new(),
            Request::WriteBatch(batch) => batch,
        }
    }

    /// 估算 rmp_serde 序列化后的大小, 避免完整序列化后丢弃.
    /// 上界估计 (保守略大), 用于 max_entry_size 校验.
    pub fn estimated_serialized_size(&self) -> usize {
        match self {
            Request::Put { key, value } | Request::PutConditional { key, value } => {
                key.len() + value.len() + 16
            }
            Request::Delete { key } => key.len() + 10,
            Request::WriteBatch(batch) => batch.estimated_serialized_size(),
            Request::Meta(_) => 512,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    Ok,
    Value(Option<Vec<u8>>),
    Error(String),
}

/// Raft 节点运行时配置
#[derive(Debug, Clone)]
pub struct RaftNodeConfig {
    pub node_id: NodeId,
    pub group_id: u64,
    pub election_timeout_min: u64,
    pub election_timeout_max: u64,
    pub heartbeat_interval: u64,
    pub max_payload_entries: u64,
    pub snapshot_logs_since_last: u64,
    /// 当 leader propose 成功累积的写估算字节数超此阈值时触发 snapshot (None = 禁用).
    pub snapshot_size_threshold: Option<u64>,
    pub max_entry_size: u64,
    pub rpc_timeout_ms: u64,
    pub grpc_max_message_size: u64,
}

impl Default for RaftNodeConfig {
    fn default() -> Self {
        Self {
            node_id: 1,
            group_id: crate::cluster::DEFAULT_GROUP_ID,
            election_timeout_min: 500,
            election_timeout_max: 1000,
            heartbeat_interval: 100,
            max_payload_entries: 100,
            snapshot_logs_since_last: 1000,
            snapshot_size_threshold: None,
            max_entry_size: 8 * 1024 * 1024,
            rpc_timeout_ms: 200,
            grpc_max_message_size: 64 * 1024 * 1024,
        }
    }
}

impl RaftNodeConfig {
    pub fn validate(&self) -> Result<(), ClusterError> {
        if self.election_timeout_min >= self.election_timeout_max {
            return Err(ClusterError::InvalidConfig(
                "election_timeout_min must be < election_timeout_max".into(),
            ));
        }
        if self.heartbeat_interval >= self.election_timeout_min {
            return Err(ClusterError::InvalidConfig(
                "heartbeat_interval must be < election_timeout_min".into(),
            ));
        }
        if self.max_payload_entries == 0 {
            return Err(ClusterError::InvalidConfig(
                "max_payload_entries must be > 0".into(),
            ));
        }
        if self.rpc_timeout_ms >= self.election_timeout_min {
            return Err(ClusterError::InvalidConfig(
                "rpc_timeout_ms must be < election_timeout_min".into(),
            ));
        }
        if let Some(0) = self.snapshot_size_threshold {
            return Err(ClusterError::InvalidConfig(
                "snapshot_size_threshold must be > 0".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serde_roundtrip() {
        use openraft::{CommittedLeaderId, Entry, EntryPayload, LogId};
        let entry = Entry::<TypeConfig> {
            log_id: LogId::new(CommittedLeaderId::new(1, 1), 1),
            payload: EntryPayload::Normal(Request::Put {
                key: b"k".to_vec(),
                value: b"v".to_vec(),
            }),
        };
        let bytes = rmp_serde::to_vec(&entry).unwrap();
        let decoded: Entry<TypeConfig> = rmp_serde::from_slice(&bytes).unwrap();
        assert!(matches!(
            decoded.payload,
            EntryPayload::Normal(Request::Put { .. })
        ));
    }

    #[test]
    fn test_request_to_batch_conversion() {
        let batch = Request::Put {
            key: b"a".to_vec(),
            value: b"b".to_vec(),
        }
        .to_batch();
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn test_raft_config_default_values() {
        let cfg = RaftNodeConfig::default();
        assert_eq!(cfg.max_payload_entries, 100);
        assert_eq!(cfg.max_entry_size, 8 * 1024 * 1024);
        assert_eq!(cfg.rpc_timeout_ms, 200);
        assert_eq!(cfg.grpc_max_message_size, 64 * 1024 * 1024);
        assert_eq!(cfg.heartbeat_interval, 100);
        assert_eq!(cfg.election_timeout_min, 500);
        assert_eq!(cfg.election_timeout_max, 1000);
        assert_eq!(cfg.snapshot_logs_since_last, 1000);
    }

    #[test]
    fn test_raft_config_validation() {
        assert!(RaftNodeConfig::default().validate().is_ok());
        let cfg = RaftNodeConfig {
            election_timeout_min: 2000,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }
}
