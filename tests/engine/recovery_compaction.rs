//! DB 恢复 + compaction 一致性测试.
//! @component aidb-engine
//!
//! 验证: 写入 → flush → compaction → close → reopen → 数据完整.

use std::sync::Arc;

use aidb::config::Options;
use aidb::DB;
use tempfile::TempDir;

fn recovery_opts() -> Options {
    let mut o = Options::for_testing();
    o.memtable_size = 1024;
    o.level0_compaction_trigger = 2;
    o.sync_wal = true;
    o
}

/// 基本恢复: 写入 → close → reopen → 验证.
#[test]
fn test_recovery_basic() {
    let dir = TempDir::new().unwrap();
    let keys_count = 50;

    // 写入数据
    {
        let db = Arc::new(DB::open(dir.path(), recovery_opts()).unwrap());
        for i in 0..keys_count {
            db.put(&[i], &[i]).unwrap();
        }
        db.close().unwrap();
    }

    // 恢复并验证
    {
        let db = DB::open(dir.path(), recovery_opts()).unwrap();
        for i in 0..keys_count {
            let v = db.get(&[i]).unwrap();
            assert_eq!(v, Some(vec![i]), "key {} should survive recovery", i);
        }
        db.close().unwrap();
    }
}

/// 写入 → flush → 关闭 → reopen → 验证.
#[test]
fn test_recovery_after_flush() {
    let dir = TempDir::new().unwrap();

    {
        let db = Arc::new(DB::open(dir.path(), recovery_opts()).unwrap());
        for i in 0..30u8 {
            db.put(&[i], &[i]).unwrap();
        }
        db.flush().unwrap();
        // 再写一批 (未 flush)
        for i in 30u8..50u8 {
            db.put(&[i], &[i]).unwrap();
        }
        db.close().unwrap();
    }

    {
        let db = DB::open(dir.path(), recovery_opts()).unwrap();
        for i in 0..50u8 {
            let v = db.get(&[i]).unwrap();
            assert_eq!(v, Some(vec![i]), "key {}", i);
        }
        db.close().unwrap();
    }
}

/// 写入 → flush → compaction → close → reopen → 验证.
#[test]
fn test_recovery_after_compaction() {
    let dir = TempDir::new().unwrap();

    {
        let db = Arc::new(DB::open(dir.path(), recovery_opts()).unwrap());
        // 第一层: 写入大量 key
        for i in 0..20u8 {
            let val = make_value(i as usize);
            db.put(&[i], &val).unwrap();
        }
        db.flush().unwrap();

        // 覆盖写入 → 产生旧版本可被 compaction 清理
        for i in 0..10u8 {
            db.put(&[i], &[i + 100]).unwrap();
        }
        db.flush().unwrap();

        // 写入更多 key 以触发 L0 → L1 compaction
        for i in 20u8..30u8 {
            db.put(&[i], &[i]).unwrap();
            db.flush().unwrap();
        }
        db.drain_compactions().unwrap();

        db.close().unwrap();
    }

    {
        let db = DB::open(dir.path(), recovery_opts()).unwrap();
        for i in 0..10u8 {
            let v = db.get(&[i]).unwrap();
            assert_eq!(
                v,
                Some(vec![i + 100]),
                "key {} should have overwritten value",
                i
            );
        }
        for i in 10u8..20u8 {
            let v = db.get(&[i]).unwrap();
            assert_eq!(v, Some(make_value(i as usize)), "key {}", i);
        }
        for i in 20u8..30u8 {
            let v = db.get(&[i]).unwrap();
            assert_eq!(v, Some(vec![i]), "key {}", i);
        }
        db.close().unwrap();
    }
}

/// 多次 compaction 后恢复: delete + overwrite + compaction → reopen.
#[test]
fn test_recovery_after_multiple_compactions() {
    let dir = TempDir::new().unwrap();

    {
        let db = Arc::new(DB::open(dir.path(), recovery_opts()).unwrap());
        // Round 1: write + flush + compact
        for i in 0..15u8 {
            db.put(&[i], &[i]).unwrap();
        }
        db.flush().unwrap();
        db.drain_compactions().unwrap();

        // Round 2: overwrite some + delete some + flush
        for i in 0..5u8 {
            db.put(&[i], &[i + 10]).unwrap();
        }
        for i in 10..15u8 {
            db.delete(&[i]).unwrap();
        }
        db.flush().unwrap();

        // Round 3: write more to trigger another compaction
        for i in 15..30u8 {
            db.put(&[i], &[i]).unwrap();
            db.flush().unwrap();
        }
        db.drain_compactions().unwrap();

        db.close().unwrap();
    }

    {
        let db = DB::open(dir.path(), recovery_opts()).unwrap();
        for i in 0..5u8 {
            assert_eq!(
                db.get(&[i]).unwrap(),
                Some(vec![i + 10]),
                "overwritten key {}",
                i
            );
        }
        for i in 5..10u8 {
            assert_eq!(db.get(&[i]).unwrap(), Some(vec![i]), "unchanged key {}", i);
        }
        for i in 10..15u8 {
            assert_eq!(db.get(&[i]).unwrap(), None, "deleted key {}", i);
        }
        for i in 15..30u8 {
            assert_eq!(db.get(&[i]).unwrap(), Some(vec![i]), "new key {}", i);
        }
        db.close().unwrap();
    }
}

fn make_value(i: usize) -> Vec<u8> {
    let mut v = vec![i as u8; 100];
    v[0] = i as u8;
    v
}
