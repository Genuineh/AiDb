//! 原子批量写.

use std::collections::VecDeque;

/// `write` / `write_without_wal` 成功后的键变更摘要.
///
/// `inserted` / `deleted` 为批内按 overlay 判定的 per-op 累计
/// (Put 且先前不存在 → +inserted; Delete 且先前存在 → +deleted).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub struct EngineWriteStats {
    pub inserted: u64,
    pub deleted: u64,
}

/// 单条写操作.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOp {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

/// 批量写: 共享连续 sequence, 一次 WAL sync.
#[derive(Debug, Default)]
pub struct WriteBatch {
    pub(crate) operations: VecDeque<WriteOp>,
    approximate_size: usize,
}

impl WriteBatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
        let key = key.into();
        let value = value.into();
        self.approximate_size += key.len() + value.len();
        self.operations.push_back(WriteOp::Put { key, value });
    }

    pub fn delete(&mut self, key: impl Into<Vec<u8>>) {
        let key = key.into();
        self.approximate_size += key.len();
        self.operations.push_back(WriteOp::Delete { key });
    }

    pub fn len(&self) -> usize {
        self.operations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    pub fn clear(&mut self) {
        self.operations.clear();
        self.approximate_size = 0;
    }

    pub fn approximate_size(&self) -> usize {
        self.approximate_size
    }
}
