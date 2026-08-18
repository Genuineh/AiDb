//! 热路径 span 级别契约 (源码扫描)
//! @component aidb-observability
//!
//! 每个条目为 (文件相对路径, 锚点文本). 锚点是宏内 `name = "..."` (有显式名)
//! 或函数声明前缀 (无显式名时用函数名定位).

use std::path::Path;

/// 热路径 span 清单: 每条 SET/GET/DEL 或读路径必经, 必须 `level = "debug"`.
///
/// 覆盖 AGENTS.md 硬约束全集 (put/get/write/WAL/MemTable/SSTable/block/Raft
/// apply/propose) 中所有已显式 debug 的 span, 防止未来回退到默认 Info.
const HOT_PATH_SPANS: &[(&str, &str)] = &[
    // 集群读写路径 (E8/E19): 每条集群 SET/GET 的入口
    (
        "src/cluster/multi_raft_node.rs",
        "pub async fn propose_group(",
    ),
    (
        "src/cluster/multi_raft_node.rs",
        "pub async fn propose_key(",
    ),
    ("src/cluster/multi_raft_node.rs", "pub async fn get_key("),
    ("src/cluster/node.rs", "pub async fn propose("),
    // 集群复制/选举 RPC (每条日志批次发往 follower + 心跳)
    ("src/cluster/network.rs", "name = \"raft_rpc_ae\""),
    ("src/cluster/network.rs", "name = \"raft_rpc_vote\""),
    // 引擎单 key 读写路径
    ("src/engine/db/inner.rs", "name = \"db_put\""),
    ("src/engine/db/inner.rs", "name = \"db_get\""),
    ("src/engine/db/inner.rs", "name = \"db_delete\""),
    ("src/engine/db/inner.rs", "name = \"db_scan\""),
    ("src/engine/db/inner.rs", "name = \"db_write_batch\""),
    ("src/engine/db/inner.rs", "name = \"db_write_batch_no_wal\""),
    ("src/engine/db/inner.rs", "name = \"db_delete_range\""),
    ("src/engine/memtable/table.rs", "name = \"mem_put\""),
    ("src/engine/memtable/table.rs", "name = \"mem_get\""),
    ("src/engine/memtable/table.rs", "name = \"mem_search\""),
    ("src/engine/memtable/table.rs", "name = \"mem_delete\""),
    // 读路径: block cache + sstable seek/block 读
    ("src/engine/cache/block_cache.rs", "name = \"cache_get\""),
    ("src/engine/cache/block_cache.rs", "name = \"cache_insert\""),
    ("src/engine/sstable/reader.rs", "name = \"sst_seek\""),
    ("src/engine/sstable/block.rs", "name = \"sst_block_seek\""),
    (
        "src/engine/sstable/block_io.rs",
        "name = \"sst_block_read\"",
    ),
    // 写路径: WAL
    ("src/engine/wal/writer.rs", "pub fn write_record("),
    ("src/engine/wal/writer.rs", "pub fn flush("),
    ("src/engine/wal/writer.rs", "pub fn sync_data("),
    ("src/engine/wal/manager.rs", "name = \"wal_write\""),
    ("src/engine/wal/manager.rs", "name = \"wal_flush\""),
    ("src/engine/wal/manager.rs", "name = \"wal_sync\""),
    // 后台高频 flush/compaction 路径 (持续运行, 带 otel 时每条记录创建 span 进 OTel layer)
    ("src/engine/sstable/builder.rs", "name = \"sst_build_add\""),
    ("src/engine/compaction/merge.rs", "name = \"cmp_merge\""),
];

/// 返回锚点文本之前的最近一个 instrument 属性文本.
fn instrument_attr_before(content: &str, anchor: &str) -> Option<String> {
    let anchor_pos = content.find(anchor)?;
    let prefix = &content[..anchor_pos];
    let start = [
        prefix.rfind("#[tracing::instrument"),
        prefix.rfind("#[instrument"),
    ]
    .into_iter()
    .flatten()
    .max()?;
    let attr = &content[start..];
    let end = attr.find(']')?;
    Some(attr[..=end].to_string())
}

/// 热路径 span 必须显式 `level = "debug"` (AGENTS.md 硬约束).
///
/// 无 `level` 参数时 tracing 默认 Info, 生产 `RUST_LOG=info` 会创建 span 并进入
/// OTel, 是 Phase 2 profiling 定位到的集群压测性能主因.
#[test]
fn test_hot_path_span_level_is_debug() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for (rel, anchor) in HOT_PATH_SPANS {
        let content = std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| {
            panic!("read {rel} failed: {e}");
        });
        match instrument_attr_before(&content, anchor) {
            Some(attr) => {
                if !attr.contains("level = \"debug\"") {
                    violations.push(format!("{rel} @ {anchor}\n  -> {attr}"));
                }
            }
            None => violations.push(format!("{rel} @ {anchor}: instrument attr not found")),
        }
    }

    assert!(
        violations.is_empty(),
        "热路径 span 必须显式 level = \"debug\" (AGENTS.md 硬约束), 否则生产 \
         RUST_LOG=info 下会创建 span 并进入 OTel:\n{}",
        violations.join("\n")
    );
}
