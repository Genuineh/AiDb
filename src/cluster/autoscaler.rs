//! Auto-scaling system for AiDb distributed cluster
//!
//! This module provides the AutoScaler which automatically scales the cluster
//! based on metrics and policies:
//! - Collects system metrics (CPU, memory, QPS, storage)
//! - Evaluates scaling policies with thresholds
//! - Automatically triggers scale-out/scale-in operations
//! - Manages cooldown periods to prevent thrashing

use super::scaling::ScalingManager;
use super::shard_group::ShardGroupManager;
use crate::error::{Error, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// System metrics for a shard or node
#[derive(Debug, Clone)]
pub struct SystemMetrics {
    /// CPU usage percentage (0.0 - 100.0)
    pub cpu_percent: f64,
    /// Memory usage percentage (0.0 - 100.0)
    pub memory_percent: f64,
    /// Requests per second
    pub qps: u64,
    /// Storage usage in bytes
    pub storage_bytes: u64,
    /// Storage capacity in bytes
    pub storage_capacity_bytes: u64,
    /// Timestamp when metrics were collected
    pub timestamp: Instant,
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemMetrics {
    /// Create new metrics with current timestamp
    pub fn new() -> Self {
        Self {
            cpu_percent: 0.0,
            memory_percent: 0.0,
            qps: 0,
            storage_bytes: 0,
            storage_capacity_bytes: 0,
            timestamp: Instant::now(),
        }
    }

    /// Calculate storage usage percentage
    pub fn storage_percent(&self) -> f64 {
        if self.storage_capacity_bytes == 0 {
            0.0
        } else {
            (self.storage_bytes as f64 / self.storage_capacity_bytes as f64) * 100.0
        }
    }

    /// Check if metrics are stale (older than threshold)
    pub fn is_stale(&self, threshold: Duration) -> bool {
        self.timestamp.elapsed() > threshold
    }
}

/// Threshold for a specific metric
#[derive(Debug, Clone)]
pub struct MetricThreshold {
    /// Scale out threshold (trigger when metric exceeds this)
    pub scale_out: f64,
    /// Scale in threshold (trigger when metric is below this)
    pub scale_in: f64,
}

impl MetricThreshold {
    /// Create a new threshold
    pub fn new(scale_out: f64, scale_in: f64) -> Self {
        Self { scale_out, scale_in }
    }
}

/// Scaling policy that determines when to scale
#[derive(Debug, Clone)]
pub struct ScalingPolicy {
    /// Name of the policy
    pub name: String,
    /// CPU threshold
    pub cpu_threshold: MetricThreshold,
    /// Memory threshold
    pub memory_threshold: MetricThreshold,
    /// QPS threshold
    pub qps_threshold: u64,
    /// Storage threshold percentage
    pub storage_threshold: MetricThreshold,
    /// Cooldown period after a scaling operation
    pub cooldown_duration: Duration,
    /// Minimum evaluation periods before scaling
    pub min_evaluation_periods: usize,
}

impl Default for ScalingPolicy {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            cpu_threshold: MetricThreshold::new(80.0, 30.0),
            memory_threshold: MetricThreshold::new(80.0, 30.0),
            qps_threshold: 10000,
            storage_threshold: MetricThreshold::new(80.0, 40.0),
            cooldown_duration: Duration::from_secs(300), // 5 minutes
            min_evaluation_periods: 3,
        }
    }
}

impl ScalingPolicy {
    /// Create a conservative scaling policy
    pub fn conservative() -> Self {
        Self {
            name: "conservative".to_string(),
            cpu_threshold: MetricThreshold::new(90.0, 20.0),
            memory_threshold: MetricThreshold::new(90.0, 20.0),
            qps_threshold: 15000,
            storage_threshold: MetricThreshold::new(90.0, 30.0),
            cooldown_duration: Duration::from_secs(600), // 10 minutes
            min_evaluation_periods: 5,
        }
    }

    /// Create an aggressive scaling policy
    pub fn aggressive() -> Self {
        Self {
            name: "aggressive".to_string(),
            cpu_threshold: MetricThreshold::new(70.0, 40.0),
            memory_threshold: MetricThreshold::new(70.0, 40.0),
            qps_threshold: 5000,
            storage_threshold: MetricThreshold::new(70.0, 50.0),
            cooldown_duration: Duration::from_secs(120), // 2 minutes
            min_evaluation_periods: 2,
        }
    }

