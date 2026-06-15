//! MemTable 功能测试 — InternalKey / CRUD / Iterator / freeze

use aidb::engine::memtable::{
  compare_internal_key, encode_internal_key, extract_sequence, MemTable, ValueType, K_MAX_SEQUENCE,
};

#[test]
fn test_internal_key_encode_decode_roundtrip() {
  let enc = encode_internal_key(b"hello", 42, ValueType::TypeDelete);
  let dec = aidb::engine::memtable::decode_internal_key(&enc).unwrap();
  assert_eq!(dec.0, b"hello");
  assert_eq!(dec.1, 42);
  assert_eq!(dec.2, ValueType::TypeDelete);
}

#[test]
fn test_internal_key_ordering_sequence_desc() {
  let old = encode_internal_key(b"k", 1, ValueType::TypePut);
  let new = encode_internal_key(b"k", 2, ValueType::TypePut);
  assert_eq!(compare_internal_key(&new, &old), std::cmp::Ordering::Less);
}

#[test]
fn test_compare_internal_key() {
  let a = encode_internal_key(b"x", 10, ValueType::TypePut);
  let b = encode_internal_key(b"y", 1, ValueType::TypePut);
  assert_eq!(compare_internal_key(&a, &b), std::cmp::Ordering::Less);
}

#[test]
fn test_memtable_put_get() {
  let mt = MemTable::new();
  mt.put(b"k", b"v", 1).unwrap();
  assert_eq!(mt.get_latest(b"k").unwrap(), Some(b"v".to_vec()));
}

#[test]
fn test_memtable_get_not_found() {
  let mt = MemTable::new();
  assert_eq!(mt.get_latest(b"missing").unwrap(), None);
}

#[test]
fn test_memtable_delete() {
  let mt = MemTable::new();
  mt.put(b"k", b"v", 1).unwrap();
  mt.delete(b"k", 2).unwrap();
  assert_eq!(mt.get_latest(b"k").unwrap(), None);
}

#[test]
fn test_memtable_overwrite() {
  let mt = MemTable::new();
  mt.put(b"k", b"v1", 1).unwrap();
  mt.put(b"k", b"v2", 2).unwrap();
  assert_eq!(mt.get_latest(b"k").unwrap(), Some(b"v2".to_vec()));
}

#[test]
fn test_memtable_snapshot() {
  let mt = MemTable::new();
  mt.put(b"k", b"v1", 1).unwrap();
  mt.put(b"k", b"v2", 2).unwrap();
  assert_eq!(mt.get(b"k", 1).unwrap(), Some(b"v1".to_vec()));
  assert_eq!(mt.get(b"k", 2).unwrap(), Some(b"v2".to_vec()));
  assert_eq!(mt.get(b"k", K_MAX_SEQUENCE).unwrap(), Some(b"v2".to_vec()));
}

#[test]
fn test_memtable_search() {
  let mt = MemTable::new();
  mt.put(b"k", b"v", 5).unwrap();
  let seek = encode_internal_key(b"k", 5, ValueType::TypePut);
  let (val, ty) = mt.search(&seek).unwrap().unwrap();
  assert_eq!(val, b"v");
  assert_eq!(ty, ValueType::TypePut);

  let seek_old = encode_internal_key(b"k", 3, ValueType::TypePut);
  assert!(mt.search(&seek_old).unwrap().is_none());

  mt.delete(b"k", 6).unwrap();
  let seek_after = encode_internal_key(b"k", 6, ValueType::TypePut);
  let (_, ty) = mt.search(&seek_after).unwrap().unwrap();
  assert_eq!(ty, ValueType::TypeDelete);
}

#[test]
fn test_memtable_iterator_seek() {
  let mt = MemTable::new();
  mt.put(b"a", b"1", 1).unwrap();
  mt.put(b"b", b"2", 2).unwrap();
  let mut it = mt.iter();
  it.seek(b"b");
  assert!(it.valid());
  assert_eq!(it.key(), encode_internal_key(b"b", 2, ValueType::TypePut));
}

