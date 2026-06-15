---
name: subcompaction-design
description: CompactionJob 内部分裂为并行子压缩，减少大 compaction 的 wall-clock 时间
---

# Subcompaction 设计规格

## 动机

大 compaction job（多层多文件合并）串行执行完整归并写入，无法利用已实现的多线程 compaction 架构。
Subcompaction 将单次 compaction 按 key range 拆分为 N 个子任务并行处理，降低 wall-clock 时间。

## 设计

### 配置

```rust
// Options 新增
pub subcompaction_min_size: u64,  // 默认 64MB, 0=禁用
```

分裂数 N 由运行时决定：
```
input_size = sum(all_inputs file_size)
if input_size > subcompaction_min_size && compaction_threads > 1:
    N = min(input_size / subcompaction_min_size, compaction_threads, 4)
else:
    N = 1  (不分裂)
```

### CompactionJob::run() 修改

当前流程：
```
count_dedup_entries()  →  完整归并写入  →  单输出文件
```

新流程：
```
count_dedup_entries() → 采样分割点 → N × 并行子归并 → N 输出文件
```

**count_dedup_entries 阶段记录分割点**：
```
last_key = None
entry_count = 0
splits = Vec<Vec<u8>>     // N-1 个分割 user_key
split_interval = total_entries / N   // 总条目数由 count 提前得知

遍历 merge_iter:
  处理去重/墓碑（同现有逻辑）
  entry_count++
  if entry_count % split_interval == 0 && splits.len() < N-1:
      splits.push(当前 user_key.to_vec())
```

由于 count 阶段还未写入输出，这个开销是免费的——本来就要遍历一次。

**N 个子归并并行执行**（`std::thread::scope`）：
```
let results: Vec<CompactionResult> = std::thread::scope(|s| {
    let mut handles = Vec::new();
    for (i, (range_start, range_end)) in enumerate(ranges) {
        handles.push(s.spawn(|| {
            let mut mi = MergeIterator::with_range(input_readers, range_start, range_end);
            let mut builder = SSTableBuilder::new(output_path, ...);
            // 应用去重、快照保护（同现有逻辑）
            // builder.add(key, value)
            // finish → return CompactionResult
        }));
    }
    handles.into_iter().map(|h| h.join().unwrap()).collect()
});
```

**MergeIterator 新增 `with_range` 构造函数**：
```rust
pub fn with_range(readers: Vec<SSTableReader>, start: &[u8], end: &[u8]) -> Self {
    let iters = readers.iter().map(|r| r.iter().seek_to_target(start)).collect();
    // ...堆初始化...
    // next_entry() 检查：如果 key >= end，停止
}
```

### 输出处理

`CompactionJob::run()` 返回 `CompactionResult` 改为返回 `Vec<CompactionResult>`。
`CompactionJob::run_subcompactions()` 是新增方法，保持现有 `run()` 签名不变以保持兼容。

实际方案：
```rust
pub fn run(&self, file_number: u64) -> Result<Vec<CompactionResult>> {
    if self.should_split() {
        self.run_parallel(file_number, num_splits)
    } else {
        Ok(vec![self.run_single(file_number)?])
    }
}
```

### VersionEdit 适配

`inner.rs` 中 apply 阶段遍历 `Vec<CompactionResult>`，每个 result 创建一个 `SSTableReader` 和 `VersionEdit::AddFile`。

## 改动文件

| 文件 | 改动 |
|------|------|
| `src/config.rs` | 新增 `subcompaction_min_size` 选项 |
| `src/engine/compaction/job.rs` | CompactionJob::run() 分裂逻辑 + 并行子压缩 |
| `src/engine/compaction/merge.rs` | 新增 `with_range` 构造函数 + 边界检查 |
| `src/engine/db/inner.rs` | apply 阶段适配 `Vec<CompactionResult>` |

## 不改的

- CompactionPicker / CompactionTask — 不变
- SSTableBuilder / SSTableReader — 不变
- Bloom filter 逻辑 — 复用现有
- Snapshot 保护 — 继承同一 `min_snapshot_sequence`

## 测试

- 现有 compaction 测试全部通过（subcompaction 默认禁用）
- 新增 `test_subcompaction_large_job` — 大输入触发分裂，验证数据正确性
