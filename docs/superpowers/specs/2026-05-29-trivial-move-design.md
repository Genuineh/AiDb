---
name: trivial-move-design
description: Trivial Move 优化 — compaction 时非重叠 SST 直接提升不重写
---

# Trivial Move 设计规格

## 动机

当前 CompactionJob 对所有输入 SST 执行全量 merge-dedup-rewrite。
当 level N 的 SST 与 level N+1 无 key range 重叠时，重写是纯浪费。
Trivial move 跳过重写，直接提升文件到下一级，节省 I/O 和 CPU。

## 设计

### 检测点

在 `CompactionPicker::pick_level_n()` 中：

```
seed → find expanded (目标 level 重叠文件)
if expanded.is_empty() → is_trivial_move = true
else → 保持现有 merge 路径
```

同样适用于 `pick_level0()`：当 L0 只有一个文件且不与 L1 重叠时。

### CompactionTask

新增字段 `is_trivial_move: bool`。避免引入新枚举类型，保持最小改动。

### 执行流程

```
if task.is_trivial_move:
  1. rename SST: {n:06}_L{n}.sst → {n:06}_L{n+1}.sst
  2. VersionEdit: AddFile(new_level) + DeleteFile(old_level)
  3. 更新 sstables 内存列表 (Arc 从旧 level vec 移到新 level vec)
  4. 跳过旧 input SST 的 remove_file
else:
  ... 现有完整 merge 路径 (不变)
```

### 不涉及

- MergeIterator / SSTableBuilder / bloom / 压缩 — 不碰
- Snapshot 保护 — internal key 不变，天然兼容
- Tombstone GC — trivial move 保留 tombstone，安全

## 改动文件

| 文件 | 改动 |
|------|------|
| `src/engine/compaction/picker.rs` | 检测无重叠 + 标记 `is_trivial_move` |
| `src/engine/compaction/mod.rs` | 如需重导出新字段 |
| `src/engine/db/inner.rs` | `run_compaction_once()` 短路逻辑 |

## 测试

- Picker 测试：验证无重叠时返回 trivial move
- 集成测试：执行 compaction，验证文件被 move 而非 rewrite
- 内容一致性：move 后读取内容正确
