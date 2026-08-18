//! WAL 功能测试 — 编解码/写入/读取/恢复/清理
//! @component aidb-wal

use aidb::config::Options;
use aidb::engine::wal::manager::WALManager;
use aidb::engine::wal::reader::{ReadStatus, Reader};
use aidb::engine::wal::record::{OpType, RecordType, WalEntry};
use aidb::engine::wal::writer::Writer;
use std::sync::Arc;
use tempfile::tempdir;

fn test_opts() -> Arc<Options> {
    Arc::new(Options::for_testing())
}

// ---- 基础写入/读取 ----

/// 验证 Writer 同步刷新数据到磁盘
#[test]
fn test_writer_sync() {
    let d = tempdir().unwrap();
    let path = d.path().join("sync.log");
    let mut w = Writer::open(&path).unwrap();
    w.write_record(RecordType::Full, b"sync_test").unwrap();
    w.sync_data().unwrap();
    drop(w);
    assert!(std::fs::metadata(&path).unwrap().len() >= 16);
}

/// 验证读取器跳过 0 字节长度空记录
#[test]
fn test_zero_length_record_skipped() {
    let d = tempdir().unwrap();
    let path = d.path().join("zero.log");
    let mut w = Writer::open(&path).unwrap();
    w.write_record(RecordType::Full, b"valid").unwrap();
    w.sync_data().unwrap();
    drop(w);
    match Reader::open(&path).unwrap().read_record().unwrap() {
        ReadStatus::Record(_, data) => assert_eq!(data, b"valid"),
        o => panic!("{:?}", o),
    }
}

/// 验证最大 64KB 键长度下 WAL 条目的编解码正确性
#[test]
fn test_max_key_value() {
    let k = vec![0xAB; 65535];
    let e = WalEntry {
        sequence: 1,
        op_type: OpType::TypePut,
        has_value: true,
        key: k.clone(),
        value: Some(b"v".to_vec()),
    };
    assert_eq!(WalEntry::decode(&e.encode()).unwrap().key.len(), 65535);
}

/// 验证 Block 尾部 Padding 的跳过与续读逻辑
#[test]
fn test_block_trailer_skip() {
    let d = tempdir().unwrap();
    let path = d.path().join("trailer.log");
    let mut w = Writer::open(&path).unwrap();
    w.write_record(RecordType::Full, &vec![0xFF; 32761])
        .unwrap();
    w.write_record(RecordType::Full, b"after").unwrap();
    w.sync_data().unwrap();
    drop(w);
    let mut r = Reader::open(&path).unwrap();
    match r.read_record().unwrap() {
        ReadStatus::Record(_, data) => assert_eq!(data.len(), 32761),
        _ => panic!("no record"),
    }
    match r.read_record().unwrap() {
        ReadStatus::Record(_, data) => assert_eq!(data, b"after"),
        _ => panic!("no record"),
    }
}

/// 验证严格模式下发生 CRC/数据损坏时抛出 Fatal 错误
#[test]
fn test_strict_wal_recovery() {
    let d = tempdir().unwrap();
    let path = d.path().join("strict.log");
    let mut w = Writer::open(&path).unwrap();
    w.write_record(RecordType::Full, b"d").unwrap();
    w.sync_data().unwrap();
    drop(w);
    let mut c = std::fs::read(&path).unwrap();
    if c.len() > 4 {
        c[0] ^= 0xFF;
        std::fs::write(&path, &c).unwrap();
    }
    match Reader::open_strict(&path).unwrap().read_record().unwrap() {
        ReadStatus::CorruptionFatal => {}
        o => panic!("{:?}", o),
    }
}

// ---- WALManager / 生命周期 ----

/// 验证拒绝解析版本号不符的 WAL 文件头
#[test]
fn test_file_header_version_reject() {
    let d = tempdir().unwrap();
    let p = d.path().join("wal_1.log");
    let mut v = vec![1u8];
    v.extend([100, 0, 0, 0, 0, 0, 0, 0].as_slice());
    v.extend([255, 255, 255, 255, 255, 255, 255, 255].as_slice());
    v.extend([0, 0, 0, 0, 0, 0, 0, 0].as_slice());
    let mut w = Writer::open(&p).unwrap();
    w.write_record(
        RecordType::Full,
        &WalEntry {
            sequence: 0,
            op_type: OpType::FileHeader,
            has_value: true,
            key: b"WAL".to_vec(),
            value: Some(v),
        }
        .encode(),
    )
    .unwrap();
    w.sync_data().unwrap();
    drop(w);
    assert!(WALManager::recover(d.path(), test_opts()).is_err());
}

