//! SkipMap 排序键包装.

use super::internal_key::compare_internal_key;
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) struct InternalKeyBytes(pub Arc<[u8]>);

impl InternalKeyBytes {
  pub(crate) fn from_slice(bytes: &[u8]) -> Self {
    Self(Arc::from(bytes))
  }
}

impl AsRef<[u8]> for InternalKeyBytes {
  fn as_ref(&self) -> &[u8] {
    &self.0
  }
}

impl Borrow<[u8]> for InternalKeyBytes {
  fn borrow(&self) -> &[u8] {
    &self.0
  }
}

impl PartialEq for InternalKeyBytes {
  fn eq(&self, other: &Self) -> bool {
    self.cmp(other) == Ordering::Equal
  }
}

impl Eq for InternalKeyBytes {}

impl PartialOrd for InternalKeyBytes {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for InternalKeyBytes {
  fn cmp(&self, other: &Self) -> Ordering {
    compare_internal_key(self.as_ref(), other.as_ref())
  }
}
