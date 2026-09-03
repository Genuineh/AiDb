//! @component aidb-engine
//! Prometheus metrics 验收测试 (需要 `monitoring` feature)
//!
//!   cargo test --test metrics --features monitoring -- --test-threads=1

#[cfg(feature = "monitoring")]
#[path = "modules/metrics/prometheus.rs"]
mod prometheus;

#[cfg(not(feature = "monitoring"))]
#[path = "modules/metrics/placeholder.rs"]
mod placeholder;

#[path = "modules/metrics/statistics_test.rs"]
mod statistics_test;