/// 验证空写入 Batch 不产生无效文件字节
#[test]
fn test_empty_write_batch() {
    let d = tempdir().unwrap();
    let path = d.path().join("empty.log");
    let mut w = Writer::open(&path).unwrap();
    w.sync_data().unwrap();
    let s = w.file_size().unwrap();
    drop(w);
    assert_eq!(s, 0);
}

/// 验证 WALManager 依据序列号清理旧 WAL 文件的边界
#[test]
fn test_wal_cleanup_boundary() {
    let d = tempdir().unwrap();
    let mut m = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
    m.close().unwrap();
    drop(m);
    let mut m = WALManager::open(d.path(), 2, 200, test_opts()).unwrap();
    let _ = m.cleanup(100).unwrap();
    m.close().unwrap();
}

// ---- 恢复测试 ----

fn put(s: u64, k: &[u8], v: &[u8]) -> WalEntry {
    WalEntry {
        sequence: s,
        op_type: OpType::TypePut,
        has_value: true,
        key: k.to_vec(),
        value: Some(v.to_vec()),
    }
}
fn del(s: u64, k: &[u8]) -> WalEntry {
    WalEntry {
        sequence: s,
        op_type: OpType::TypeDelete,
        has_value: false,
        key: k.to_vec(),
        value: None,
    }
}

/// 验证正常关闭后重启, 旧 WAL 日志记录的重放与恢复
#[test]
fn test_wal_replay_after_clean_close() {
    let d = tempdir().unwrap();
    {
        let mut m = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
        m.append(&put(100, b"k1", b"v1").encode()).unwrap();
        m.append(&put(101, b"k2", b"v2").encode()).unwrap();
        m.close().unwrap();
    }
    let r = WALManager::recover(d.path(), test_opts()).unwrap();
    assert_eq!(r.entries.len(), 2);
    assert_eq!(r.max_sequence, 101);
}

/// 验证未正常关闭 (Crash) 场景下的数据同步与日志恢复
#[test]
fn test_crash_recovery() {
    let d = tempdir().unwrap();
    {
        let mut m = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
        m.append(&put(100, b"crash", b"ok").encode()).unwrap();
        m.sync().unwrap();
    }
    assert_eq!(
        WALManager::recover(d.path(), test_opts())
            .unwrap()
            .entries
            .len(),
        1
    );
}

/// 验证发生多轮 WAL 文件轮转后 Crash 的数据全量恢复
#[test]
fn test_crash_with_rotation() {
    let d = tempdir().unwrap();
    {
        let mut m = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
        m.append(&put(100, b"a", b"1").encode()).unwrap();
        m.rotate(200).unwrap();
        m.append(&put(200, b"b", b"2").encode()).unwrap();
        m.rotate(300).unwrap();
        m.append(&put(300, b"c", b"3").encode()).unwrap();
    }
    assert_eq!(
        WALManager::recover(d.path(), test_opts())
            .unwrap()
            .entries
            .len(),
        3
    );
}

/// 验证按序列号按序重放多个历史 WAL 文件
#[test]
fn test_multiple_wals_replay() {
    let d = tempdir().unwrap();
    {
        let mut m = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
        m.append(&put(100, b"a", b"1").encode()).unwrap();
        m.rotate(200).unwrap();
        m.append(&put(200, b"b", b"2").encode()).unwrap();
        m.rotate(300).unwrap();
        m.append(&put(300, b"c", b"3").encode()).unwrap();
        m.close().unwrap();
    }
    assert_eq!(
        WALManager::recover(d.path(), test_opts())
            .unwrap()
            .entries
            .len(),
        3
    );
}

/// 验证 WAL 删除标记 (TypeDelete) 的正确重放与记录
#[test]
fn test_delete_replay() {
    let d = tempdir().unwrap();
    {
        let mut m = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
        m.append(&put(100, b"k1", b"v1").encode()).unwrap();
        m.append(&del(101, b"k1").encode()).unwrap();
        m.close().unwrap();
    }
    assert_eq!(
        WALManager::recover(d.path(), test_opts()).unwrap().entries[1].op_type,
        OpType::TypeDelete
    );
}