    /// Evaluate if metrics indicate scale-out is needed
    pub fn should_scale_out(&self, metrics: &SystemMetrics) -> bool {
        metrics.cpu_percent > self.cpu_threshold.scale_out
            || metrics.memory_percent > self.memory_threshold.scale_out
            || metrics.qps > self.qps_threshold
            || metrics.storage_percent() > self.storage_threshold.scale_out
    }

    /// Evaluate if metrics indicate scale-in is needed
    pub fn should_scale_in(&self, metrics: &SystemMetrics) -> bool {
        metrics.cpu_percent < self.cpu_threshold.scale_in
            && metrics.memory_percent < self.memory_threshold.scale_in
            && metrics.qps < self.qps_threshold / 2
            && metrics.storage_percent() < self.storage_threshold.scale_in
    }
}

/// Scaling decision
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalingDecision {
    /// No scaling needed
    NoAction,
    /// Scale out (add resources)
    ScaleOut,
    /// Scale in (remove resources)
    ScaleIn,
    /// In cooldown period
    Cooldown,
}

/// Auto-scaler state
#[derive(Debug)]
struct AutoScalerState {
    /// Metrics per shard
    metrics: HashMap<String, SystemMetrics>,
    /// Last scaling operation time
    last_scaling_time: Option<Instant>,
    /// Consecutive evaluation periods for scale-out
    scale_out_periods: usize,
    /// Consecutive evaluation periods for scale-in
    scale_in_periods: usize,
    /// Whether auto-scaling is currently enabled
    enabled: bool,
}

impl AutoScalerState {
    fn new() -> Self {
        Self {
            metrics: HashMap::new(),
            last_scaling_time: None,
            scale_out_periods: 0,
            scale_in_periods: 0,
            enabled: false,
        }
    }

    fn reset_periods(&mut self) {
        self.scale_out_periods = 0;
        self.scale_in_periods = 0;
    }
}

/// Automatic scaling manager
pub struct AutoScaler {
    /// Reference to the scaling manager
    scaling_manager: Arc<ScalingManager>,
    /// Reference to the shard group manager
    shard_manager: Arc<ShardGroupManager>,
    /// Scaling policy
    policy: ScalingPolicy,
    /// Internal state
    state: Arc<RwLock<AutoScalerState>>,
}

impl AutoScaler {
    /// Create a new auto-scaler
    ///
    /// # Arguments
    /// * `scaling_manager` - The scaling manager to use for operations
    /// * `shard_manager` - The shard group manager
    /// * `policy` - Scaling policy to apply
    pub fn new(
        scaling_manager: Arc<ScalingManager>,
        shard_manager: Arc<ShardGroupManager>,
        policy: ScalingPolicy,
    ) -> Self {
        Self {
            scaling_manager,
            shard_manager,
            policy,
            state: Arc::new(RwLock::new(AutoScalerState::new())),
        }
    }

    /// Create an auto-scaler with default policy
    pub fn with_defaults(
        scaling_manager: Arc<ScalingManager>,
        shard_manager: Arc<ShardGroupManager>,
    ) -> Self {
        Self::new(scaling_manager, shard_manager, ScalingPolicy::default())
    }

    /// Enable auto-scaling
    pub fn enable(&self) {
        let mut state = self.state.write();
        state.enabled = true;
        log::info!("Auto-scaling enabled with policy: {}", self.policy.name);
    }

    /// Disable auto-scaling
    pub fn disable(&self) {
        let mut state = self.state.write();
        state.enabled = false;
        state.reset_periods();
        log::info!("Auto-scaling disabled");
    }

    /// Check if auto-scaling is enabled
    pub fn is_enabled(&self) -> bool {
        self.state.read().enabled
    }

    /// Update metrics for a shard
    ///
    /// # Arguments
    /// * `shard_id` - Identifier of the shard
    /// * `metrics` - Latest metrics for the shard
    pub fn update_metrics(&self, shard_id: String, metrics: SystemMetrics) {
        let mut state = self.state.write();
        state.metrics.insert(shard_id, metrics);
    }

    /// Get current metrics for a shard
    ///
    /// # Arguments
    /// * `shard_id` - Identifier of the shard
    pub fn get_metrics(&self, shard_id: &str) -> Option<SystemMetrics> {
        self.state.read().metrics.get(shard_id).cloned()
    }

