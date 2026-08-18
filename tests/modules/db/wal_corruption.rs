//! WAL 损坏与截断恢复集成测试 (DB 级)
//! @component aidb-engine
//!
//! DB::close() 会自动 flush + cleanup WAL, 所以需要直接 drop DB
//! (不调用 close) 来模拟崩溃, 让 WAL 文件保留在磁盘上.

use aidb::config::Options;
use aidb::DB;
use std::fs;
use tempfile::tempdir;

fn small_opts() -> Options {
    let mut o = Options::for_testing();
    o.memtable_size = 64 * 1024; // 足够大, 不自动 flush
    o.sync_wal = true;
    o
}

/// 扫描目录中文件号最大的 WAL 文件
fn find_latest_wal(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("wal_") && n.ends_with(".log"))
        })
        .max_by_key(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("wal_"))
                .and_then(|n| n.strip_suffix(".log"))
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0)
        })
        .map(|e| e.path())
}

// ── CRC 损坏测试 ─────────────────────────────────────────────────────────────

/// strict_wal_recovery=true + FileHeader CRC 损坏 → DB::open 返回错误
#[test]
fn test_wal_crc_corruption_strict_fails() {
    let dir = tempdir().unwrap();

    // Phase 1: 写入 batch1 + flush → SST
    {
        let mut opts = small_opts();
        opts.strict_wal_recovery = false;
        let db = DB::open(dir.path(), opts).unwrap();
        for i in 0u8..5 {
            db.put(&[b'a' + i], b"flushed").unwrap();
        }
        db.flush().unwrap();
        // Phase 2: 写入 batch2, 直接 drop (不 close) → WAL 保留
        for i in 0u8..5 {
            db.put(&[b'A' + i], b"wal_only").unwrap();
        }
        // 不调用 close(), 直接 drop → WAL 文件留存
    }

    let wal = find_latest_wal(dir.path()).expect("WAL file must exist after crash-drop");

    // 损坏第一字节 (FileHeader Record 的 CRC 第一字节)
    let mut bytes = fs::read(&wal).unwrap();
    assert!(!bytes.is_empty(), "WAL must not be empty");
    bytes[0] ^= 0xFF;
    fs::write(&wal, &bytes).unwrap();

    // strict=true 时打开应失败
    let mut strict_opts = small_opts();
    strict_opts.strict_wal_recovery = true;
    let result = DB::open(dir.path(), strict_opts);
    assert!(
        result.is_err(),
        "strict_wal_recovery=true should return error on corrupted WAL FileHeader"
    );
}

/// strict_wal_recovery=false + FileHeader CRC 损坏 → DB::open 成功, SST 数据可读
#[test]
fn test_wal_crc_corruption_lenient_recovers_sst_data() {
    let dir = tempdir().unwrap();

    {
        let mut opts = small_opts();
        opts.strict_wal_recovery = false;
        let db = DB::open(dir.path(), opts).unwrap();
        for i in 0u8..5 {
            db.put(&[b'a' + i], b"flushed").unwrap();
        }
        db.flush().unwrap();
        for i in 0u8..5 {
            db.put(&[b'A' + i], b"wal_only").unwrap();
        }
        // 不 close → WAL 保留
    }

    let wal = find_latest_wal(dir.path()).expect("WAL file must exist after crash-drop");

    let mut bytes = fs::read(&wal).unwrap();
    assert!(!bytes.is_empty());
    bytes[0] ^= 0xFF;
    fs::write(&wal, &bytes).unwrap();

    // lenient=false 时应能打开
    let mut lenient_opts = small_opts();
    lenient_opts.strict_wal_recovery = false;
    let db = DB::open(dir.path(), lenient_opts).unwrap();

    // flush 前的 batch1 在 SST 中, 必须可读
    for i in 0u8..5 {
        assert_eq!(
            db.get(&[b'a' + i]).unwrap(),
            Some(b"flushed".to_vec()),
            "pre-flush key 'a'+{i} must survive WAL corruption"
        );
    }
    db.close().unwrap();
}

// ── 截断 batch 测试 ──────────────────────────────────────────────────────────

/// 截断 WAL 末尾若干字节 → 不 panic, DB::open 成功, SST 数据可读
#[test]
fn test_wal_truncated_tail_recovers() {
    let dir = tempdir().unwrap();

    {
        let db = DB::open(dir.path(), small_opts()).unwrap();
        // batch1: flush 到 SST
        for i in 0u32..5 {
            let key = format!("pre_{i}");
            db.put(key.as_bytes(), b"pre_value").unwrap();
        }
        db.flush().unwrap();
        // batch2: 留在 WAL
        for i in 0u32..5 {
            let key = format!("post_{i}");
            db.put(key.as_bytes(), b"post_value").unwrap();
        }
        // 不 close → WAL 文件保留
    }

    // 找到 WAL 文件
    let wal = find_latest_wal(dir.path()).expect("WAL file must exist after crash-drop");

    let bytes = fs::read(&wal).unwrap();
    if bytes.len() > 64 {
        // 截断末尾 32 字节 (模拟不完整写入)
        let new_len = bytes.len() - 32;
        fs::write(&wal, &bytes[..new_len]).unwrap();
    }

    // 重新打开: 不 panic, 返回 Ok
    let db = DB::open(dir.path(), small_opts()).unwrap();

    // SST 中的 batch1 必须可读
    for i in 0u32..5 {
        let key = format!("pre_{i}");
        assert_eq!(
            db.get(key.as_bytes()).unwrap(),
            Some(b"pre_value".to_vec()),
            "pre-flush key {key} must be readable after WAL truncation"
        );
    }

    db.close().unwrap();
}
