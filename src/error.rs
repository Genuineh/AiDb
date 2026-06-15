//! 错误类型: 使用 thiserror 提供类型化错误, 不用 anyhow.
//! 库应返回类型化错误, 让调用方自行处理.

#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("I/O 错误: {0}")]
  Io(#[from] std::io::Error),

  #[error("数据损坏: {0}")]
  Corruption(String),

  #[error("操作繁忙: {0}")]
  Busy(String),

  #[error("未找到")]
  NotFound,

  #[error("参数错误: {0}")]
  InvalidArgument(String),

  #[error("非法状态: {0}")]
  InvalidState(String),

  #[cfg(feature = "cluster")]
  #[error("集群错误: {0}")]
  Cluster(#[from] ClusterError),
}

#[cfg(feature = "cluster")]
#[derive(Debug, thiserror::Error)]
pub enum ClusterError {
  #[error("Raft 错误: {0}")]
  Raft(String),

  #[error("I/O 错误: {0}")]
  Io(#[from] std::io::Error),

  #[error("序列化错误: {0}")]
  Serialization(String),

  #[error("不是 Leader (当前 Leader: {leader:?}, 地址: {leader_addr:?}, ASK: {is_ask})")]
  NotLeader {
    leader: Option<u64>,
    leader_addr: Option<String>,
    is_ask: bool,
  },

  #[error("集群配置错误: {0}")]
  InvalidConfig(String),

  #[error("操作超时: {0}")]
  Timeout(String),

  #[error("非法状态: {0}")]
  InvalidState(String),

  #[error("内部状态错误: {0}")]
  Internal(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn error_display() {
    let e = Error::NotFound;
    assert_eq!(e.to_string(), "未找到");

    let e = Error::Corruption("checksum mismatch".to_string());
    assert!(e.to_string().contains("checksum mismatch"));
  }

  #[test]
  fn error_from_io() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let e = Error::from(io_err);
    assert!(matches!(e, Error::Io(_)));
  }

  #[cfg(feature = "cluster")]
  #[test]
  fn test_cluster_error_display() {
    let e = ClusterError::NotLeader {
      leader: Some(2),
      leader_addr: Some("127.0.0.1:8001".to_string()),
      is_ask: false,
    };
    assert!(e.to_string().contains("Leader"));
    // Also verify leader_addr=None case works
    let e_no_addr = ClusterError::NotLeader {
      leader: Some(2),
      leader_addr: None,
      is_ask: false,
    };
    assert!(e_no_addr.to_string().contains("Leader"));
  }
}
