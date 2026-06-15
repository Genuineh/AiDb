//! 目录级 checkpoint (Phase 11.6′ MVP) — BGSAVE 语义, 非 Redis RDB.

use crate::engine::db::DB;
use crate::error::{Error, Result};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

/// 目录快照 API (Kvrocks/RocksDB checkpoint 同类).
pub struct Checkpoint;

struct CheckpointGuard<'a>(&'a DB);

impl Drop for CheckpointGuard<'_> {
  fn drop(&mut self) {
    self.0.leave_checkpoint();
  }
}

impl Checkpoint {
  /// 创建一致性目录快照. 见 `06-persistence` 0.2.1 并发协议.
  #[tracing::instrument(name = "bgsave_checkpoint", skip(db), fields(dest = %dest.as_ref().display()))]
  pub fn create(db: &DB, dest: impl AsRef<Path>) -> Result<PathBuf> {
    let dest = dest.as_ref();
    if dest.exists() {
      remove_dir_if_exists(dest)?;
    }

    db.flush()?;

    db.enter_checkpoint();
    let _guard = CheckpointGuard(db);

    // Pin SST readers so compaction cannot unlink while we link/copy.
    let _pinned = db.pin_sstables();

    let tmp = checkpoint_tmp_path(dest);
    if tmp.exists() {
      remove_dir_if_exists(&tmp)?;
    }
    fs::create_dir_all(&tmp)?;

    let files = db.collect_checkpoint_file_paths()?;
    let file_count = files.len();
    for src in &files {
      let rel = src
        .strip_prefix(db.path())
        .map_err(|_| Error::InvalidArgument("checkpoint path outside db dir".into()))?;
      let dst = tmp.join(rel);
      if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
      }
      link_or_copy(src, &dst)?;
    }

    sync_dir(&tmp)?;
    if dest.exists() {
      remove_dir_if_exists(dest)?;
    }
    fs::rename(&tmp, dest)?;
    if let Some(parent) = dest.parent() {
      sync_dir(parent)?;
    }

    tracing::info!(
      target: "db",
      dest = %dest.display(),
      file_count,
      "checkpoint.create.complete"
    );
    Ok(dest.to_path_buf())
  }

  /// smoke: backup 目录可被 `DB::open`.
  pub fn verify_openable(
    backup_dir: impl AsRef<Path>,
    options: crate::config::Options,
  ) -> Result<()> {
    let _db = DB::open(backup_dir, options)?;
    Ok(())
  }
}

fn checkpoint_tmp_path(dest: &Path) -> PathBuf {
  let name = dest
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("backup");
  dest
    .parent()
    .map(|p| p.join(format!("{name}.tmp")))
    .unwrap_or_else(|| PathBuf::from(format!("{name}.tmp")))
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
  if path.exists() {
    fs::remove_dir_all(path)?;
  }
  Ok(())
}

fn link_or_copy(src: &Path, dst: &Path) -> Result<()> {
  match fs::hard_link(src, dst) {
    Ok(()) => Ok(()),
    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
    Err(_) => {
      fs::copy(src, dst)?;
      Ok(())
    }
  }
}

fn sync_dir(path: &Path) -> Result<()> {
  let dir = File::open(path)?;
  dir.sync_all()?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::config::Options;
  use std::sync::Arc;
  use std::thread;
  use std::time::Duration;
  use tempfile::tempdir;

  fn open_db(dir: &Path) -> Arc<DB> {
    DB::open(dir, Options::for_testing()).unwrap()
  }

  #[test]
  fn test_checkpoint_create_empty_db() {
    let dir = tempdir().unwrap();
    let db = open_db(dir.path());
    let backup = dir.path().join("backup");
    let path = Checkpoint::create(&db, &backup).unwrap();
    assert_eq!(path, backup);
    Checkpoint::verify_openable(&backup, Options::for_testing()).unwrap();
  }

  #[test]
  fn test_checkpoint_after_puts() {
    let dir = tempdir().unwrap();
    let db = open_db(dir.path());
    db.put(b"k1", b"v1").unwrap();
    db.put(b"k2", b"v2").unwrap();
    db.flush().unwrap();

    let backup = dir.path().join("backup");
    Checkpoint::create(&db, &backup).unwrap();

    let restored = DB::open(&backup, Options::for_testing()).unwrap();
    assert_eq!(restored.get(b"k1").unwrap(), Some(b"v1".to_vec()));
    assert_eq!(restored.get(b"k2").unwrap(), Some(b"v2".to_vec()));
  }

  #[test]
  fn test_checkpoint_after_delete() {
    let dir = tempdir().unwrap();
    let db = open_db(dir.path());
    db.put(b"gone", b"x").unwrap();
    db.flush().unwrap();
    db.delete(b"gone").unwrap();
    db.flush().unwrap();

    let backup = dir.path().join("backup");
    Checkpoint::create(&db, &backup).unwrap();

    let restored = DB::open(&backup, Options::for_testing()).unwrap();
    assert_eq!(restored.get(b"gone").unwrap(), None);
  }

  #[test]
  fn test_checkpoint_overwrites_existing() {
    let dir = tempdir().unwrap();
    let db = open_db(dir.path());
    db.put(b"k1", b"v1").unwrap();
    db.flush().unwrap();

    let backup = dir.path().join("backup");
    Checkpoint::create(&db, &backup).unwrap();
    fs::write(backup.join("marker"), b"stale").unwrap();

    db.put(b"k2", b"v2").unwrap();
    db.flush().unwrap();
    Checkpoint::create(&db, &backup).unwrap();

    let restored = DB::open(&backup, Options::for_testing()).unwrap();
    assert_eq!(restored.get(b"k1").unwrap(), Some(b"v1".to_vec()));
    assert_eq!(restored.get(b"k2").unwrap(), Some(b"v2".to_vec()));
    assert!(!backup.join("marker").exists());
  }

  #[test]
  fn test_checkpoint_during_compaction() {
    let dir = tempdir().unwrap();
    let mut opts = Options::for_testing();
    opts.background_compaction = true;
    opts.level0_compaction_trigger = 2;
    opts.memtable_size = 4096;
    let db = DB::open(dir.path(), opts).unwrap();

    for i in 0..200u32 {
      db.put(format!("key_{i:04}").as_bytes(), b"value").unwrap();
    }
    db.flush().unwrap();

    let backup = dir.path().join("backup");
    let db_clone = Arc::clone(&db);
    let backup_for_thread = backup.clone();
    let checkpoint_handle = thread::spawn(move || {
      thread::sleep(Duration::from_millis(10));
      Checkpoint::create(&db_clone, &backup_for_thread).unwrap();
    });

    for i in 200..400u32 {
      db.put(format!("key_{i:04}").as_bytes(), b"value").unwrap();
    }
    let _ = db.drain_compactions();

    checkpoint_handle.join().unwrap();
    Checkpoint::verify_openable(&backup, Options::for_testing()).unwrap();
  }
}
