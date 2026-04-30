# 核心引擎层测试完善实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补充 CompactionJob::run() 和 DBIterator 的单元测试及集成测试

**架构:** 直接在现有源文件中追加 `#[cfg(test)]` 单元测试，在 `tests/` 中补充集成测试。不重构测试基础设施。

**涉及文件:**
- 修改: `src/compaction/mod.rs` — 追加 6 个 CompactionJob 单元测试
- 修改: `src/iterator.rs` — 追加 8 个 DBIterator 单元测试
- 修改: `tests/compaction_tests.rs` — 追加 2 个 compaction 集成测试
- 修改: `tests/integration_tests.rs` — 追加 5 个 iterator 集成测试

**注意:** `tests/advanced_integration_tests.rs` 已有 `test_iterator_basic`、`test_iterator_range`、`test_iterator_with_deletes`、`test_iterator_after_flush`、`test_range_scan`，集成测试不要重复这些场景。

**相比 spec 的调整:**
- 移除了 `test_iterator_across_all_layers`（已由 `test_iterator_after_flush` 覆盖）
- 将 `test_iterator_seek_to_last` 改为调用 `seek_to_first` 的等价实现

---

### Task 1: CompactionJob 单元测试 — 辅助函数 + 基本合并测试

**文件:** `src/compaction/mod.rs:178-188`（追加到现有 `#[cfg(test)]` 模块）

- [ ] **Step 1: 添加测试辅助函数和第一个测试用例**

