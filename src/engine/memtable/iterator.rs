//! MemTable 迭代器: seek / seek_to_first / next / prev.

use super::internal_key::{encode_internal_key, ValueType, K_MAX_SEQUENCE};
use super::key_bytes::InternalKeyBytes;
use super::MemTable;
use crossbeam_skiplist::map::Entry;
use crossbeam_skiplist::SkipMap;
use std::ops::Bound;
use std::sync::Arc;

/// MemTable 前向迭代器.
pub struct MemTableIterator<'a> {
  table: &'a SkipMap<InternalKeyBytes, Arc<[u8]>>,
  current: Option<Entry<'a, InternalKeyBytes, Arc<[u8]>>>,
}

impl<'a> MemTableIterator<'a> {
  pub(crate) fn new(table: &'a MemTable) -> Self {
    Self {
      table: table.map(),
      current: None,
    }
  }

  /// 定位到 >= target_user_key 的第一个 entry (含所有版本).
  pub fn seek(&mut self, target_user_key: &[u8]) {
    let encoded = encode_internal_key(target_user_key, K_MAX_SEQUENCE, ValueType::TypePut);
    let seek = InternalKeyBytes::from_slice(&encoded);
    self.current = self.table.lower_bound(Bound::Included(&seek));
  }

  pub fn seek_to_first(&mut self) {
    self.current = self.table.front();
  }

  #[allow(clippy::should_implement_trait)]
  pub fn next(&mut self) -> bool {
    self.current = match self.current.take() {
      None => self.table.front(),
      Some(entry) => entry.next(),
    };
    self.valid()
  }

  pub fn valid(&self) -> bool {
    self.current.is_some()
  }

  pub fn key(&self) -> &[u8] {
    self
      .current
      .as_ref()
      .expect("iterator not valid")
      .key()
      .as_ref()
  }

  pub fn value(&self) -> &[u8] {
    self
      .current
      .as_ref()
      .expect("iterator not valid")
      .value()
      .as_ref()
  }

  #[allow(clippy::should_implement_trait)]
  pub fn prev(&mut self) -> bool {
    self.current = match self.current.take() {
      None => self.table.back(),
      Some(entry) => entry.prev(),
    };
    self.valid()
  }
}
