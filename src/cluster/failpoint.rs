//! 故障注入框架 (cluster-test-util feature).
//!
//! 提供 7 种注入点, 支持 ARM / ARM ONCE / RELEASE / RELEASE ALL / STATUS.
//! 条件编译, release 构建零成本.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::OnceLock;

/// 故障注入点枚举.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailPoint {
    /// Append DB write 之前 panic.
    AppendBeforeDbWrite,
    /// Apply DB write 之前 panic.
    ApplyBeforePersist,
    /// Apply DB write 之后 panic.
    ApplyAfterPersist,
    /// Truncate DB 操作之前 panic.
    TruncateBeforePersist,
    /// Truncate DB 操作之后 panic.
    TruncateAfterPersist,
    /// Purge DB 操作之前 panic.
    PurgeBeforePersist,
    /// Purge DB 操作之后 panic.
    PurgeAfterPersist,
}

impl FailPoint {
    fn all() -> &'static [FailPoint] {
        &[
            FailPoint::AppendBeforeDbWrite,
            FailPoint::ApplyBeforePersist,
            FailPoint::ApplyAfterPersist,
            FailPoint::TruncateBeforePersist,
            FailPoint::TruncateAfterPersist,
            FailPoint::PurgeBeforePersist,
            FailPoint::PurgeAfterPersist,
        ]
    }

    /// 从字符串解析 failpoint 名称 (大小写不敏感).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<FailPoint> {
        let lower = s.to_lowercase();
        match lower.as_str() {
            "appendbeforedbwrite" | "append_before_db_write" => {
                Some(FailPoint::AppendBeforeDbWrite)
            }
            "applybeforepersist" | "apply_before_persist" => Some(FailPoint::ApplyBeforePersist),
            "applyafterpersist" | "apply_after_persist" => Some(FailPoint::ApplyAfterPersist),
            "truncatebeforepersist" | "truncate_before_persist" => {
                Some(FailPoint::TruncateBeforePersist)
            }
            "truncateafterpersist" | "truncate_after_persist" => {
                Some(FailPoint::TruncateAfterPersist)
            }
            "purgebeforepersist" | "purge_before_persist" => Some(FailPoint::PurgeBeforePersist),
            "purgeafterpersist" | "purge_after_persist" => Some(FailPoint::PurgeAfterPersist),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            FailPoint::AppendBeforeDbWrite => "AppendBeforeDbWrite",
            FailPoint::ApplyBeforePersist => "ApplyBeforePersist",
            FailPoint::ApplyAfterPersist => "ApplyAfterPersist",
            FailPoint::TruncateBeforePersist => "TruncateBeforePersist",
            FailPoint::TruncateAfterPersist => "TruncateAfterPersist",
            FailPoint::PurgeBeforePersist => "PurgeBeforePersist",
            FailPoint::PurgeAfterPersist => "PurgeAfterPersist",
        }
    }
}

/// 单个 failpoint 的状态.
#[derive(Debug, Clone)]
struct FailPointState {
    /// 是否已武装.
    armed: bool,
    /// 一次性模式: 触发一次后自动解除.
    once: bool,
}

/// 故障注入注册表.
pub struct FailPointRegistry {
    inner: RwLock<HashMap<FailPoint, FailPointState>>,
}

impl Default for FailPointRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FailPointRegistry {
    pub fn new() -> Self {
        let mut map = HashMap::new();
        for fp in FailPoint::all() {
            map.insert(
                *fp,
                FailPointState {
                    armed: false,
                    once: false,
                },
            );
        }
        Self {
            inner: RwLock::new(map),
        }
    }

    /// 武装一个 failpoint. 触发时会 panic.
    pub fn arm(&self, fp: FailPoint) {
        let mut inner = self.inner.write();
        inner.insert(
            fp,
            FailPointState {
                armed: true,
                once: false,
            },
        );
    }

    /// 武装一次性 failpoint. 触发一次后自动解除.
    pub fn arm_once(&self, fp: FailPoint) {
        let mut inner = self.inner.write();
        inner.insert(
            fp,
            FailPointState {
                armed: true,
                once: true,
            },
        );
    }

    /// 解除一个 failpoint.
    pub fn release(&self, fp: FailPoint) {
        let mut inner = self.inner.write();
        if let Some(state) = inner.get_mut(&fp) {
            state.armed = false;
            state.once = false;
        }
    }

    /// 解除所有 failpoint.
    pub fn release_all(&self) {
        let mut inner = self.inner.write();
        for state in inner.values_mut() {
            state.armed = false;
            state.once = false;
        }
    }

    /// 查询所有 failpoint 的状态, 返回 "(name) arm/once/release" 行.
    pub fn status(&self) -> String {
        let inner = self.inner.read();
        let mut lines: Vec<String> = Vec::new();
        for fp in FailPoint::all() {
            let state = inner.get(fp).unwrap();
            let status = if state.armed {
                if state.once {
                    "arm_once"
                } else {
                    "arm"
                }
            } else {
                "release"
            };
            lines.push(format!("{}: {}", fp.display_name(), status));
        }
        lines.join("\n")
    }