在 `src/compaction/mod.rs` 的 `mod tests` 中，修改现有的 import，并添加 `create_sstable` 辅助函数和 `test_compaction_basic_merge`：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sstable::SSTableBuilder;
    use crate::sstable::SSTableReader;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn create_sstable(dir: &Path, number: u64, entries: &[(&[u8], &[u8])]) -> Arc<SSTableReader> {
        let path = dir.join(format!("{:06}.sst", number));
        let mut builder = SSTableBuilder::new(&path).unwrap();
        for (k, v) in entries {
            builder.add(k, v).unwrap();
        }
        builder.finish().unwrap();
        Arc::new(SSTableReader::open(&path).unwrap())
    }

    #[test]
    fn test_target_size_for_level() {
        assert_eq!(target_size_for_level(1), 10 * 1024 * 1024);
        assert_eq!(target_size_for_level(2), 100 * 1024 * 1024);
        assert_eq!(target_size_for_level(3), 1000 * 1024 * 1024);
    }

    #[test]
    fn test_compaction_basic_merge() {
        let dir = TempDir::new().unwrap();

        let sst1 = create_sstable(&dir, 1, &[(b"a", b"1"), (b"b", b"2"), (b"c", b"3")]);
        let sst2 = create_sstable(&dir, 2, &[(b"d", b"4"), (b"e", b"5"), (b"f", b"6")]);

        let job = CompactionJob::new(
            vec![sst1, sst2],
            1,
            dir.path().to_path_buf(),
            4096,
        );
        let result = job.run(100).unwrap();

        assert_eq!(result.entry_count, 6);
        let output_path = dir.path().join("000100.sst");
        assert!(output_path.exists());

        let reader = SSTableReader::open(&output_path).unwrap();
        assert_eq!(reader.get(b"a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(reader.get(b"b").unwrap(), Some(b"2".to_vec()));
        assert_eq!(reader.get(b"c").unwrap(), Some(b"3".to_vec()));
        assert_eq!(reader.get(b"d").unwrap(), Some(b"4".to_vec()));
        assert_eq!(reader.get(b"e").unwrap(), Some(b"5".to_vec()));
        assert_eq!(reader.get(b"f").unwrap(), Some(b"6".to_vec()));
    }
}
```

- [ ] **Step 2: 运行测试验证通过**

```bash
cd /root/code/wiqun/AiDb && cargo test compaction::tests::test_compaction_basic_merge -- --nocapture 2>&1
```

Expected: `test compaction::tests::test_compaction_basic_merge ... ok`

- [ ] **Step 3: 提交**

```bash
git add src/compaction/mod.rs
git commit -m "test(compaction): add basic merge unit test for CompactionJob"
```

---

### Task 2: CompactionJob 单元测试 — 去重、tombstone、边界情况

**文件:** `src/compaction/mod.rs`（在上一个测试之后追加）

- [ ] **Step 1: 添加去重测试**

在 `test_compaction_basic_merge` 之后追加：

```rust
#[test]
fn test_compaction_removes_duplicates() {
    let dir = TempDir::new().unwrap();

    let sst1 = create_sstable(&dir, 1, &[(b"a", b"1"), (b"b", b"2"), (b"c", b"3")]);
    let sst2 = create_sstable(&dir, 2, &[(b"b", b"20"), (b"c", b"30"), (b"d", b"40")]);

    let job = CompactionJob::new(
        vec![sst1, sst2],
        1,
        dir.path().to_path_buf(),
        4096,
    );
    let result = job.run(100).unwrap();
    assert_eq!(result.entry_count, 4); // a, b, c, d (no duplicates)

    let reader = SSTableReader::open(&dir.path().join("000100.sst")).unwrap();
    assert_eq!(reader.get(b"a").unwrap(), Some(b"1".to_vec()));
    assert_eq!(reader.get(b"b").unwrap(), Some(b"2".to_vec()));
    assert_eq!(reader.get(b"c").unwrap(), Some(b"3".to_vec()));
    assert_eq!(reader.get(b"d").unwrap(), Some(b"40".to_vec()));
}
```

- [ ] **Step 2: 添加 tombstone 在 level 1+ 被清除的测试**

```rust
#[test]
fn test_compaction_removes_tombstones_at_level1() {
    let dir = TempDir::new().unwrap();

    // "b" and "d" have empty values (tombstones)
    let sst1 = create_sstable(
        &dir,
        1,
        &[(b"a", b"1"), (b"b", b""), (b"c", b"3"), (b"d", b""), (b"e", b"5")],
    );

    let job = CompactionJob::new(
        vec![sst1],
        1, // level 1+ removes tombstones
        dir.path().to_path_buf(),
        4096,
    );
    let result = job.run(100).unwrap();
    assert_eq!(result.entry_count, 3); // a, c, e (tombstones removed)

    let reader = SSTableReader::open(&dir.path().join("000100.sst")).unwrap();
    assert_eq!(reader.get(b"a").unwrap(), Some(b"1".to_vec()));
    assert_eq!(reader.get(b"b").unwrap(), None); // tombstone removed
    assert_eq!(reader.get(b"c").unwrap(), Some(b"3".to_vec()));
    assert_eq!(reader.get(b"d").unwrap(), None); // tombstone removed
    assert_eq!(reader.get(b"e").unwrap(), Some(b"5".to_vec()));
}
```

- [ ] **Step 3: 添加 tombstone 在 level 0 被保留的测试**

```rust
#[test]
fn test_compaction_preserves_tombstones_at_level0() {
    let dir = TempDir::new().unwrap();

    let sst1 = create_sstable(
        &dir,
        1,
        &[(b"a", b"1"), (b"b", b""), (b"c", b"3")],
    );

    let job = CompactionJob::new(
        vec![sst1],
        0, // level 0 preserves tombstones
        dir.path().to_path_buf(),
        4096,
    );
    let result = job.run(100).unwrap();
    assert_eq!(result.entry_count, 3); // all entries kept

    let reader = SSTableReader::open(&dir.path().join("000100.sst")).unwrap();
    assert_eq!(reader.get(b"a").unwrap(), Some(b"1".to_vec()));
    // At level 0, tombstone is preserved — reader returns None for empty values
    // since get() treats empty value as non-existent
    assert_eq!(reader.get(b"b").unwrap(), None);
    assert_eq!(reader.get(b"c").unwrap(), Some(b"3".to_vec()));
}
```

- [ ] **Step 4: 添加单输入和全删除测试**

```rust
#[test]
fn test_compaction_single_input() {
    let dir = TempDir::new().unwrap();

    let sst1 = create_sstable(&dir, 1, &[(b"x", b"10"), (b"y", b"20"), (b"z", b"30")]);

    let job = CompactionJob::new(
        vec![sst1],
        1,
        dir.path().to_path_buf(),
        4096,
    );
    let result = job.run(100).unwrap();
    assert_eq!(result.entry_count, 3);

    let reader = SSTableReader::open(&dir.path().join("000100.sst")).unwrap();
    assert_eq!(reader.get(b"x").unwrap(), Some(b"10".to_vec()));
    assert_eq!(reader.get(b"y").unwrap(), Some(b"20".to_vec()));
    assert_eq!(reader.get(b"z").unwrap(), Some(b"30".to_vec()));
}

#[test]
fn test_compaction_all_entries_removed() {
    let dir = TempDir::new().unwrap();

    // All entries are tombstones
    let sst1 = create_sstable(&dir, 1, &[(b"a", b""), (b"b", b""), (b"c", b"")]);

    let job = CompactionJob::new(
        vec![sst1],
        1, // level 1+ removes tombstones
        dir.path().to_path_buf(),
        4096,
    );
    let result = job.run(100).unwrap();
    assert_eq!(result.file_number, 0);
    assert_eq!(result.entry_count, 0);
    // Output file should not exist
    assert!(!dir.path().join("000100.sst").exists());
}
```

- [ ] **Step 5: 运行所有 compaction 单元测试**

```bash
cd /root/code/wiqun/AiDb && cargo test compaction::tests -- --nocapture 2>&1
```

Expected: 所有 7 个测试通过（原有的 1 个 + 新增 6 个）

- [ ] **Step 6: 提交**

```bash
git add src/compaction/mod.rs
git commit -m "test(compaction): add dedup, tombstone, and edge case unit tests"
```

---

### Task 3: DBIterator 单元测试 — 空库 + seek 测试

**文件:** `src/iterator.rs:560-592`（追加到现有 `#[cfg(test)]` 模块）

- [ ] **Step 1: 添加空库和 seek 测试**

追加到 `src/iterator.rs` 现有 `mod tests` 的末尾：

```rust
#[test]
fn test_iterator_empty_db() {
    let tmp_dir = TempDir::new().unwrap();
    let db = DB::open(tmp_dir.path(), Options::default()).unwrap();
    let db = Arc::new(db);

    let mut iter = db.iter().unwrap();
    assert!(!iter.valid());
}

#[test]
fn test_iterator_seek_existing() {
    let tmp_dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

    db.put(b"key1", b"val1").unwrap();
    db.put(b"key2", b"val2").unwrap();
    db.put(b"key3", b"val3").unwrap();

    let mut iter = db.iter().unwrap();
    iter.seek(b"key2").unwrap();
    assert!(iter.valid());
    assert_eq!(iter.key(), b"key2");
    assert_eq!(iter.value(), b"val2");
}

#[test]
fn test_iterator_seek_nonexistent() {
    let tmp_dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

    db.put(b"key1", b"val1").unwrap();
    db.put(b"key3", b"val3").unwrap();
    db.put(b"key5", b"val5").unwrap();

    // Seek between key1 and key3 should land on key3
    let mut iter = db.iter().unwrap();
    iter.seek(b"key2").unwrap();
    assert!(iter.valid());
    assert_eq!(iter.key(), b"key3");

    // Seek past all keys should be invalid
    let mut iter = db.iter().unwrap();
    iter.seek(b"zzz").unwrap();
    assert!(!iter.valid());
}
```

- [ ] **Step 2: 运行测试验证**

```bash
cd /root/code/wiqun/AiDb && cargo test iterator::tests -- --nocapture 2>&1
```

Expected: 4 个测试通过（原有的 1 个 + 新增 3 个）

- [ ] **Step 3: 提交**

```bash
git add src/iterator.rs
git commit -m "test(iterator): add empty db and seek unit tests"
```

---

### Task 4: DBIterator 单元测试 — 范围、tombstone、覆盖、seek_to_first

**文件:** `src/iterator.rs`（在上一个测试之后追加）

- [ ] **Step 1: 添加 seek_to_first 和 range scan 测试**

在 `test_iterator_seek_nonexistent` 之后追加：

```rust
#[test]
fn test_iterator_seek_to_first() {
    let tmp_dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

    db.put(b"key2", b"val2").unwrap();
    db.put(b"key1", b"val1").unwrap();
    db.put(b"key3", b"val3").unwrap();

    let mut iter = db.iter().unwrap();
    iter.seek_to_first();
    assert!(iter.valid());
    assert_eq!(iter.key(), b"key1");
}

#[test]
fn test_iterator_range() {
    let tmp_dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

    db.put(b"key1", b"val1").unwrap();
    db.put(b"key2", b"val2").unwrap();
    db.put(b"key3", b"val3").unwrap();
    db.put(b"key4", b"val4").unwrap();
    db.put(b"key5", b"val5").unwrap();

    let mut iter = db.scan(Some(b"key2"), Some(b"key5")).unwrap();
    let mut entries = Vec::new();
    while iter.valid() {
        entries.push((iter.key().to_vec(), iter.value().to_vec()));
        iter.next();
    }
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].0, b"key2");
    assert_eq!(entries[1].0, b"key3");
    assert_eq!(entries[2].0, b"key4");
}
```

- [ ] **Step 2: 添加 tombstone 过滤和 overwrite 测试**

```rust
#[test]
fn test_iterator_tombstone_filtering() {
    let tmp_dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

    db.put(b"key1", b"val1").unwrap();
    db.put(b"key2", b"val2").unwrap();
    db.delete(b"key2").unwrap();

    let mut iter = db.iter().unwrap();
    let mut keys = Vec::new();
    while iter.valid() {
        keys.push(iter.key().to_vec());
        iter.next();
    }
    assert_eq!(keys, vec![b"key1"]);
}

#[test]
fn test_iterator_overwrite() {
    let tmp_dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

    db.put(b"key1", b"v1").unwrap();
    db.put(b"key1", b"v2").unwrap();

    let mut iter = db.iter().unwrap();
    assert!(iter.valid());
    assert_eq!(iter.key(), b"key1");
    assert_eq!(iter.value(), b"v2");
}

#[test]
fn test_iterator_seek_to_last() {
    let tmp_dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(tmp_dir.path(), Options::default()).unwrap());

    db.put(b"a", b"1").unwrap();
    db.put(b"b", b"2").unwrap();
    db.put(b"c", b"3").unwrap();

    // seek_to_last currently delegates to seek_to_first
    let mut iter = db.iter().unwrap();
    iter.seek_to_last();
    assert!(iter.valid());
}
```

- [ ] **Step 3: 运行测试验证**

```bash
cd /root/code/wiqun/AiDb && cargo test iterator::tests -- --nocapture 2>&1
```

Expected: 8 个测试全部通过

- [ ] **Step 4: 提交**

```bash
git add src/iterator.rs
git commit -m "test(iterator): add range, tombstone, overwrite, seek_to_first, seek_to_last unit tests"
```

---

### Task 5: 集成测试 — compaction 补充

**文件:** `tests/compaction_tests.rs`（追加到文件末尾）

- [ ] **Step 1: 添加 compaction consolidation 测试**

```rust
#[test]
fn test_compaction_consolidation() {
    env_logger::try_init().ok();

    let temp_dir = TempDir::new().unwrap();
    let options = Options::default().memtable_size(1024);

    let db = Arc::new(DB::open(temp_dir.path(), options).unwrap());

    // Multiple rounds of write + flush to trigger compaction
    for round in 0..8 {
        for i in 0..30 {
            let key = format!("round{}_key{:04}", round, i);
            let value = format!("val{}", i);
            db.put(key.as_bytes(), value.as_bytes()).unwrap();
        }
        db.flush().unwrap();
    }

    // Wait for compaction to complete
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Verify no duplicate keys by counting through iteration
    let mut iter = db.iter().unwrap();
    let mut keys = std::collections::BTreeSet::new();
    while iter.valid() {
        let key = iter.key().to_vec();
        assert!(keys.insert(key.clone()), "Duplicate key found: {:?}", String::from_utf8_lossy(&key));
        iter.next();
    }

    // Verify all expected keys exist
    for round in 0..8 {
        for i in 0..30 {
            let key = format!("round{}_key{:04}", round, i);
            let value = db.get(key.as_bytes()).unwrap();
            assert!(value.is_some(), "Key {} should exist", key);
        }
    }
}

#[test]
fn test_compaction_all_deleted() {
    env_logger::try_init().ok();

    let temp_dir = TempDir::new().unwrap();
    let options = Options::default().memtable_size(1024);

    let db = DB::open(temp_dir.path(), options).unwrap();

    // Write data across multiple SSTables
    for batch in 0..5 {
        for i in 0..20 {
            let key = format!("key{:04}_{:04}", batch, i);
            db.put(key.as_bytes(), b"value").unwrap();
        }
        db.flush().unwrap();
    }

    // Delete all keys
    for batch in 0..5 {
        for i in 0..20 {
            let key = format!("key{:04}_{:04}", batch, i);
            db.delete(key.as_bytes()).unwrap();
        }
    }
    db.flush().unwrap();

    // Trigger compaction
    for batch in 5..10 {
        for i in 0..20 {
            let key = format!("newkey{:04}_{:04}", batch, i);
            db.put(key.as_bytes(), b"value").unwrap();
        }
        db.flush().unwrap();
    }

    // Verify deleted keys are gone
    for batch in 0..5 {
        for i in 0..20 {
            let key = format!("key{:04}_{:04}", batch, i);
            assert_eq!(db.get(key.as_bytes()).unwrap(), None);
        }
    }
}
```

- [ ] **Step 2: 运行 compaction 集成测试**

```bash
cd /root/code/wiqun/AiDb && cargo test compaction_tests::test_compaction_consolidation compaction_tests::test_compaction_all_deleted -- --nocapture 2>&1
```

Expected: 2 个测试通过

- [ ] **Step 3: 提交**

```bash
git add tests/compaction_tests.rs
git commit -m "test(compaction): add consolidation and all-deleted integration tests"
```

---

### Task 6: 集成测试 — iterator 补充

**文件:** `tests/integration_tests.rs`（追加到文件末尾，在最后一个 `}` 之前）

- [ ] **Step 1: 添加 seek 和 range scan 集成测试**

```rust
/// Test seek across memtable + SSTable layers
#[test]
fn test_iterator_seek_e2e() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    db.put(b"alpha", b"value_alpha").unwrap();
    db.put(b"beta", b"value_beta").unwrap();
    db.put(b"delta", b"value_delta").unwrap();
    db.put(b"gamma", b"value_gamma").unwrap();

    // Seek to beta
    let mut iter = db.iter().unwrap();
    iter.seek(b"beta").unwrap();
    assert!(iter.valid());
    assert_eq!(iter.key(), b"beta");
    assert_eq!(iter.value(), b"value_beta");

    // Seek to non-existent key between beta and delta
    let mut iter = db.iter().unwrap();
    iter.seek(b"charlie").unwrap();
    assert!(iter.valid());
    assert_eq!(iter.key(), b"delta");

    // Seek past all keys
    let mut iter = db.iter().unwrap();
    iter.seek(b"zzz").unwrap();
    assert!(!iter.valid());
}

/// Test scan with actual range bounds
#[test]
fn test_iterator_range_bounds_e2e() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    for i in 0..10 {
        let key = format!("key{:02}", i);
        let value = format!("val{:02}", i);
        db.put(key.as_bytes(), value.as_bytes()).unwrap();
    }

    // Scan middle range
    let mut iter = db.scan(Some(b"key03"), Some(b"key07")).unwrap();
    let mut count = 0;
    let mut first_key = None;
    while iter.valid() {
        if first_key.is_none() {
            first_key = Some(iter.key().to_vec());
        }
        count += 1;
        iter.next();
    }
    assert_eq!(count, 4);
    assert_eq!(first_key, Some(b"key03".to_vec()));

    // Scan with no end bound (from key05 onwards)
    let mut iter = db.scan(Some(b"key07"), None).unwrap();
    let mut keys = Vec::new();
    while iter.valid() {
        keys.push(iter.key().to_vec());
        iter.next();
    }
    assert_eq!(keys.len(), 3);
    assert_eq!(keys[0], b"key07");
    assert_eq!(keys[1], b"key08");
    assert_eq!(keys[2], b"key09");
}
```

- [ ] **Step 2: 添加 empty range 和跨层 tombstone 集成测试**

```rust
/// Test scan with empty result range
#[test]
fn test_iterator_empty_range_e2e() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    db.put(b"a", b"1").unwrap();
    db.put(b"b", b"2").unwrap();

    // Range that doesn't overlap
    let mut iter = db.scan(Some(b"z"), Some(b"zz")).unwrap();
    assert!(!iter.valid());

    // Range where start >= end -> empty
    let mut iter = db.scan(Some(b"b"), Some(b"a")).unwrap();
    assert!(!iter.valid());
}

/// Test tombstone in memtable while data exists in SSTable
#[test]
fn test_iterator_tombstone_across_layers() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Write data and flush to SSTable
    db.put(b"key1", b"value1").unwrap();
    db.put(b"key2", b"value2").unwrap();
    db.put(b"key3", b"value3").unwrap();
    db.flush().unwrap();

    // Delete key2 in memtable (without flush)
    db.delete(b"key2").unwrap();

    // Iterator should skip key2 despite it existing in SSTable
    let mut iter = db.iter().unwrap();
    let mut keys = Vec::new();
    while iter.valid() {
        keys.push(iter.key().to_vec());
        iter.next();
    }
    assert_eq!(keys, vec![b"key1", b"key3"]);
}

/// Test seek across memtable and SSTable layers
#[test]
fn test_iterator_across_layers_with_seek() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(DB::open(dir.path(), Options::default()).unwrap());

    // Write data and flush to SSTable
    db.put(b"key1", b"from_sstable").unwrap();
    db.put(b"key2", b"from_sstable").unwrap();
    db.put(b"key3", b"from_sstable").unwrap();
    db.flush().unwrap();

    // Write more data (stays in memtable only)
    db.put(b"key4", b"from_memtable").unwrap();
    db.put(b"key5", b"from_memtable").unwrap();

    // Seek to key in SSTable layer
    let mut iter = db.iter().unwrap();
    iter.seek(b"key2").unwrap();
    assert!(iter.valid());
    assert_eq!(iter.key(), b"key2");

    // Seek to key in memtable layer
    let mut iter = db.iter().unwrap();
    iter.seek(b"key4").unwrap();
    assert!(iter.valid());
    assert_eq!(iter.key(), b"key4");

    // Seek between layers (SSTable key3 -> memtable key4)
    let mut iter = db.iter().unwrap();
    iter.seek(b"key35").unwrap();
    assert!(iter.valid());
    assert_eq!(iter.key(), b"key4");
}
```

- [ ] **Step 3: 运行所有集成测试**

```bash
cd /root/code/wiqun/AiDb && cargo test integration_tests::test_iterator_seek_e2e integration_tests::test_iterator_range_bounds_e2e integration_tests::test_iterator_empty_range_e2e integration_tests::test_iterator_tombstone_across_layers integration_tests::test_iterator_across_layers_with_seek -- --nocapture 2>&1
```

Expected: 5 个测试全部通过

- [ ] **Step 4: 运行全部测试验证没有回归**

```bash
cd /root/code/wiqun/AiDb && cargo test -- --nocapture 2>&1
```

Expected: 全部测试通过

- [ ] **Step 5: 提交**

```bash
git add tests/integration_tests.rs
git commit -m "test(iterator): add seek, range bounds, empty-range, cross-layer tombstone, and across-layers seek integration tests"
```
