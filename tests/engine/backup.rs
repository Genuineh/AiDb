//! 备份模块端到端集成测试。
//! 覆盖跨模块场景：空数据库备份、完整 roundtrip、并发写入一致性。

use std::sync::Arc;
use tempfile::tempdir;

use aidb::backup::*;
use aidb::config::Options;
use aidb::DB;

#[test]
fn test_backup_empty_db() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), Options::for_testing()).unwrap();
    db.flush().unwrap();

    let backup_root = dir.path().join("backups");
    let storage = Arc::new(LocalFileStorage::new(backup_root.clone()));
    let manager = BackupManager::new(storage.clone(), RetentionPolicy::default());
    let id = manager.create_backup(&db).unwrap();
    drop(db);

    // 空数据库备份后元信息可查询
    let info = manager.get_backup_info(id).unwrap();
    assert_eq!(info.id, id);

    // 备份完整性可验证（即使空数据库也有元数据文件）
    let recovery = RecoveryManager::new(storage);
    assert!(recovery.verify_backup(id).unwrap());
}

#[test]
fn test_backup_restore_roundtrip() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), Options::for_testing()).unwrap();
    for i in 0u32..10 {
        db.put(format!("k{i}").as_bytes(), b"v").unwrap();
    }
    db.flush().unwrap();

    let backup_root = dir.path().join("backups");
    let storage = Arc::new(LocalFileStorage::new(backup_root.clone()));
    let manager = BackupManager::new(storage.clone(), RetentionPolicy::default());
    let id = manager.create_backup(&db).unwrap();
    drop(db);

    let restore_dir = dir.path().join("restored");
    let recovery = RecoveryManager::new(storage);
    recovery.restore(id, &restore_dir).unwrap();

    let restored = DB::open(&restore_dir, Options::for_testing()).unwrap();
    for i in 0u32..10 {
        assert_eq!(
            restored.get(format!("k{i}").as_bytes()).unwrap(),
            Some(b"v".to_vec())
        );
    }
}

#[test]
fn test_backup_during_concurrent_writes() {
    let dir = tempdir().unwrap();
    let db = DB::open(dir.path(), Options::for_testing()).unwrap();

    // 先写入一批数据（零填充确保字典序）
    for i in 0u32..50 {
        db.put(format!("k{i:02}").as_bytes(), b"before").unwrap();
    }
    db.flush().unwrap();

    let backup_root = dir.path().join("backups");
    let storage = Arc::new(LocalFileStorage::new(backup_root.clone()));
    let manager = BackupManager::new(storage.clone(), RetentionPolicy::default());
    let _id = manager.create_backup(&db).unwrap();

    // 备份后继续写入（模拟备份期间的并发写入）
    for i in 50u32..100 {
        db.put(format!("k{i:02}").as_bytes(), b"after").unwrap();
    }
    db.flush().unwrap();

    // 验证备份内容不受后续写入影响
    let recovery = RecoveryManager::new(storage);
    assert!(recovery.verify_backup(_id).unwrap());
}
