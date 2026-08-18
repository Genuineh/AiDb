//! DB::scan 边界测试 + delete_range/compaction 交互测试
//! @component aidb-engine

use aidb::config::Options;
use aidb::DB;
use tempfile::tempdir;

fn small_opts() -> Options {
    let mut o = Options::for_testing();
    o.memtable_size = 4096;
    o.sync_wal = true;
    o
}

// ── scan 边界 ────────────────────────────────────────────────────────────────

#[test]
fn test_scan_empty_db() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), small_opts()).unwrap();
    let items: Vec<_> = db.scan(None, None).unwrap().collect();
    assert!(items.is_empty(), "empty db scan should return no items");
    db.close().unwrap();
}

#[test]
fn test_scan_no_match_range() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), small_opts()).unwrap();
    for c in b'a'..=b'e' {
        db.put(&[c], b"v").unwrap();
    }
    // range that is beyond all keys
    let items: Vec<_> = db
        .scan(Some(b"z"), None)
        .unwrap()
        .map(|r| r.unwrap().0)
        .collect();
    assert!(items.is_empty(), "scan beyond all keys should return empty");
    db.close().unwrap();
}

#[test]
fn test_scan_key_at_start_boundary() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), small_opts()).unwrap();
    for (k, v) in [
        (b"a".as_ref(), b"1".as_ref()),
        (b"b", b"2"),
        (b"c", b"3"),
        (b"d", b"4"),
    ] {
        db.put(k, v).unwrap();
    }
    // scan from "b" to "d" → should include b, c but not d (exclusive end)
    let keys: Vec<_> = db
        .scan(Some(b"b"), Some(b"d"))
        .unwrap()
        .map(|r| r.unwrap().0)
        .collect();
    assert_eq!(keys, vec![b"b".to_vec(), b"c".to_vec()]);
    db.close().unwrap();
}

#[test]
fn test_scan_key_at_end_boundary() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), small_opts()).unwrap();
    for (k, v) in [(b"a".as_ref(), b"1".as_ref()), (b"b", b"2"), (b"c", b"3")] {
        db.put(k, v).unwrap();
    }
    // scan up to "c" (exclusive) → should return a, b
    let keys: Vec<_> = db
        .scan(None, Some(b"c"))
        .unwrap()
        .map(|r| r.unwrap().0)
        .collect();
    assert_eq!(keys, vec![b"a".to_vec(), b"b".to_vec()]);
    db.close().unwrap();
}

#[test]
fn test_scan_full_range_no_bounds() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), small_opts()).unwrap();
    for c in b'a'..=b'e' {
        db.put(&[c], b"v").unwrap();
    }
    let keys: Vec<_> = db.scan(None, None).unwrap().map(|r| r.unwrap().0).collect();
    assert_eq!(keys.len(), 5);
    assert_eq!(keys[0], b"a".to_vec());
    assert_eq!(keys[4], b"e".to_vec());
    db.close().unwrap();
}

#[test]
fn test_scan_after_flush_and_compaction() {
    let dir = tempdir().unwrap();
    let mut opts = small_opts();
    opts.memtable_size = 512; // tiny to trigger flushes
    let db = DB::open(dir.path(), opts).unwrap();

    for i in 0u8..20 {
        db.put(&[i], b"value").unwrap();
    }
    db.flush().unwrap();
    for i in 20u8..40 {
        db.put(&[i], b"value").unwrap();
    }
    db.flush().unwrap();

    let items: Vec<_> = db.scan(None, None).unwrap().map(|r| r.unwrap()).collect();
    assert_eq!(items.len(), 40, "all keys should be visible after flush");

    // verify sequential order
    for (i, (k, _)) in items.iter().enumerate() {
        assert_eq!(k[0], i as u8);
    }
    db.close().unwrap();
}

