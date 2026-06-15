---
name: multi-threaded-compaction-design
description: Compaction 多线程并行，支持 configurable compaction_threads
---

# 多线程 Compaction 设计规格

## 动机

当前 compaction 是单线程模型。当 compaction 工作量大（大 SST 文件 merge），后续 compaction 被阻塞。
多线程后，不重叠的 compaction 任务可并行执行，提高多核利用率。

## 设计

### 线程模型

- 单 coordinator（现有 `compaction_background_loop` 适配为 N 线程）
- 每个线程独立运行 `run_compaction_once()` 循环
- N 个 `crossbeam_channel::Receiver<()>`，每人一份
- `maybe_trigger_compaction()` 广播 `try_send` 给所有线程

### 文件占位 (Claim)

防止两个线程 pick 到重叠的 compaction：

```rust
// DB 新增
compacting: Mutex<HashSet<u64>>,

// run_compaction_once 中 pick 之后
fn try_claim_files(&self, task: &CompactionTask) -> bool {
    let mut guard = self.compacting.lock();
    for f in task.inputs.iter().chain(task.expanded_inputs.iter()) {
        if !guard.insert(f.file_number()) {
            // 回滚已插入的
            for f2 in task.inputs.iter().chain(task.expanded_inputs.iter()) {
                guard.remove(&f2.file_number());
            }
            return false;
        }
    }
    true
}

// Apply 完成后释放
fn release_files(&self, task: &CompactionTask) {
    let mut guard = self.compacting.lock();
    for f in task.inputs.iter().chain(task.expanded_inputs.iter()) {
        guard.remove(&f.file_number());
    }
}
```

Claim 失败时线程不阻塞，直接返回 `Ok(false)` 进入等待，下次信号再试。

### 配置

- `Options.compaction_threads` — 现有预留字段，改为可用，默认 1，上限 4
- `background_compaction` — 不变

### 信号机制

```rust
compaction_signals: Vec<crossbeam_channel::Sender<()>>,

fn maybe_trigger_compaction(&self) {
    let levels = self.sstables.read().iter().cloned().collect();
    if self.compaction_picker.pick_compaction(&levels).is_some() {
        for s in &self.compaction_signals {
            let _ = s.try_send(());
        }
    }
}
```

### 线程生命周期

- 启动：`start_compaction_threads()` 循环 N 次，每个线程绑定自己的 Receiver
- 关闭：统一 `compaction_shutdown` AtomicBool，join 所有 handle
- 通过 `Weak<DB>` 避免阻止析构

### drain_compactions

保持同步单线程执行，不参与 claim 机制。
用于测试场景，不涉及其它线程竞争。

## 改动文件

| 文件 | 改动 |
|------|------|
| `src/config.rs` | `compaction_threads` 注释更新，默认值不变 |
| `src/engine/db/inner.rs` | 线程启动/信号/claim/release/关闭 |

## 测试

- 现有 compaction 集成测试全部通过（threads=1 默认行为不变）
- `compaction_threads = 2` 时 post 数据 + drain_compactions 验证正确性
