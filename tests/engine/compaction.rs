//! Compaction 集成: 经 `DB` API (CI 默认 ~800 keys).
//!
//! ```bash
//! cargo test --test engine compaction -- --test-threads=1
//! cargo test --test engine compaction -- --ignored --test-threads=1  # 10000 压测
//! ```

use aidb::config::Options;
use aidb::DB;
use std::sync::Arc;
use tempfile::tempdir;

fn compaction_opts() -> Options {
    let mut o = Options::for_testing();
    o.memtable_size = 256;
    o.level0_compaction_trigger = 2;
    o.sync_wal = true;
    o
}

/// 验证 write stall 在 L0 堆积时不会死锁, compaction 可恢复.
#[test]
fn test_write_stall_on_l0_pileup() {
    let dir = tempdir().unwrap();
    let mut opts = compaction_opts();
    opts.background_compaction = true;
    opts.level0_compaction_trigger = 2;
    opts.level0_slowdown_writes_trigger = 3;
    opts.level0_stop_writes_trigger = 6;
    let db = Arc::new(DB::open(dir.path(), opts).unwrap());
    for batch in 0..15u64 {
        for i in 0..10u64 {
            db.put(format!("k{batch}_{i}").as_bytes(), b"val").unwrap();
        }
        db.flush().unwrap();
    }
    // stall 不应引起死锁
    db.drain_compactions().unwrap();
    assert!(
        db.level0_sstable_count() < 5,
        "L0 should be compacted: {}",
        db.level0_sstable_count()
    );
    db.close().unwrap();
}

fn write_keys(db: &DB, n: usize) {
    for i in 0..n {
        let key = format!("key{:06}", i);
        let val = format!("val{:06}", i);
        db.put(key.as_bytes(), val.as_bytes()).unwrap();
    }
}

fn verify_keys(db: &DB, n: usize) {
    for i in 0..n {
        let key = format!("key{:06}", i);
        let val = format!("val{:06}", i);
        assert_eq!(db.get(key.as_bytes()).unwrap(), Some(val.into_bytes()));
    }
}

#[test]
fn test_large_dataset_compaction_ci() {
    let dir = tempdir().unwrap();
    {
        let db = Arc::new(DB::open(dir.path(), compaction_opts()).unwrap());
        write_keys(&db, 800);
        for _ in 0..6 {
            db.flush().unwrap();
        }
        db.drain_compactions().unwrap();
        verify_keys(&db, 800);
        db.close().unwrap();
    }

    let db2 = DB::open(dir.path(), compaction_opts()).unwrap();
    verify_keys(&db2, 800);
    db2.close().unwrap();
}

#[test]
fn test_compaction_removes_deleted_entries() {
    let dir = tempdir().unwrap();
    let db = Arc::new(DB::open(dir.path(), compaction_opts()).unwrap());
    db.put(b"gone", b"x").unwrap();
    db.flush().unwrap();
    db.put(b"stay", b"y").unwrap();
    db.flush().unwrap();
    db.delete(b"gone").unwrap();
    db.flush().unwrap();
    db.drain_compactions().unwrap();
    assert_eq!(db.get(b"gone").unwrap(), None);
    assert_eq!(db.get(b"stay").unwrap(), Some(b"y".to_vec()));
    db.close().unwrap();
}

#[test]
fn test_compaction_across_restarts() {
    let dir = tempdir().unwrap();
    {
        let db = Arc::new(DB::open(dir.path(), compaction_opts()).unwrap());
        write_keys(&db, 500);
        for _ in 0..5 {
            db.flush().unwrap();
        }
        db.drain_compactions().unwrap();
        db.close().unwrap();
    }
    let db2 = DB::open(dir.path(), compaction_opts()).unwrap();
    verify_keys(&db2, 500);
    assert!(dir.path().join("CURRENT").exists());
    db2.close().unwrap();
}

