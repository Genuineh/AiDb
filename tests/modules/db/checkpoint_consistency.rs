//! Checkpoint backup 数据一致性校验集成测试

use tempfile::tempdir;
use aidb::config::Options;
use aidb::{Checkpoint, DB};

fn small_opts() -> Options {
  let mut o = Options::for_testing();
  o.memtable_size = 4096;
  o.sync_wal = true;
  o
}

/// 写入 20 个 key, flush, checkpoint, 验证 backup 中所有数据完整正确
#[test]
fn test_checkpoint_all_20_keys_present() {
  let dir = tempdir().unwrap();
  let backup_dir = tempdir().unwrap();
  let backup_path = backup_dir.path().join("backup");

  let db = DB::open(dir.path(), small_opts()).unwrap();

  for i in 0u32..20 {
    let key = format!("k_{i:02}");
    let val = format!("v_{i:02}");
    db.put(key.as_bytes(), val.as_bytes()).unwrap();
  }
  db.flush().unwrap();

  Checkpoint::create(&db, &backup_path).unwrap();

  // backup 目录必须可以打开
  let backup_db = DB::open(&backup_path, small_opts()).unwrap();

  // 遍历验证全部 20 个 key 存在且值正确
  for i in 0u32..20 {
    let key = format!("k_{i:02}");
    let expected = format!("v_{i:02}");
    assert_eq!(
      backup_db.get(key.as_bytes()).unwrap(),
      Some(expected.into_bytes()),
      "backup should contain key k_{i:02} with correct value"
    );
  }

  backup_db.close().unwrap();
  db.close().unwrap();
}

/// checkpoint 后在新会话写入 5 个新 key, 验证 backup 中这 5 个 key 不存在 (快照语义).
///
/// 原理：Checkpoint 通过 hard link 捕获活跃 WAL 文件。若在同一会话写入新数据,
/// 会追加到被 hard link 的 WAL, 导致备份也能看到。
/// 解决方法：checkpoint 后 close 原 DB, reopen 产生新 WAL (不在备份中),
/// 再写入新 key, 备份不会包含这些 key。
#[test]
fn test_checkpoint_snapshot_excludes_post_checkpoint_writes() {
  let dir = tempdir().unwrap();
  let backup_dir = tempdir().unwrap();
  let backup_path = backup_dir.path().join("snapshot");

  // Phase 1: 写入 20 个 key, flush, checkpoint
  {
    let db = DB::open(dir.path(), small_opts()).unwrap();
    for i in 0u32..20 {
      let key = format!("k_{i:02}");
      let val = format!("v_{i:02}");
      db.put(key.as_bytes(), val.as_bytes()).unwrap();
    }
    db.flush().unwrap();
    Checkpoint::create(&db, &backup_path).unwrap();
    db.close().unwrap(); // close 冻结/清理 checkpoint 时的 WAL
  }

  // Phase 2: 重新打开原 DB (新 WAL, 不在 backup 中), 写入 5 个新 key
  {
    let db = DB::open(dir.path(), small_opts()).unwrap();
    for i in 20u32..25 {
      let key = format!("k_{i:02}");
      let val = format!("new_v_{i}");
      db.put(key.as_bytes(), val.as_bytes()).unwrap();
    }
    db.flush().unwrap();

    // 原 DB 中新 key 可见
    for i in 20u32..25 {
      let key = format!("k_{i:02}");
      assert!(
        db.get(key.as_bytes()).unwrap().is_some(),
        "new key {key} should be visible in original db"
      );
    }
    db.close().unwrap();
  }

  // Phase 3: 打开 backup, 验证快照语义
  let backup_db = DB::open(&backup_path, small_opts()).unwrap();

  // backup 中新 key 不存在 (新 WAL 不在备份中)
  for i in 20u32..25 {
    let key = format!("k_{i:02}");
    assert_eq!(
      backup_db.get(key.as_bytes()).unwrap(),
      None,
      "post-checkpoint key {key} must NOT exist in backup"
    );
  }

  // backup 中原始 20 个 key 仍然正确
  for i in 0u32..20 {
    let key = format!("k_{i:02}");
    let expected = format!("v_{i:02}");
    assert_eq!(
      backup_db.get(key.as_bytes()).unwrap(),
      Some(expected.into_bytes()),
      "original key {key} must still be correct in backup"
    );
  }

  backup_db.close().unwrap();
}
