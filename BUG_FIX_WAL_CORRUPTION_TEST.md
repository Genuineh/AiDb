# Bug Fix: WAL Corruption Test Directory Path

## 问题描述

**发现时间**: 2025-11-06  
**严重程度**: Medium  
**类型**: Test Bug

### 问题

`test_recovery_with_wal_corruption` 测试在错误的目录中查找 WAL 文件，导致测试无法有效验证 WAL 损坏处理。

**错误代码**:
```rust
// 错误：在 path.join("wal") 子目录中查找
let wal_dir = path.join("wal");
if let Ok(entries) = fs::read_dir(&wal_dir) {
    // ...查找 .wal 文件
}
```

**问题**:
1. WAL 文件实际上直接创建在数据库根目录 `path` 中
2. WAL 文件扩展名是 `.log`（不是 `.wal`）
3. 测试永远找不到 WAL 文件，因此无法执行损坏操作
4. 测试总是通过，但实际上没有测试任何内容

**根据代码验证** (`src/lib.rs`):
- Line 159: `let wal_path = path.join(wal::wal_filename(1));`
- Line 681: WAL 文件直接在 `path` 目录中创建
- `wal::wal_filename()` 返回 `"{:06}.log"` 格式（例如 `000001.log`）

## 解决方案

### 修复 1: 正确的目录和文件扩展名

```rust
// 修复：在数据库根目录中查找，并使用正确的扩展名
if let Ok(entries) = fs::read_dir(&path) {  // 直接使用 path
    for entry in entries.flatten() {
        let file_path = entry.path();
        // WAL 文件扩展名是 .log，不是 .wal
        if file_path.extension().and_then(|s| s.to_str()) == Some("log") {
            // 损坏 WAL 文件
            if let Ok(metadata) = fs::metadata(&file_path) {
                let size = metadata.len();
                if size > 100 {
                    fs::write(&file_path, vec![0u8; (size / 2) as usize]).ok();
                }
            }
        }
    }
}
```

### 修复 2: 改进测试逻辑

原测试在数据正常关闭后才损坏 WAL，此时数据已经 flush 到 SSTable，WAL 损坏不会影响恢复。

**改进**:
1. 使用 `simulate_crash()` 模拟崩溃（不 flush 数据）
2. 数据只在 WAL 中存在
3. 损坏 WAL 文件
4. 尝试恢复并验证数据丢失

```rust
// 写入数据但崩溃（不 flush）
{
    let db = DB::open(&path, Options::default()).unwrap();
    for i in 0..50 {
        let key = format!("wal_key_{}", i);
        db.put(key.as_bytes(), b"wal_value").unwrap();
    }
    simulate_crash(db);  // 不调用 Drop，数据只在 WAL 中
}

// 损坏 WAL 文件
// ...

// 恢复并验证数据丢失
let db = DB::open(&path, Options::default()).unwrap();
let mut recovered = 0;
let mut lost = 0;
for i in 0..50 {
    if db.get(format!("wal_key_{}", i).as_bytes()).unwrap().is_some() {
        recovered += 1;
    } else {
        lost += 1;
    }
}

// 由于 WAL 损坏，应该有数据丢失
assert!(lost > 0, "Some data should be lost due to WAL corruption");
```

## 测试结果

### 修复前
```
test test_recovery_with_wal_corruption ... ok
```
- ❌ 测试通过，但未找到任何 WAL 文件
- ❌ 没有实际测试 WAL 损坏处理
- ❌ 假阳性结果

### 修复后
```bash
$ cargo test --test crash_recovery_tests test_recovery_with_wal_corruption -- --nocapture

Corrupting WAL file: "/tmp/.tmp4Fm0Cm/000001.log" (truncating from 1790 to 895 bytes)
Database opened after WAL corruption
Recovered 0 keys, lost 50 keys due to WAL corruption
test test_recovery_with_wal_corruption ... ok
```

- ✅ 正确找到 WAL 文件（`000001.log`）
- ✅ 成功损坏 WAL 文件（1790 → 895 字节）
- ✅ 验证数据丢失（0 恢复，50 丢失）
- ✅ 真正测试了 WAL 损坏处理

## 影响

### 测试覆盖
- **修复前**: WAL 损坏处理未被测试
- **修复后**: WAL 损坏处理被正确测试

### 代码质量
- 发现了测试假阳性问题
- 提高了测试的有效性
- 确保 WAL 损坏能被正确处理

### 所有测试状态
```bash
$ cargo test --test crash_recovery_tests

running 11 tests
test test_data_consistency_after_crash ... ok
test test_multiple_crash_recovery_cycles ... ok
test test_recovery_after_proper_shutdown ... ok
test test_recovery_after_write_crash ... ok
test test_recovery_during_flush ... ok
test test_recovery_empty_database ... ok
test test_recovery_mixed_operations ... ok
test test_recovery_partial_writes ... ok
test test_recovery_with_deletes ... ok
test test_recovery_with_wal_corruption ... ok ✅
test test_wal_replay_correctness ... ok

test result: ok. 11 passed; 0 failed
```

## 相关代码

### WAL 文件创建 (`src/lib.rs`)

```rust
// Line 157-177: DB::open 中查找 WAL 文件
let mut wal_number = 1u64;
let mut latest_wal_path = path.join(wal::wal_filename(1));

// 扫描目录找到最新的 WAL 文件
if path.exists() {
    if let Ok(entries) = std::fs::read_dir(&path) {
        for entry in entries.flatten() {
            if let Some(filename) = entry.file_name().to_str() {
                if let Some(num) = wal::parse_wal_filename(filename) {
                    if num >= wal_number {
                        wal_number = num;
                        latest_wal_path = entry.path();
                    }
                }
            }
        }
    }
}
```

### WAL 文件命名 (`src/wal/mod.rs`)

```rust
/// Generate a WAL filename for a given sequence number
pub fn wal_filename(seq: u64) -> String {
    format!("{:06}.log", seq)  // 例如: 000001.log
}

/// Parse a WAL filename to extract the sequence number
pub fn parse_wal_filename(filename: &str) -> Option<u64> {
    if !filename.ends_with(".log") {
        return None;
    }
    let name = filename.trim_end_matches(".log");
    name.parse().ok()
}
```

## 经验教训

1. **测试假阳性风险**: 测试可能通过，但实际上没有测试任何内容
2. **验证测试有效性**: 测试应该有可观察的效果（如日志输出）
3. **了解实现细节**: 测试需要准确反映实际的文件结构和命名
4. **崩溃模拟**: 使用 `mem::forget` 而不是 `drop` 来模拟真实崩溃

## 检查清单

为避免类似问题，测试应该：

- [x] 验证文件/目录确实存在
- [x] 输出日志确认操作已执行
- [x] 使用断言验证预期行为
- [x] 在修改文件系统时检查返回值
- [x] 理解被测系统的实际实现

## 总结

这个 bug 修复：
- ✅ 修正了 WAL 文件查找路径（从 `path.join("wal")` 到 `path`）
- ✅ 修正了文件扩展名（从 `.wal` 到 `.log`）
- ✅ 改进了测试逻辑（崩溃后损坏，而不是正常关闭后）
- ✅ 增加了有效性验证（确认文件被损坏）
- ✅ 验证了预期行为（数据丢失）

现在测试真正验证了数据库能够处理 WAL 损坏的情况。

---

**修复者**: Cursor AI Agent  
**审核者**: User  
**日期**: 2025-11-06  
**版本**: 1.0
