//! 可靠性: 未 close 崩溃、多轮 restart (roadmap 5.10)

use tempfile::tempdir;
use aidb::config::Options;
use aidb::DB;

fn opts() -> Options {
  let mut o = Options::for_testing();
  o.memtable_size = 64 * 1024;
  o.sync_wal = true;
  o
}

#[test]
fn test_crash_recovery_without_close() {
  let dir = tempdir().unwrap();
  {
    let db = DB::open(dir.path(), opts()).unwrap();
    db.put(b"crash1", b"data1").unwrap();
    db.put(b"crash2", b"data2").unwrap();
    // 故意不 close — 模拟进程崩溃 (Drop 会 sync WAL 并释放 LOCK, 不调用 close())
  }
  let db = DB::open(dir.path(), opts()).unwrap();
  assert_eq!(db.get(b"crash1").unwrap(), Some(b"data1".to_vec()));
  assert_eq!(db.get(b"crash2").unwrap(), Some(b"data2".to_vec()));
  db.close().unwrap();
}

#[test]
fn test_crash_recovery_multiple_restarts() {
  let dir = tempdir().unwrap();
  for round in 0..3u8 {
    let db = DB::open(dir.path(), opts()).unwrap();
    let key = format!("key{round}");
    db.put(key.as_bytes(), &[round]).unwrap();
    db.close().unwrap();
  }
  let db = DB::open(dir.path(), opts()).unwrap();
  for round in 0..3u8 {
    let key = format!("key{round}");
    assert_eq!(db.get(key.as_bytes()).unwrap(), Some(vec![round]));
  }
  db.close().unwrap();
}
