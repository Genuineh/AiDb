//! @component aidb-engine
//! Prometheus metrics 验收测试 (需要 `monitoring` feature)
//!
//!   cargo test --test metrics --features monitoring -- --test-threads=1

mod common;

#[cfg(feature = "monitoring")]
#[path = "modules/metrics/prometheus.rs"]
mod prometheus;

#[cfg(not(feature = "monitoring"))]
#[test]
fn monitoring_feature_disabled_placeholder() {}