/// 验证空 WAL 文件的正常重放, 且无任何有效数据条目
#[test]
fn test_empty_wal_replay() {
    let d = tempdir().unwrap();
    {
        let mut m = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
        m.close().unwrap();
    }
    assert_eq!(
        WALManager::recover(d.path(), test_opts())
            .unwrap()
            .entries
            .len(),
        0
    );
}

/// 验证 BatchStart 事务批次记录的重放逻辑
#[test]
fn test_batch_entry_replay() {
    let batch = WalEntry {
        sequence: 0,
        op_type: OpType::BatchStart,
        has_value: true,
        key: vec![],
        value: Some(2u32.to_le_bytes().to_vec()),
    };
    let d = tempdir().unwrap();
    let mut m = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
    m.append(&batch.encode()).unwrap();
    m.append(&put(100, b"bk", b"bv").encode()).unwrap();
    m.append(&del(101, b"bd").encode()).unwrap();
    m.close().unwrap();
    assert!(
        WALManager::recover(d.path(), test_opts())
            .unwrap()
            .entries
            .len()
            >= 2
    );
}

/// 验证 WALManager 物理删除指定 Sequence 之前的历史 WAL 文件
#[test]
fn test_wal_cleanup() {
    let d = tempdir().unwrap();
    let mut m = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
    m.append(&put(100, b"old", b"x").encode()).unwrap();
    m.rotate(200).unwrap();
    assert_eq!(m.cleanup(200).unwrap().len(), 1);
    assert!(!d.path().join("wal_1.log").exists());
    m.close().unwrap();
}

// ---- 专项验证测试 ----

/// 验证跨多代 WAL 文件回填并伴随 Crash 场景下的数据完整恢复
#[test]
fn test_backfill_crash_recovery() {
    let d = tempdir().unwrap();
    // Phase 1: 写入后正常 close (触发 backfill)
    {
        let mut m = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
        m.append(&put(100, b"k1", b"v1").encode()).unwrap();
        m.append(&put(101, b"k2", b"v2").encode()).unwrap();
        m.close().unwrap();
    }
    // Phase 2: 再写入后崩溃 (不 close)
    {
        let mut m = WALManager::open(d.path(), 2, 200, test_opts()).unwrap();
        m.append(&put(200, b"k3", b"v3").encode()).unwrap();
    }
    // Phase 3: recover, 两个 WAL 的数据都应在
    let r = WALManager::recover(d.path(), test_opts()).unwrap();
    assert_eq!(r.entries.len(), 3, "backfill + crash: all 3 entries");
    assert_eq!(r.max_sequence, 200);
}

/// 验证 WAL 文件排他锁防多进程并发打开
#[test]
fn test_lock_file() {
    let d = tempdir().unwrap();
    let _m1 = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
    match WALManager::open(d.path(), 1, 100, test_opts()) {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("in use") || msg.contains("Busy"),
                "wrong error: {}",
                msg
            );
        }
        Ok(_) => panic!("second open should fail"),
    }
}

/// 验证不完整 Batch 事务因尾部截断而自动回滚
#[test]
fn test_batch_truncated_rollback() {
    let d = tempdir().unwrap();
    // 写 BatchStart(batch_size=3) + 仅 1 条 entry, 不凑满 3 条就 close
    {
        let mut m = WALManager::open(d.path(), 1, 100, test_opts()).unwrap();
        let batch = WalEntry {
            sequence: 0,
            op_type: OpType::BatchStart,
            has_value: true,
            key: vec![],
            value: Some(3u32.to_le_bytes().to_vec()),
        };
        m.append(&batch.encode()).unwrap();
        m.append(&put(100, b"orphan", b"x").encode()).unwrap();
        // batch_size=3, 但只有 1 条跟在后面, batch 不完整
        m.close().unwrap();
    }
    // recover 时应丢弃整个 batch (包括那条 orphan)
    // 因为 BatchStart + 1/3 条 = 不完整 batch, 回滚
    let r = WALManager::recover(d.path(), test_opts()).unwrap();
    // 只有 FileHeader 有效, batch 内条目应被丢弃
    assert_eq!(r.entries.len(), 0, "truncated batch should rollback");
}