#[test]
#[ignore = "stress: run with cargo test --test engine compaction -- --ignored"]
fn test_large_dataset_compaction_stress_10000() {
    let dir = tempdir().unwrap();
    let db = Arc::new(DB::open(dir.path(), compaction_opts()).unwrap());
    write_keys(&db, 10_000);
    for _ in 0..12 {
        db.flush().unwrap();
    }
    db.drain_compactions().unwrap();
    verify_keys(&db, 10_000);
    db.close().unwrap();
}

#[test]
fn test_trivial_move_promotes_file_without_rewrite() {
    let dir = tempdir().unwrap();
    let mut opts = Options::for_testing();
    // Use manual drain_compactions (not background thread) to avoid race conditions.
    // Small level base forces each L1 file to exceed the per-level target, which
    // triggers pick_level_n and, when L2 is empty, a trivial-move promotion.
    opts.max_bytes_for_level_base = 10;
    opts.max_bytes_for_level_multiplier = 10;
    opts.level0_compaction_trigger = 2;
    let db = Arc::new(DB::open(dir.path(), opts).unwrap());

    // Write one range of keys so L1 files don't overlap with L2 seed
    for i in 0..5 {
        let key = format!("k{i:04}");
        db.put(key.as_bytes(), b"v").unwrap();
        db.flush().unwrap();
    }
    db.drain_compactions().unwrap();

    // Snapshot non-L0 file numbers after first compaction round.
    // With a tiny max_bytes_for_level_base, files cascade down multiple levels
    // via trivial moves within a single drain_compactions call.
    let first_file_nums: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let fname = e.file_name();
            let name = fname.to_string_lossy();
            name.ends_with(".sst") && !name.contains("_L0.sst")
        })
        .map(|e| {
            let name = e.file_name();
            let n = name.to_string_lossy();
            // Extract just the file number portion (e.g. "000007" from "000007_L3.sst")
            n.split("_L").next().unwrap_or("").to_string()
        })
        .collect();
    assert!(
        !first_file_nums.is_empty(),
        "should have non-L0 files after first compaction"
    );
    eprintln!("first file numbers: {:?}", first_file_nums);

    // Now write more data to push into L1 again (different key range, no overlap with first)
    for i in 100..105 {
        let key = format!("k{i:04}");
        db.put(key.as_bytes(), b"v").unwrap();
        db.flush().unwrap();
    }
    db.drain_compactions().unwrap();

    // Snapshot all non-L0 file numbers after second compaction
    let second_file_nums: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let fname = e.file_name();
            let name = fname.to_string_lossy();
            name.ends_with(".sst") && !name.contains("_L0.sst")
        })
        .map(|e| {
            let name = e.file_name();
            let n = name.to_string_lossy();
            n.split("_L").next().unwrap_or("").to_string()
        })
        .collect();
    eprintln!("second file numbers: {:?}", second_file_nums);

    // Verify file_numbers are preserved: files were trivially moved (renamed), not rewritten.
    // If a file was rewritten by a full compaction, it would get a new file_number.
    for num in &first_file_nums {
        assert!(
            second_file_nums.contains(num),
            "file number {num} from first compaction was not found after second drain"
        );
    }

    // Verify data is still correct after trivial moves
    for i in 0..5 {
        let key = format!("k{i:04}");
        assert_eq!(
            db.get(key.as_bytes()).unwrap(),
            Some(b"v".to_vec()),
            "missing key {key}"
        );
    }
    for i in 100..105 {
        let key = format!("k{i:04}");
        assert_eq!(
            db.get(key.as_bytes()).unwrap(),
            Some(b"v".to_vec()),
            "missing key {key}"
        );
    }
    db.close().unwrap();
}

#[test]
fn test_compaction_threads_2() {
    let dir = tempdir().unwrap();
    let mut opts = Options::for_testing();
    opts.background_compaction = true;
    opts.compaction_threads = 2;
    opts.level0_compaction_trigger = 2;
    let db = Arc::new(DB::open(dir.path(), opts).unwrap());

    // Write enough data to trigger multiple compactions
    for batch in 0..10 {
        for i in 0..10u64 {
            db.put(format!("k{batch}_{i}").as_bytes(), b"val").unwrap();
        }
        db.flush().unwrap();
    }

    // Drain all compactions
    db.drain_compactions().unwrap();

    // Verify data
    for batch in 0..10 {
        for i in 0..10u64 {
            let key = format!("k{batch}_{i}");
            assert_eq!(db.get(key.as_bytes()).unwrap(), Some(b"val".to_vec()));
        }
    }
    db.close().unwrap();
}