    /// Get aggregated metrics across all shards
    pub fn get_aggregate_metrics(&self) -> SystemMetrics {
        let state = self.state.read();

        if state.metrics.is_empty() {
            return SystemMetrics::new();
        }

        let count = state.metrics.len() as f64;
        let mut aggregate = SystemMetrics::new();

        for metrics in state.metrics.values() {
            aggregate.cpu_percent += metrics.cpu_percent;
            aggregate.memory_percent += metrics.memory_percent;
            aggregate.qps += metrics.qps;
            aggregate.storage_bytes += metrics.storage_bytes;
            aggregate.storage_capacity_bytes += metrics.storage_capacity_bytes;
        }

        aggregate.cpu_percent /= count;
        aggregate.memory_percent /= count;

        aggregate
    }

    /// Evaluate whether scaling is needed
    ///
    /// This checks the current metrics against the policy and returns
    /// a scaling decision.
    pub fn evaluate(&self) -> ScalingDecision {
        let state = self.state.write();

        // If not enabled, no action
        if !state.enabled {
            return ScalingDecision::NoAction;
        }

        // Check cooldown period
        if let Some(last_time) = state.last_scaling_time {
            if last_time.elapsed() < self.policy.cooldown_duration {
                return ScalingDecision::Cooldown;
            }
        }

        // Get aggregate metrics
        drop(state); // Release lock before calling get_aggregate_metrics
        let metrics = self.get_aggregate_metrics();
        let mut state = self.state.write(); // Reacquire lock

        // Evaluate scale-out
        if self.policy.should_scale_out(&metrics) {
            state.scale_out_periods += 1;
            state.scale_in_periods = 0;

            if state.scale_out_periods >= self.policy.min_evaluation_periods {
                log::info!(
                    "Scale-out condition met for {} consecutive periods",
                    state.scale_out_periods
                );
                return ScalingDecision::ScaleOut;
            }
        }
        // Evaluate scale-in
        else if self.policy.should_scale_in(&metrics) {
            state.scale_in_periods += 1;
            state.scale_out_periods = 0;

            if state.scale_in_periods >= self.policy.min_evaluation_periods {
                log::info!(
                    "Scale-in condition met for {} consecutive periods",
                    state.scale_in_periods
                );
                return ScalingDecision::ScaleIn;
            }
        }
        // Metrics are in the middle range
        else {
            state.reset_periods();
        }

        ScalingDecision::NoAction
    }

    /// Execute auto-scaling based on current evaluation
    ///
    /// This performs the actual scaling operation if needed.
    pub async fn execute(&self) -> Result<ScalingDecision> {
        let decision = self.evaluate();

        match decision {
            ScalingDecision::ScaleOut => {
                log::info!("Executing scale-out operation");
                self.perform_scale_out().await?;

                // Update last scaling time and reset periods
                let mut state = self.state.write();
                state.last_scaling_time = Some(Instant::now());
                state.reset_periods();

                Ok(ScalingDecision::ScaleOut)
            }
            ScalingDecision::ScaleIn => {
                log::info!("Executing scale-in operation");
                self.perform_scale_in().await?;

                // Update last scaling time and reset periods
                let mut state = self.state.write();
                state.last_scaling_time = Some(Instant::now());
                state.reset_periods();

                Ok(ScalingDecision::ScaleIn)
            }
            _ => Ok(decision),
        }
    }

    /// Perform scale-out by adding a new shard
    async fn perform_scale_out(&self) -> Result<()> {
        let shard_count = self.shard_manager.list_groups().len();
        let new_shard_id = format!("shard{}", shard_count + 1);
        let new_address = format!("127.0.0.1:{}", 5000 + shard_count + 1);

        log::info!("Auto-scaling: Adding shard {} at {}", new_shard_id, new_address);

        match self.scaling_manager.add_shard(new_shard_id.clone(), new_address, true).await {
            Ok(stats) => {
                log::info!(
                    "Auto-scaling: Successfully added shard {} (migrated {} keys)",
                    new_shard_id,
                    stats.keys_migrated
                );
                Ok(())
            }
            Err(e) => {
                log::error!("Auto-scaling: Failed to add shard {}: {}", new_shard_id, e);
                Err(e)
            }
        }
    }

