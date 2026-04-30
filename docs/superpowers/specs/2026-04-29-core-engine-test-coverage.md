# 核心引擎层测试完善设计

## 目标

填补 AiDb 核心引擎层的测试缺口，聚焦两个模块：
1. `CompactionJob::run()` — 目前无单元测试
2. `DBIterator` — 目前仅 1 个基础迭代测试

采用方案 A：精准补齐 — 直接在源文件中追加 `#[cfg(test)]` 单元测试，并在 `tests/` 中补充集成测试。

---

## 1. CompactionJob 单元测试

**位置：** `src/compaction/mod.rs` — 追加到现有 `#[cfg(test)] mod tests`

使用 `SSTableBuilder` 直接在测试中创建临时 SSTable 文件，构造 `CompactionJob` 调用 `run()`，用 `SSTableReader` 验证输出。

### 测试用例

| # | 测试函数 | 场景 | 验证点 |
|---|----------|------|--------|
| 1 | `test_compaction_basic_merge` | 两个 SSTable（keys a-c, d-f），compact 到 level 1 | 输出包含全部 6 个 key |
| 2 | `test_compaction_removes_duplicates` | 两个 SSTable 含重叠 key，compact 到 level 1 | 输出无重复 key |
| 3 | `test_compaction_removes_tombstones_at_level1` | SSTable 含空 value（tombstone），compact 到 level 1 | tombstone 被移除 |
| 4 | `test_compaction_preserves_tombstones_at_level0` | SSTable 含 tombstone，compact 到 level 0 | tombstone 保留 |
| 5 | `test_compaction_single_input` | 单个 SSTable 输入 | 输出与原文件一致 |
| 6 | `test_compaction_all_entries_removed` | 全部为 tombstone，compact 到 level 1 | 输出为 0 条，file_number=0 |

---

## 2. DBIterator 单元测试

**位置：** `src/iterator.rs` — 追加到现有 `#[cfg(test)] mod tests`

通过 `Arc<DB>` 构造 `DBIterator`（使用 `db.iter()` 或 `db.scan()`），写入数据后验证迭代行为。

### 测试用例

| # | 测试函数 | 场景 | 验证点 |
|---|----------|------|--------|
| 1 | `test_iterator_empty_db` | 无数据 | `iter.valid() == false` |
| 2 | `test_iterator_seek_existing` | seek("key2") | 定位到 key2，value 正确 |
| 3 | `test_iterator_seek_nonexistent` | seek("key15") 其时 key1,key2,key3 存在 | 定位到 key2（如果 key15 > 所有 key 则 valid=false） |
| 4 | `test_iterator_seek_to_first` | seek_to_first() | 定位到最小 key |
| 5 | `test_iterator_range` | scan("key1", "key3") | 只返回 key1, key2 |
| 6 | `test_iterator_tombstone_filtering` | 写入 key1, key2，删除 key2 | 迭代只出现 key1 |
| 7 | `test_iterator_overwrite` | 写入 key1=v1，覆盖 key1=v2 | 迭代显示 key1=v2 |
| 8 | `test_iterator_seek_to_last` | seek_to_last() | 定位到最大 key |

---

## 3. 集成测试补充

**位置：** `tests/compaction_tests.rs` 和 `tests/integration_tests.rs`

### Compaction 集成测试

| # | 测试函数 | 场景 | 验证点 |
|---|----------|------|--------|
| 1 | `test_compaction_consolidation` | 多轮 flush + compaction 后 | DB 中无重复 key |
| 2 | `test_compaction_all_deleted` | 所有 key 被删除后触发 compaction | DB 为空，打开正常 |

### Iterator 集成测试

| # | 测试函数 | 场景 | 验证点 |
|---|----------|------|--------|
| 1 | `test_iterator_seek_e2e` | seek 到不同位置 | 每次定位正确 |
| 2 | `test_iterator_range_e2e` | scan() 不同范围 | 范围准确 |
| 3 | `test_iterator_across_all_layers` | 数据在 memtable + SSTable + immutable memtable | 迭代器正确合并所有层 |
| 4 | `test_iterator_empty_range` | scan("z", "zz") | 返回空 |
| 5 | `test_iterator_tombstone_span_layers` | 数据在 SSTable，tombstone 在 memtable | 迭代跳过已删 key |

---

## 不纳入范围

- 错误路径测试（损坏 SSTable、I/O 错误等）— 留待后续
- 测试基础设施重构（如提取公共 test_util）— 当前直接内联
- 并发 compaction 测试 — 已有集成测试覆盖

---

## 实施顺序

1. CompactionJob 单元测试（`src/compaction/mod.rs`）
2. DBIterator 单元测试（`src/iterator.rs`）
3. 集成测试（`tests/compaction_tests.rs` 和 `tests/integration_tests.rs`）
4. 运行 `cargo test` 验证全部通过