    /// 触发一个 failpoint. 如果已武装则 panic.
    /// 一次性模式: 触发后自动解除.
    pub fn fire(&self, fp: FailPoint) {
        let should_panic = {
            let mut inner = self.inner.write();
            let state = inner.get_mut(&fp).unwrap();
            if !state.armed {
                false
            } else {
                if state.once {
                    state.armed = false;
                    state.once = false;
                }
                true
            }
        };
        if should_panic {
            panic!("failpoint triggered: {}", fp.display_name());
        }
    }
}

// 全局 failpoint 注册表
static FAILPOINT_REGISTRY: OnceLock<FailPointRegistry> = OnceLock::new();

/// 获取全局 failpoint 注册表.
pub fn registry() -> &'static FailPointRegistry {
    FAILPOINT_REGISTRY.get_or_init(FailPointRegistry::new)
}

/// 触发 failpoint 的宏/函数, 条件编译.
/// 非 test-util 构建时为空实现.
#[inline(always)]
pub fn fire(fp: FailPoint) {
    registry().fire(fp);
}

// ---------------------------------------------------------------------------
// 网络黑洞 (network blackhole)
//
// 模拟网络分区: 把某个节点的出站 Raft RPC 定向丢弃. 按 (源节点, 目标 addr)
// 键控, 只影响"本节点 → 目标"方向的流, 不阻断其它节点间的互连 (否则分区窗口
// 内多数派也无法通信、无法选举新 leader).
//
// 只作用于 Raft 协议 RPC (RaftNetworkClient 的 append_entries / vote /
// full_snapshot), 扩展 RPC (get_key / remote_propose / migration) 不受影响.
// ---------------------------------------------------------------------------

use std::collections::HashSet;

use crate::cluster::types::NodeId;

/// 全局黑洞集合: `(源节点 id, 目标 addr)` 对.
/// `addr` 是归一化后的完整 RPC 地址 (如 `http://127.0.0.1:20349`),
/// 必须与 `RaftNetworkClient::target_addr` 精确一致.
static BLACKHOLES: OnceLock<RwLock<HashSet<(NodeId, String)>>> = OnceLock::new();

fn blackholes() -> &'static RwLock<HashSet<(NodeId, String)>> {
    BLACKHOLES.get_or_init(|| RwLock::new(HashSet::new()))
}

/// 网络黑洞 guard — Drop 时自动移除本 guard 注册的黑洞 (RAII).
///
/// 作用域跨 `await` 保持有效, 与 async 测试天然兼容; 不同测试各自持有
/// 独立 guard, Drop 时只清理自己注册的 `(src, target)` 对, 互不污染.
pub struct BlackholeGuard {
    src: NodeId,
    targets: HashSet<String>,
}

/// 为 `src` 节点的出站 Raft RPC 注册黑洞: 发往 `targets` 中任一 addr 的
/// RPC 将被 `RaftNetworkClient` 判活并丢弃 (返回网络错误).
pub fn blackhole(src: NodeId, targets: HashSet<String>) -> BlackholeGuard {
    {
        let mut inner = blackholes().write();
        for t in &targets {
            inner.insert((src, t.clone()));
        }
    }
    BlackholeGuard { src, targets }
}

/// 查询 `src` 节点发往 `addr` 的 RPC 是否命中黑洞.
pub fn blackhole_active(src: NodeId, addr: &str) -> bool {
    blackholes().read().contains(&(src, addr.to_string()))
}

impl Drop for BlackholeGuard {
    fn drop(&mut self) {
        let mut inner = blackholes().write();
        for t in &self.targets {
            inner.remove(&(self.src, t.clone()));
        }
    }
}

#[cfg(test)]
mod blackhole_tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn blackhole_guard_activates_and_cleans_up() {
        let src = 1u64;
        let targets = HashSet::from(["http://127.0.0.1:20349".to_string()]);

        assert!(!blackhole_active(src, "http://127.0.0.1:20349"));
        {
            let _guard = blackhole(src, targets.clone());
            assert!(blackhole_active(src, "http://127.0.0.1:20349"));
            // 源键控: 其它源节点不受影响
            assert!(!blackhole_active(2u64, "http://127.0.0.1:20349"));
        }
        // Drop 后清理
        assert!(!blackhole_active(src, "http://127.0.0.1:20349"));
    }

    #[test]
    #[serial]
    fn blackhole_target_addr_must_match_exactly() {
        let src = 1u64;
        let targets = HashSet::from(["http://127.0.0.1:20349".to_string()]);
        let _guard = blackhole(src, targets);
        assert!(blackhole_active(src, "http://127.0.0.1:20349"));
        // 未归一化 / 端口不同不命中
        assert!(!blackhole_active(src, "127.0.0.1:20349"));
        assert!(!blackhole_active(src, "http://127.0.0.1:20350"));
    }
}