#[test]
fn test_scan_single_key_range() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), small_opts()).unwrap();
    db.put(b"a", b"1").unwrap();
    db.put(b"b", b"2").unwrap();
    db.put(b"c", b"3").unwrap();

    // scan [b, b+1) — only key "b"
    let keys: Vec<_> = db
        .scan(Some(b"b"), Some(b"b\x00"))
        .unwrap()
        .map(|r| r.unwrap().0)
        .collect();
    assert_eq!(keys, vec![b"b".to_vec()]);
    db.close().unwrap();
}

// ── delete_range / compaction 交互 ────────────────────────────────────────────

#[test]
fn test_delete_range_empty_range() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), small_opts()).unwrap();
    for (k, v) in [(b"a".as_ref(), b"1".as_ref()), (b"b", b"2"), (b"c", b"3")] {
        db.put(k, v).unwrap();
    }
    // start > end — should not delete anything
    db.delete_range(b"z", b"a").unwrap();
    assert_eq!(db.get(b"a").unwrap(), Some(b"1".to_vec()));
    assert_eq!(db.get(b"b").unwrap(), Some(b"2".to_vec()));
    assert_eq!(db.get(b"c").unwrap(), Some(b"3".to_vec()));
    db.close().unwrap();
}

#[test]
fn test_delete_range_after_flush() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), small_opts()).unwrap();
    for i in 0u8..10 {
        db.put(&[i], b"v").unwrap();
    }
    db.flush().unwrap();

    // delete middle range [3, 7)
    db.delete_range(&[3u8], &[7u8]).unwrap();

    for i in 0u8..10 {
        let expected = if !(3..7).contains(&i) {
            Some(b"v".to_vec())
        } else {
            None
        };
        assert_eq!(
            db.get(&[i]).unwrap(),
            expected,
            "key {} unexpected after delete_range",
            i
        );
    }
    db.close().unwrap();
}

#[test]
fn test_delete_range_survives_reopen() {
    let dir = tempdir().unwrap();
    {
        let db = DB::open(dir.path(), small_opts()).unwrap();
        for i in 0u8..10 {
            db.put(&[i], b"v").unwrap();
        }
        db.flush().unwrap();
        db.delete_range(&[3u8], &[7u8]).unwrap();
        db.close().unwrap();
    }
    {
        let db = DB::open(dir.path(), small_opts()).unwrap();
        for i in 0u8..3 {
            assert_eq!(db.get(&[i]).unwrap(), Some(b"v".to_vec()));
        }
        for i in 3u8..7 {
            assert_eq!(db.get(&[i]).unwrap(), None, "key {} should be deleted", i);
        }
        for i in 7u8..10 {
            assert_eq!(db.get(&[i]).unwrap(), Some(b"v".to_vec()));
        }
        db.close().unwrap();
    }
}

#[test]
fn test_delete_range_then_compaction_consistency() {
    let dir = tempdir().unwrap();
    let mut opts = small_opts();
    opts.memtable_size = 512;
    let db = DB::open(dir.path(), opts).unwrap();

    // write 30 keys, flush to L0
    for i in 0u8..30 {
        db.put(&[i], b"value").unwrap();
    }
    db.flush().unwrap();

    // delete middle 10 keys
    db.delete_range(&[10u8], &[20u8]).unwrap();

    // write more keys to trigger another flush + compaction path
    for i in 30u8..40 {
        db.put(&[i], b"value").unwrap();
    }
    db.flush().unwrap();

    // verify surviving keys
    let surviving: Vec<u8> = db
        .scan(None, None)
        .unwrap()
        .map(|r| r.unwrap().0[0])
        .collect();

    for i in 0u8..10 {
        assert!(surviving.contains(&i), "key {} should exist", i);
    }
    for i in 10u8..20 {
        assert!(!surviving.contains(&i), "key {} should be deleted", i);
    }
    for i in 20u8..40 {
        assert!(surviving.contains(&i), "key {} should exist", i);
    }

    db.close().unwrap();
}