    /// Perform scale-in by removing a shard
    async fn perform_scale_in(&self) -> Result<()> {
        let shards = self.shard_manager.list_groups();

        // Find the shard with lowest load
        let target_shard = {
            let state = self.state.read();
            shards
                .iter()
                .min_by_key(|shard_id| {
                    state.metrics.get(*shard_id).map(|m| m.qps).unwrap_or(u64::MAX)
                })
                .cloned()
        };

        if let Some(shard_id) = target_shard {
            log::info!("Auto-scaling: Removing shard {}", shard_id);

            match self.scaling_manager.remove_shard(&shard_id, true).await {
                Ok(stats) => {
                    log::info!(
                        "Auto-scaling: Successfully removed shard {} (migrated {} keys)",
                        shard_id,
                        stats.keys_migrated
                    );
                    Ok(())
                }
                Err(e) => {
                    log::error!("Auto-scaling: Failed to remove shard {}: {}", shard_id, e);
                    Err(e)
                }
            }
        } else {
            Err(Error::ClusterError("No shard to remove".to_string()))
        }
    }

    /// Get the current policy
    pub fn policy(&self) -> &ScalingPolicy {
        &self.policy
    }

    /// Get time until cooldown expires
    pub fn time_until_cooldown_expires(&self) -> Option<Duration> {
        let state = self.state.read();

        if let Some(last_time) = state.last_scaling_time {
            let elapsed = last_time.elapsed();
            if elapsed < self.policy.cooldown_duration {
                Some(self.policy.cooldown_duration - elapsed)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Clear all metrics
    pub fn clear_metrics(&self) {
        let mut state = self.state.write();
        state.metrics.clear();
        state.reset_periods();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::{Coordinator, ScalingConfig, ScalingManager};

    #[test]
    fn test_system_metrics_new() {
        let metrics = SystemMetrics::new();
        assert_eq!(metrics.cpu_percent, 0.0);
        assert_eq!(metrics.memory_percent, 0.0);
        assert_eq!(metrics.qps, 0);
        assert!(!metrics.is_stale(Duration::from_secs(60)));
    }

    #[test]
    fn test_system_metrics_storage_percent() {
        let mut metrics = SystemMetrics::new();
        metrics.storage_bytes = 500;
        metrics.storage_capacity_bytes = 1000;

        assert_eq!(metrics.storage_percent(), 50.0);
    }

    #[test]
    fn test_system_metrics_stale() {
        let mut metrics = SystemMetrics::new();
        metrics.timestamp = Instant::now() - Duration::from_secs(120);

        assert!(metrics.is_stale(Duration::from_secs(60)));
        assert!(!metrics.is_stale(Duration::from_secs(180)));
    }

    #[test]
    fn test_scaling_policy_default() {
        let policy = ScalingPolicy::default();
        assert_eq!(policy.name, "default");
        assert_eq!(policy.cpu_threshold.scale_out, 80.0);
        assert_eq!(policy.min_evaluation_periods, 3);
    }

    #[test]
    fn test_scaling_policy_conservative() {
        let policy = ScalingPolicy::conservative();
        assert_eq!(policy.name, "conservative");
        assert_eq!(policy.cpu_threshold.scale_out, 90.0);
        assert_eq!(policy.min_evaluation_periods, 5);
    }

    #[test]
    fn test_scaling_policy_aggressive() {
        let policy = ScalingPolicy::aggressive();
        assert_eq!(policy.name, "aggressive");
        assert_eq!(policy.cpu_threshold.scale_out, 70.0);
        assert_eq!(policy.min_evaluation_periods, 2);
    }

    #[test]
    fn test_policy_should_scale_out() {
        let policy = ScalingPolicy::default();
        let mut metrics = SystemMetrics::new();

        // Below threshold - no scale out
        metrics.cpu_percent = 50.0;
        assert!(!policy.should_scale_out(&metrics));

        // Above threshold - scale out
        metrics.cpu_percent = 85.0;
        assert!(policy.should_scale_out(&metrics));
    }

    #[test]
    fn test_policy_should_scale_in() {
        let policy = ScalingPolicy::default();
        let mut metrics = SystemMetrics::new();

        // Above threshold - no scale in
        metrics.cpu_percent = 50.0;
        metrics.memory_percent = 50.0;
        metrics.qps = 1000;
        metrics.storage_bytes = 500;
        metrics.storage_capacity_bytes = 1000;
        assert!(!policy.should_scale_in(&metrics));

        // Below all thresholds - scale in
        metrics.cpu_percent = 20.0;
        metrics.memory_percent = 20.0;
        metrics.qps = 100;
        metrics.storage_bytes = 100;
        assert!(policy.should_scale_in(&metrics));
    }

    #[test]
    fn test_autoscaler_creation() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_config = ScalingConfig::default();
        let scaling_manager =
            Arc::new(ScalingManager::new(coordinator, shard_manager.clone(), scaling_config));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        assert!(!autoscaler.is_enabled());
    }

    #[test]
    fn test_autoscaler_enable_disable() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_config = ScalingConfig::default();
        let scaling_manager =
            Arc::new(ScalingManager::new(coordinator, shard_manager.clone(), scaling_config));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        assert!(!autoscaler.is_enabled());

        autoscaler.enable();
        assert!(autoscaler.is_enabled());

        autoscaler.disable();
        assert!(!autoscaler.is_enabled());
    }

    #[test]
    fn test_update_and_get_metrics() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_config = ScalingConfig::default();
        let scaling_manager =
            Arc::new(ScalingManager::new(coordinator, shard_manager.clone(), scaling_config));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        let mut metrics = SystemMetrics::new();
        metrics.cpu_percent = 75.0;
        metrics.qps = 5000;

        autoscaler.update_metrics("shard1".to_string(), metrics.clone());

        let retrieved = autoscaler.get_metrics("shard1");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().cpu_percent, 75.0);
    }

