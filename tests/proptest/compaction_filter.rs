//! CompactionFilter property-based 测试:
//! @component aidb-engine
//! 随机 put/delete/flush/compact 操作序列 + 对照模型, 验证 filter 语义.

use aidb::config::Options;
use aidb::engine::compaction::{CompactionFilter, FilterDecision};
use aidb::DB;
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use tempfile::tempdir;

fn test_opts() -> Options {
    let mut o = Options::for_testing();
    o.memtable_size = 512;
    o.level0_compaction_trigger = 2;
    o.sync_wal = false;
    o
}

/// 过滤掉 value 最后一位 == 0 的 entry (偶数).
struct EvenFilter;

impl CompactionFilter for EvenFilter {
    fn filter(&self, _level: usize, _key: &[u8], value: &[u8]) -> FilterDecision {
        if value.last().is_some_and(|b| b % 2 == 0) {
            FilterDecision::Remove
        } else {
            FilterDecision::Keep
        }
    }
}

proptest! {
#![proptest_config(ProptestConfig::with_cases(100))]

/// 随机 put+flush+compact, 验证 CompactionFilter 正确移除了偶数 value 的 entry.
#[test]
fn prop_filter_removes_entries(
    puts in prop::collection::vec((0u8..=12u8, 0u8..=255u8), 1..60),
) {
    let dir = tempdir().unwrap();
    let db = Arc::new(DB::open(dir.path(), test_opts()).unwrap());
    db.set_compaction_filter(Some(Arc::new(EvenFilter)));

    let mut model = BTreeMap::new();
    for &(k, v) in &puts {
        db.put(&[k], &[v]).unwrap();
        model.insert(k, v);
    }
    db.flush().unwrap();
    // 写入 key 直到触发 L0 compaction
    for i in 0..5u8 {
        db.put(&[b'p', i], &[i]).unwrap();
        db.flush().unwrap();
    }
    db.drain_compactions().unwrap();

    for k in 0u8..=12u8 {
        let expected = model.get(&k).and_then(|v| {
            if v % 2 == 0 { None }
            else { Some(vec![*v]) }
        });
        let actual = db.get(&[k]).unwrap();
        prop_assert_eq!(&actual, &expected,
            "key {}: expected={:?}, got={:?}", k, expected, actual);
    }
    db.close().unwrap();
}

/// put+delete交替, filter 不影响 tombstone.
#[test]
fn prop_filter_with_deletes(
    ops in prop::collection::vec(
        prop_oneof![
            (0u8..=10u8, 0u8..=255u8).prop_map(|(k,v)| (false, k, v)),
            (0u8..=10u8).prop_map(|k| (true, k, 0u8)),
        ],
        1..50,
    ),
) {
    let dir = tempdir().unwrap();
    let db = Arc::new(DB::open(dir.path(), test_opts()).unwrap());
    db.set_compaction_filter(Some(Arc::new(EvenFilter)));

    let mut model = BTreeMap::new();
    for &(is_del, k, v) in &ops {
        if is_del {
            db.delete(&[k]).unwrap();
            model.insert(k, None);
        } else {
            db.put(&[k], &[v]).unwrap();
            model.insert(k, Some(v));
        }
    }
    db.flush().unwrap();
    for i in 0..5u8 {
        db.put(&[b'q', i], &[i]).unwrap();
        db.flush().unwrap();
    }
    db.drain_compactions().unwrap();

    for k in 0u8..=10u8 {
        let actual = db.get(&[k]).unwrap();
        match model.get(&k) {
            Some(None) => {
                prop_assert_eq!(&actual, &None,
                    "deleted key {}: got {:?}", k, actual);
            }
            Some(&Some(v)) if v % 2 == 0 => {
                prop_assert_eq!(&actual, &None,
                    "filtered key {}: got {:?}", k, actual);
            }
            Some(&Some(v)) => {
                prop_assert_eq!(&actual, &Some(vec![v]),
                    "unfiltered key {}", k);
            }
            None => {
                prop_assert_eq!(&actual, &None,
                    "never-written key {}", k);
            }
        }
    }
    db.close().unwrap();
}

/// 没有 filter 时所有 entry 保留.
#[test]
fn prop_no_filter_keeps_all(
    puts in prop::collection::vec((0u8..=12u8, 0u8..=255u8), 1..40),
) {
    let dir = tempdir().unwrap();
    let db = Arc::new(DB::open(dir.path(), test_opts()).unwrap());

    let mut model = BTreeMap::new();
    for &(k, v) in &puts {
        db.put(&[k], &[v]).unwrap();
        model.insert(k, v);
    }
    db.flush().unwrap();
    for i in 0..5u8 {
        db.put(&[b'x', i], &[i]).unwrap();
        db.flush().unwrap();
    }
    db.drain_compactions().unwrap();

    for k in 0u8..=12u8 {
        let expected = model.get(&k).map(|v| vec![*v]);
        let actual = db.get(&[k]).unwrap();
        prop_assert_eq!(&actual, &expected, "key {}", k);
    }
    db.close().unwrap();
}

}
