//! 可观测性验证工具
//! @component aidb-test-harness
//!
//! 提供跨模块共享的 tracing/metrics 测试辅助函数.
//! 各模块测试可通过 EventCatcher 捕获 tracing events.
//!
//! 使用方式:
//!   use crate::common::observability::EventCatcher;
//!
//!   let catcher = EventCatcher::new();
//!   let _guard = tracing_subscriber::registry()
//!       .with(catcher.clone())
//!       .set_default();
//!
//!   // ... 执行被测操作 ...
//!   catcher.assert_event_emitted("wal.write.complete");

use parking_lot::ReentrantMutex;
use std::sync::{Arc, Mutex};

/// 串行化所有安装 tracing subscriber 的测试, 避免并行时抢默认 subscriber.
static TRACING_TEST_LOCK: ReentrantMutex<()> = ReentrantMutex::new(());

/// 在所有测试启动前安装一个无输出的全局默认 subscriber,
/// 确保 `#[instrument]` callsite 在首次访问时被标记为"感兴趣",
/// 避免无 subscriber 测试先行访问后永久污染缓存.
#[cfg(test)]
#[ctor::ctor]
fn init_global_tracing_subscriber() {
    use tracing_subscriber::util::SubscriberInitExt;

    let _ = tracing_subscriber::Registry::default().try_init();
}

pub fn tracing_test_lock() -> parking_lot::ReentrantMutexGuard<'static, ()> {
    TRACING_TEST_LOCK.lock()
}

/// 捕获 tracing events 的 MockLayer.
///
/// 每个 event 以格式化字符串形式存储, 后续通过 assert 方法验证.
/// 内部使用 Arc<Mutex<...>>, 支持 Clone 以通过 tracing_subscriber::Layer 接口.
#[derive(Clone)]
pub struct EventCatcher {
    events: Arc<Mutex<Vec<String>>>,
}

/// 内部 visitor, 将 event 的 fields 收集为字符串
struct StringVisitor(pub String);

impl tracing::field::Visit for StringVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if !self.0.is_empty() {
            self.0.push_str(", ");
        }
        self.0.push_str(&format!("{}={}", field.name(), value));
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if !self.0.is_empty() {
            self.0.push_str(", ");
        }
        self.0.push_str(&format!("{}={:?}", field.name(), value));
    }
}

impl EventCatcher {
    /// 创建新的 EventCatcher
    pub fn new() -> Self {
        EventCatcher {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 获取所有捕获的事件 (清空内部缓冲区)
    pub fn drain(&self) -> Vec<String> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }

    /// 获取事件总数
    pub fn event_count(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    /// 断言包含 substr 的 event 已发出
    ///
    /// # Panics
    /// 若没有任何 event 包含 substr.
    pub fn assert_event_emitted(&self, substr: &str) {
        let guard = self.events.lock().unwrap();
        assert!(
            guard.iter().any(|e| e.contains(substr)),
            "expected event containing '{}' to have been emitted.\nCaptured events: {:#?}",
            substr,
            *guard
        );
    }

    /// 断言包含 substr 的 event 已发出至少 n 次
    ///
    /// # Panics
    /// 若包含 substr 的 event 数量 < n.
    pub fn assert_event_emitted_n(&self, substr: &str, n: usize) {
        let guard = self.events.lock().unwrap();
        let count = guard.iter().filter(|e| e.contains(substr)).count();
        assert!(
            count >= n,
            "expected event containing '{}' emitted {} times, got {}.\nCaptured events: {:#?}",
            substr,
            n,
            count,
            *guard
        );
    }

    /// 清空所有已捕获事件
    pub fn clear(&self) {
        self.events.lock().unwrap().clear();
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for EventCatcher {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = StringVisitor(String::new());
        event.record(&mut visitor);
        let mut guard = self.events.lock().unwrap();
        guard.push(visitor.0);
    }
}

impl Default for EventCatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// 在持有 `tracing_test_lock` 下执行 `f`, 捕获期间所有 tracing events.
///
/// 含 tracing 的集成测试必须与无 subscriber 的同模块测试串行 (CI 用
/// `--test-threads=1`), 否则并行跑 Writer 等功能测试时会污染 callsite 兴趣缓存.
pub fn capture_events_under_lock<F>(f: F) -> Vec<String>
where
    F: FnOnce(),
{
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    let _lock = tracing_test_lock();
    let catcher = EventCatcher::new();
    let subscriber = Registry::default()
        .with(tracing_subscriber::filter::LevelFilter::TRACE)
        .with(catcher.clone());
    tracing::subscriber::with_default(subscriber, f);
    catcher.drain()
}

/// 创建一个带 EventCatcher 的 tracing subscriber, 返回 guard.
/// guard 被 drop 时 subscriber 自动卸载.
///
/// ```
/// let catcher = EventCatcher::new();
/// let _guard = observability::init_test_subscriber(catcher.clone());
/// ```
pub fn init_test_subscriber(catcher: EventCatcher) -> tracing::subscriber::DefaultGuard {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::Registry;

    Registry::default()
        .with(catcher)
        .with(tracing_subscriber::filter::EnvFilter::new("debug"))
        .set_default()
}