#[test]
fn test_concurrent_writes_during_compaction() {
    let dir = tempdir().unwrap();
    let mut opts = compaction_opts();
    opts.background_compaction = true;
    let db = Arc::new(DB::open(dir.path(), opts).unwrap());
    write_keys(&db, 400);
    for _ in 0..4 {
        db.flush().unwrap();
    }
    let db2 = Arc::clone(&db);
    let writer = std::thread::spawn(move || {
        for i in 400..500 {
            let key = format!("key{:06}", i);
            let val = format!("val{:06}", i);
            db2.put(key.as_bytes(), val.as_bytes()).unwrap();
        }
    });
    let db3 = Arc::clone(&db);
    let compactor = std::thread::spawn(move || {
        for _ in 0..15 {
            let _ = db3.drain_compactions();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    writer.join().unwrap();
    compactor.join().unwrap();
    db.drain_compactions().unwrap();
    verify_keys(&db, 500);
    db.close().unwrap();
}

#[test]
fn test_compaction_expanded_inputs() {
    let dir = tempdir().unwrap();
    let mut opts = compaction_opts();
    opts.max_bytes_for_level_base = 1500;
    opts.max_bytes_for_level_multiplier = 10;
    let db = Arc::new(DB::open(dir.path(), opts).unwrap());
    for i in 0usize..300 {
        let key = format!("key{:06}", i);
        let val = vec![b'x'; 128];
        db.put(key.as_bytes(), &val).unwrap();
        if i.is_multiple_of(25) {
            db.flush().unwrap();
        }
    }
    db.drain_compactions().unwrap();
    for i in 0..300 {
        let key = format!("key{:06}", i);
        assert!(
            db.get(key.as_bytes()).unwrap().is_some(),
            "missing key {key}"
        );
    }
    assert!(dir.path().join("CURRENT").exists());
    db.close().unwrap();
}

#[test]
fn test_compaction_crash_recovery() {
    let dir = tempdir().unwrap();
    {
        let db = Arc::new(DB::open(dir.path(), compaction_opts()).unwrap());
        write_keys(&db, 200);
        for _ in 0..4 {
            db.flush().unwrap();
        }
        db.drain_compactions().unwrap();
        // 模拟崩溃: 不 close, 留下 orphan 输出 SST
        std::fs::write(dir.path().join("999999_L1.sst"), b"orphan").unwrap();
    }
    let db = DB::open(dir.path(), compaction_opts()).unwrap();
    assert!(
        !dir.path().join("999999_L1.sst").exists(),
        "orphan SST should be removed on open"
    );
    assert!(dir.path().join("CURRENT").exists());
    verify_keys(&db, 200);
    db.close().unwrap();
}

#[test]
fn test_subcompaction_large_job() {
    let dir = tempdir().unwrap();
    let mut opts = Options::for_testing();
    opts.background_compaction = true;
    opts.compaction_threads = 2;
    opts.subcompaction_min_size = 1024;
    opts.level0_compaction_trigger = 2;
    let db = Arc::new(DB::open(dir.path(), opts).unwrap());

    // Write enough data to trigger multiple compressions with subcompaction
    for batch in 0..20 {
        for i in 0..20u64 {
            db.put(format!("k{batch:04}_{i:04}").as_bytes(), b"val")
                .unwrap();
        }
        db.flush().unwrap();
    }
    db.drain_compactions().unwrap();

    // Verify all data is still accessible
    for batch in 0..20 {
        for i in 0..20u64 {
            let key = format!("k{batch:04}_{i:04}");
            assert_eq!(
                db.get(key.as_bytes()).unwrap(),
                Some(b"val".to_vec()),
                "missing key {key}"
            );
        }
    }
    db.close().unwrap();
}
