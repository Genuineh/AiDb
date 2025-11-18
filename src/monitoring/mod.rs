//! Monitoring and metrics collection module
//!
//! This module provides Prometheus-compatible metrics collection for AiDb.
//! It tracks request metrics, system metrics, business metrics, and errors.

#[cfg(feature = "monitoring")]
pub mod metrics;

#[cfg(feature = "monitoring")]
pub mod server;

#[cfg(feature = "monitoring")]
pub use metrics::{
    record_delete_operation, record_flush_operation, record_get_operation, record_put_operation,
    register_metrics, MetricsCollector,
};

#[cfg(feature = "monitoring")]
pub use server::MetricsServer;

// Re-export for convenience
#[cfg(feature = "monitoring")]
pub use prometheus;
