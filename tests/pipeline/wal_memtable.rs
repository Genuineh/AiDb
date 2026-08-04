//! WAL recover → MemTable replay
//! @component aidb-engine

use aidb::config::Options;
use aidb::engine::db::replay::replay_entries;
use aidb::engine::memtable::MemTable;
use aidb::engine::wal::manager::WALManager;
use aidb::engine::wal::record::{OpType, WalEntry};
use std::sync::Arc;
use tempfile::tempdir;

fn test_opts() -> Arc<Options> {
    Arc::new(Options::for_testing())
}

fn put(seq: u64, key: &[u8], value: &[u8]) -> WalEntry {
    WalEntry {
        sequence: seq,
        op_type: OpType::TypePut,
        has_value: true,
        key: key.to_vec(),
        value: Some(value.to_vec()),
    }
}

fn delete(seq: u64, key: &[u8]) -> WalEntry {
    WalEntry {
        sequence: seq,
        op_type: OpType::TypeDelete,
        has_value: false,
        key: key.to_vec(),
        value: None,
    }
}

/// 验证 WAL 重放写回 MemTable 数据一致性
#[test]
fn test_wal_memtable_consistency() {
    let dir = tempdir().unwrap();
    {
        let mut wal = WALManager::open(dir.path(), 1, 100, test_opts()).unwrap();
        wal.append(&put(1, b"k1", b"v1").encode()).unwrap();
        wal.append(&put(2, b"k2", b"v2").encode()).unwrap();
        wal.close().unwrap();
    }
    let recovery = WALManager::recover(dir.path(), test_opts()).unwrap();
    let mem = MemTable::new();
    replay_entries(&mem, &recovery.entries).unwrap();
    assert_eq!(mem.get_latest(b"k1").unwrap(), Some(b"v1".to_vec()));
    assert_eq!(mem.get_latest(b"k2").unwrap(), Some(b"v2".to_vec()));
    assert_eq!(recovery.max_sequence, 2);
}

/// 验证 WAL 删除记录 (TypeDelete) 重放到 MemTable 生效
#[test]
fn test_wal_memtable_delete_replay() {
    let dir = tempdir().unwrap();
    {
        let mut wal = WALManager::open(dir.path(), 1, 100, test_opts()).unwrap();
        wal.append(&put(10, b"k", b"v").encode()).unwrap();
        wal.append(&delete(11, b"k").encode()).unwrap();
        wal.close().unwrap();
    }
    let recovery = WALManager::recover(dir.path(), test_opts()).unwrap();
    let mem = MemTable::new();
    replay_entries(&mem, &recovery.entries).unwrap();
    assert_eq!(mem.get_latest(b"k").unwrap(), None);
}

/// 验证 异常 Crash 后 WAL 日志成功恢复到 MemTable
#[test]
fn test_wal_memtable_crash() {
    let dir = tempdir().unwrap();
    {
        let mut wal = WALManager::open(dir.path(), 1, 100, test_opts()).unwrap();
        wal.append(&put(100, b"crash", b"ok").encode()).unwrap();
        wal.sync().unwrap();
    }
    let recovery = WALManager::recover(dir.path(), test_opts()).unwrap();
    let mem = MemTable::new();
    replay_entries(&mem, &recovery.entries).unwrap();
    assert_eq!(mem.get_latest(b"crash").unwrap(), Some(b"ok".to_vec()));
}

/// 验证 跨多个 WAL 文件的日志链式重放到 MemTable
#[test]
fn test_wal_memtable_multi_file_replay() {
    let dir = tempdir().unwrap();
    {
        let mut wal = WALManager::open(dir.path(), 1, 100, test_opts()).unwrap();
        wal.append(&put(1, b"a", b"1").encode()).unwrap();
        wal.rotate(200).unwrap();
        wal.append(&put(2, b"b", b"2").encode()).unwrap();
    }
    let recovery = WALManager::recover(dir.path(), test_opts()).unwrap();
    assert_eq!(recovery.entries.len(), 2);
    let mem = MemTable::new();
    replay_entries(&mem, &recovery.entries).unwrap();
    assert_eq!(mem.get_latest(b"a").unwrap(), Some(b"1".to_vec()));
    assert_eq!(mem.get_latest(b"b").unwrap(), Some(b"2".to_vec()));
}

/// 验证 不完整 Batch 截断在 MemTable 重放中回滚
#[test]
fn test_wal_memtable_batch_truncation_empty() {
    let dir = tempdir().unwrap();
    {
        let mut wal = WALManager::open(dir.path(), 1, 100, test_opts()).unwrap();
        let batch = WalEntry {
            sequence: 0,
            op_type: OpType::BatchStart,
            has_value: true,
            key: vec![],
            value: Some(3u32.to_le_bytes().to_vec()),
        };
        wal.append(&batch.encode()).unwrap();
        wal.append(&put(100, b"orphan", b"x").encode()).unwrap();
        wal.close().unwrap();
    }
    let recovery = WALManager::recover(dir.path(), test_opts()).unwrap();
    assert!(recovery.entries.is_empty());
    let mem = MemTable::new();
    replay_entries(&mem, &recovery.entries).unwrap();
    assert_eq!(mem.get_latest(b"orphan").unwrap(), None);
}
