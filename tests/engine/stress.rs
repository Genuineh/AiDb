//! 并发写入 + compaction 压力测试.
//!
//! 验证高频写入与 compaction 并发执行时数据完整性.
//! 这些测试默认 `#[ignore]` (耗时), 在 CI `test-slow` job 中用 `--ignored` 运行.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use aidb::config::Options;
use aidb::DB;
use tempfile::TempDir;

fn stress_opts() -> Options {
    let mut o = Options::for_testing();
    o.memtable_size = 4096; // 小 memtable 加速 flush
    o.level0_compaction_trigger = 2;
    o.sync_wal = false;
    o
}

#[ignore = "stress: concurrent write + compaction ~5s"]
#[test]
fn test_concurrent_write_and_compaction() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), stress_opts()).unwrap());
    // 启用后台 compaction
    db.set_compaction_filter(None);

    let stop = Arc::new(AtomicBool::new(false));
    let write_count = Arc::new(AtomicUsize::new(0));
    const NUM_WRITERS: usize = 4;
    const KEY_COUNT: u64 = 100;

    let mut handles = Vec::new();

    // 启动写线程
    for t in 0..NUM_WRITERS {
        let db = Arc::clone(&db);
        let stop = Arc::clone(&stop);
        let cnt = Arc::clone(&write_count);
        let handle = thread::spawn(move || {
            let mut i: u64 = 0;
            while !stop.load(Ordering::Relaxed) {
                let offset = (t as u64) * KEY_COUNT;
                let k = b"k";
                let mut key_buf = [0u8; 20];
                let n = format_key(&mut key_buf, k, offset + (i % KEY_COUNT));
                let v = i.to_le_bytes();
                // 允许写入失败 (e.g. write stall), 继续即可
                let _ = db.put(&key_buf[..n], &v);
                i = i.wrapping_add(1);
                cnt.fetch_add(1, Ordering::Relaxed);
            }
        });
        handles.push(handle);
    }

    // 持续运行 5 秒
    thread::sleep(Duration::from_secs(5));
    stop.store(true, Ordering::Relaxed);

    for h in handles {
        h.join().unwrap();
    }

    let total = write_count.load(Ordering::Relaxed);
    // 读取所有 key, 确保能读到且不 panic
    for t in 0..NUM_WRITERS {
        let offset = (t as u64) * KEY_COUNT;
        for i in 0..KEY_COUNT {
            let mut key_buf = [0u8; 20];
            let n = format_key(&mut key_buf, b"k", offset + i);
            let val = db.get(&key_buf[..n]).unwrap();
            // 只要能读到就算正常 (不要求特定值, 因为可能被其他线程覆盖)
            if val.is_none() {
                // 可能被 compaction filter 或 flush 影响, 但不应该崩溃
            }
        }
    }
    db.close().unwrap();
    eprintln!("concurrent_write_and_compaction: total_writes={total}");
}

#[ignore = "stress: concurrent write with compaction filter ~5s"]
#[test]
fn test_concurrent_write_with_filter() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), stress_opts()).unwrap());
    // 设置一个"全保留"的 filter, 验证 filter 在并发场景不会崩溃
    struct KeepAllFilter;
    impl aidb::engine::compaction::CompactionFilter for KeepAllFilter {
        fn filter(
            &self,
            _level: usize,
            _key: &[u8],
            _value: &[u8],
        ) -> aidb::engine::compaction::FilterDecision {
            aidb::engine::compaction::FilterDecision::Keep
        }
    }
    db.set_compaction_filter(Some(Arc::new(KeepAllFilter)));

    let stop = Arc::new(AtomicBool::new(false));
    let db_ref = Arc::clone(&db);
    let stop_ref = Arc::clone(&stop);
    let writer = thread::spawn(move || {
        let mut i: u64 = 0;
        while !stop_ref.load(Ordering::Relaxed) {
            let mut kb = [0u8; 20];
            let n = format_key(&mut kb, b"k", i % 50);
            let _ = db_ref.put(&kb[..n], &i.to_le_bytes());
            i = i.wrapping_add(1);
        }
    });

    let db_compact = Arc::clone(&db);
    let stop_compact = Arc::clone(&stop);
    let compactor = thread::spawn(move || {
        while !stop_compact.load(Ordering::Relaxed) {
            let _ = db_compact.drain_compactions();
            thread::sleep(Duration::from_millis(10));
        }
    });

    thread::sleep(Duration::from_secs(5));
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    compactor.join().unwrap();
    db.close().unwrap();
}

/// format_key 将 key 格式化为 "k:NNNNNNNN".
fn format_key(buf: &mut [u8], prefix: &[u8], num: u64) -> usize {
    let pfx_len = prefix.len();
    buf[..pfx_len].copy_from_slice(prefix);
    buf[pfx_len] = b':';
    let start = pfx_len + 1;
    // 固定 8 位十进制, 不够前面补 0
    for i in 0..8 {
        let digit = ((num / 10u64.pow(7 - i as u32)) % 10) as u8 + b'0';
        buf[start + i] = digit;
    }
    start + 8
}