    #[test]
    fn test_aggregate_metrics() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_config = ScalingConfig::default();
        let scaling_manager =
            Arc::new(ScalingManager::new(coordinator, shard_manager.clone(), scaling_config));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        // Add metrics for two shards
        let mut metrics1 = SystemMetrics::new();
        metrics1.cpu_percent = 60.0;
        metrics1.qps = 2000;

        let mut metrics2 = SystemMetrics::new();
        metrics2.cpu_percent = 80.0;
        metrics2.qps = 3000;

        autoscaler.update_metrics("shard1".to_string(), metrics1);
        autoscaler.update_metrics("shard2".to_string(), metrics2);

        let aggregate = autoscaler.get_aggregate_metrics();

        // Average CPU: (60 + 80) / 2 = 70
        assert_eq!(aggregate.cpu_percent, 70.0);
        // Total QPS: 2000 + 3000 = 5000
        assert_eq!(aggregate.qps, 5000);
    }

    #[test]
    fn test_evaluate_when_disabled() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_config = ScalingConfig::default();
        let scaling_manager =
            Arc::new(ScalingManager::new(coordinator, shard_manager.clone(), scaling_config));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        let decision = autoscaler.evaluate();
        assert_eq!(decision, ScalingDecision::NoAction);
    }

    #[test]
    fn test_evaluate_scale_out() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_config = ScalingConfig::default();
        let scaling_manager =
            Arc::new(ScalingManager::new(coordinator, shard_manager.clone(), scaling_config));

        let policy = ScalingPolicy { min_evaluation_periods: 2, ..ScalingPolicy::default() };
        let autoscaler = AutoScaler::new(scaling_manager, shard_manager, policy);

        autoscaler.enable();

        // Set high CPU metrics
        let mut metrics = SystemMetrics::new();
        metrics.cpu_percent = 90.0;
        autoscaler.update_metrics("shard1".to_string(), metrics);

        // First evaluation - not enough periods
        let decision1 = autoscaler.evaluate();
        assert_eq!(decision1, ScalingDecision::NoAction);

        // Second evaluation - enough periods
        let decision2 = autoscaler.evaluate();
        assert_eq!(decision2, ScalingDecision::ScaleOut);
    }

    #[test]
    fn test_clear_metrics() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_config = ScalingConfig::default();
        let scaling_manager =
            Arc::new(ScalingManager::new(coordinator, shard_manager.clone(), scaling_config));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        let metrics = SystemMetrics::new();
        autoscaler.update_metrics("shard1".to_string(), metrics);

        assert!(autoscaler.get_metrics("shard1").is_some());

        autoscaler.clear_metrics();

        assert!(autoscaler.get_metrics("shard1").is_none());
    }
}