#[test]
fn test_memtable_iterator_seek_to_first() {
  let mt = MemTable::new();
  mt.put(b"z", b"z", 1).unwrap();
  mt.put(b"a", b"a", 2).unwrap();
  let mut it = mt.iter();
  it.seek_to_first();
  assert!(it.valid());
  let seq = extract_sequence(it.key()).unwrap();
  assert_eq!(seq, 2);
}

#[test]
fn test_memtable_iterator_next() {
  let mt = MemTable::new();
  mt.put(b"a", b"1", 1).unwrap();
  mt.put(b"b", b"2", 2).unwrap();
  let mut it = mt.iter();
  it.seek_to_first();
  let mut keys = Vec::new();
  while it.valid() {
    keys.push(aidb::engine::memtable::extract_user_key(it.key()).to_vec());
    if !it.next() {
      break;
    }
  }
  assert_eq!(keys, vec![b"a".to_vec(), b"b".to_vec()]);
}

#[test]
fn test_memtable_iterator_prev() {
  use aidb::engine::memtable::extract_user_key;

  let mt = MemTable::new();
  mt.put(b"a", b"1", 1).unwrap();
  mt.put(b"b", b"2", 2).unwrap();
  mt.put(b"c", b"3", 3).unwrap();

  let mut it = mt.iter();
  assert!(!it.valid());
  assert!(it.prev());
  assert_eq!(extract_user_key(it.key()), b"c");

  assert!(it.prev());
  assert_eq!(extract_user_key(it.key()), b"b");

  assert!(it.prev());
  assert_eq!(extract_user_key(it.key()), b"a");

  assert!(!it.prev());
  assert!(!it.valid());

  it.seek(b"b");
  assert!(it.valid());
  assert_eq!(extract_user_key(it.key()), b"b");
  assert!(it.prev());
  assert_eq!(extract_user_key(it.key()), b"a");
}

#[test]
fn test_memtable_size_approx() {
  let mt = MemTable::new();
  assert_eq!(mt.approximate_size(), 0);
  mt.put(b"ab", b"cd", 1).unwrap();
  assert_eq!(mt.approximate_size(), 4);
  mt.delete(b"x", 2).unwrap();
  assert_eq!(mt.approximate_size(), 5);
}

#[test]
fn test_immutable_freeze() {
  let mt = MemTable::new();
  mt.put(b"k", b"v", 1).unwrap();
  let frozen = mt.freeze(1);
  assert_eq!(frozen.flush_seq(), 1);
  assert_eq!(frozen.get_latest(b"k").unwrap(), Some(b"v".to_vec()));

  let active = MemTable::new();
  active.put(b"k2", b"v2", 2).unwrap();
  assert_eq!(active.get_latest(b"k2").unwrap(), Some(b"v2".to_vec()));
  assert_eq!(frozen.get_latest(b"k2").unwrap(), None);
}

#[test]
fn test_memtable_empty_user_key() {
  let mt = MemTable::new();
  mt.put(b"", b"v", 1).unwrap();
  assert_eq!(mt.get_latest(b"").unwrap(), Some(b"v".to_vec()));
}

#[test]
fn test_internal_key_value_type_tiebreak() {
  let put = encode_internal_key(b"k", 1, ValueType::TypePut);
  let del = encode_internal_key(b"k", 1, ValueType::TypeDelete);
  assert_eq!(compare_internal_key(&put, &del), std::cmp::Ordering::Less);
}

#[test]
fn test_decode_internal_key_corruption() {
  assert!(aidb::engine::memtable::decode_internal_key(b"short").is_err());
  let mut bad = encode_internal_key(b"k", 1, ValueType::TypePut);
  *bad.last_mut().unwrap() = 9;
  assert!(aidb::engine::memtable::decode_internal_key(&bad).is_err());
}

#[test]
fn test_memtable_sequence_overflow() {
  use aidb::engine::memtable::SEQUENCE_LIMIT;
  let mt = MemTable::new();
  assert!(mt.put(b"k", b"v", SEQUENCE_LIMIT).is_err());
}
