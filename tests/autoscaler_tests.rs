//! Integration tests for auto-scaling functionality
//!
//! These tests validate the AutoScaler's ability to:
//! - Monitor metrics across shards
//! - Evaluate scaling decisions based on policies
//! - Automatically trigger scaling operations
//! - Respect cooldown periods

#[cfg(feature = "cluster")]
mod autoscaler_integration_tests {
    use aidb::cluster::{
        AutoScaler, Coordinator, ScalingDecision, ScalingManager, ScalingPolicy,
        ShardGroupManager, SystemMetrics,
    };
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn test_autoscaler_creation() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        assert!(!autoscaler.is_enabled());
        assert_eq!(autoscaler.policy().name, "default");
    }

    #[test]
    fn test_enable_disable() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        // Initially disabled
        assert!(!autoscaler.is_enabled());

        // Enable
        autoscaler.enable();
        assert!(autoscaler.is_enabled());

        // Disable
        autoscaler.disable();
        assert!(!autoscaler.is_enabled());
    }

    #[test]
    fn test_update_and_retrieve_metrics() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        let mut metrics = SystemMetrics::new();
        metrics.cpu_percent = 75.0;
        metrics.memory_percent = 60.0;
        metrics.qps = 5000;

        autoscaler.update_metrics("shard1".to_string(), metrics);

        let retrieved = autoscaler.get_metrics("shard1");
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.cpu_percent, 75.0);
        assert_eq!(retrieved.memory_percent, 60.0);
        assert_eq!(retrieved.qps, 5000);
    }

    #[test]
    fn test_aggregate_metrics_single_shard() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        let mut metrics = SystemMetrics::new();
        metrics.cpu_percent = 80.0;
        metrics.qps = 3000;

        autoscaler.update_metrics("shard1".to_string(), metrics);

        let aggregate = autoscaler.get_aggregate_metrics();
        assert_eq!(aggregate.cpu_percent, 80.0);
        assert_eq!(aggregate.qps, 3000);
    }

    #[test]
    fn test_aggregate_metrics_multiple_shards() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        // Add metrics for multiple shards
        let mut metrics1 = SystemMetrics::new();
        metrics1.cpu_percent = 60.0;
        metrics1.memory_percent = 50.0;
        metrics1.qps = 2000;

        let mut metrics2 = SystemMetrics::new();
        metrics2.cpu_percent = 80.0;
        metrics2.memory_percent = 70.0;
        metrics2.qps = 3000;

        let mut metrics3 = SystemMetrics::new();
        metrics3.cpu_percent = 70.0;
        metrics3.memory_percent = 60.0;
        metrics3.qps = 4000;

        autoscaler.update_metrics("shard1".to_string(), metrics1);
        autoscaler.update_metrics("shard2".to_string(), metrics2);
        autoscaler.update_metrics("shard3".to_string(), metrics3);

        let aggregate = autoscaler.get_aggregate_metrics();

        // Average CPU: (60 + 80 + 70) / 3 = 70
        assert_eq!(aggregate.cpu_percent, 70.0);
        // Average memory: (50 + 70 + 60) / 3 = 60
        assert_eq!(aggregate.memory_percent, 60.0);
        // Total QPS: 2000 + 3000 + 4000 = 9000
        assert_eq!(aggregate.qps, 9000);
    }

    #[test]
    fn test_evaluate_when_disabled() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        // Add high metrics
        let mut metrics = SystemMetrics::new();
        metrics.cpu_percent = 95.0;
        autoscaler.update_metrics("shard1".to_string(), metrics);

        // Evaluation should return NoAction when disabled
        let decision = autoscaler.evaluate();
        assert_eq!(decision, ScalingDecision::NoAction);
    }

    #[test]
    fn test_evaluate_scale_out_condition() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        // Use aggressive policy with fewer evaluation periods
        let policy = ScalingPolicy { min_evaluation_periods: 2, ..ScalingPolicy::aggressive() };
        let autoscaler = AutoScaler::new(scaling_manager, shard_manager, policy);

        autoscaler.enable();

        // Add high CPU metrics
        let mut metrics = SystemMetrics::new();
        metrics.cpu_percent = 85.0;
        autoscaler.update_metrics("shard1".to_string(), metrics);

        // First evaluation - not enough periods yet
        let decision1 = autoscaler.evaluate();
        assert_eq!(decision1, ScalingDecision::NoAction);

        // Second evaluation - should trigger scale-out
        let decision2 = autoscaler.evaluate();
        assert_eq!(decision2, ScalingDecision::ScaleOut);
    }

    #[test]
    fn test_evaluate_scale_in_condition() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        // Use aggressive policy with fewer evaluation periods
        let policy = ScalingPolicy { min_evaluation_periods: 2, ..ScalingPolicy::aggressive() };
        let autoscaler = AutoScaler::new(scaling_manager, shard_manager, policy);

        autoscaler.enable();

        // Add low metrics
        let mut metrics = SystemMetrics::new();
        metrics.cpu_percent = 20.0;
        metrics.memory_percent = 20.0;
        metrics.qps = 100;
        metrics.storage_bytes = 100;
        metrics.storage_capacity_bytes = 1000;
        autoscaler.update_metrics("shard1".to_string(), metrics);

        // First evaluation - not enough periods yet
        let decision1 = autoscaler.evaluate();
        assert_eq!(decision1, ScalingDecision::NoAction);

        // Second evaluation - should trigger scale-in
        let decision2 = autoscaler.evaluate();
        assert_eq!(decision2, ScalingDecision::ScaleIn);
    }

    #[test]
    fn test_evaluation_periods_reset_on_mixed_conditions() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        let policy = ScalingPolicy { min_evaluation_periods: 3, ..ScalingPolicy::default() };
        let autoscaler = AutoScaler::new(scaling_manager, shard_manager, policy);

        autoscaler.enable();

        // High metrics
        let mut high_metrics = SystemMetrics::new();
        high_metrics.cpu_percent = 90.0;

        // Low metrics
        let mut low_metrics = SystemMetrics::new();
        low_metrics.cpu_percent = 20.0;
        low_metrics.memory_percent = 20.0;
        low_metrics.qps = 100;

        // Normal metrics
        let mut normal_metrics = SystemMetrics::new();
        normal_metrics.cpu_percent = 50.0;
        normal_metrics.memory_percent = 50.0;
        normal_metrics.qps = 5000;

        // Two periods of high metrics
        autoscaler.update_metrics("shard1".to_string(), high_metrics.clone());
        let _d1 = autoscaler.evaluate();
        let _d2 = autoscaler.evaluate();

        // Switch to normal - should reset periods
        autoscaler.update_metrics("shard1".to_string(), normal_metrics);
        let decision3 = autoscaler.evaluate();
        assert_eq!(decision3, ScalingDecision::NoAction);

        // Now we need 3 more periods for scale-in
        autoscaler.update_metrics("shard1".to_string(), low_metrics.clone());
        let _d4 = autoscaler.evaluate();
        let _d5 = autoscaler.evaluate();
        let decision6 = autoscaler.evaluate();
        assert_eq!(decision6, ScalingDecision::ScaleIn);
    }

    #[test]
    fn test_different_policy_types() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator.clone(), shard_manager.clone()));

        // Test default policy
        let default_autoscaler = AutoScaler::new(
            scaling_manager.clone(),
            shard_manager.clone(),
            ScalingPolicy::default(),
        );
        assert_eq!(default_autoscaler.policy().name, "default");
        assert_eq!(default_autoscaler.policy().cpu_threshold.scale_out, 80.0);

        // Test conservative policy
        let conservative_autoscaler = AutoScaler::new(
            scaling_manager.clone(),
            shard_manager.clone(),
            ScalingPolicy::conservative(),
        );
        assert_eq!(conservative_autoscaler.policy().name, "conservative");
        assert_eq!(conservative_autoscaler.policy().cpu_threshold.scale_out, 90.0);

        // Test aggressive policy
        let aggressive_autoscaler =
            AutoScaler::new(scaling_manager, shard_manager, ScalingPolicy::aggressive());
        assert_eq!(aggressive_autoscaler.policy().name, "aggressive");
        assert_eq!(aggressive_autoscaler.policy().cpu_threshold.scale_out, 70.0);
    }

    #[test]
    fn test_clear_metrics() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        // Add some metrics
        let metrics = SystemMetrics::new();
        autoscaler.update_metrics("shard1".to_string(), metrics.clone());
        autoscaler.update_metrics("shard2".to_string(), metrics);

        assert!(autoscaler.get_metrics("shard1").is_some());
        assert!(autoscaler.get_metrics("shard2").is_some());

        // Clear metrics
        autoscaler.clear_metrics();

        assert!(autoscaler.get_metrics("shard1").is_none());
        assert!(autoscaler.get_metrics("shard2").is_none());
    }

    #[test]
    fn test_cooldown_period() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        let policy = ScalingPolicy {
            cooldown_duration: Duration::from_secs(60),
            min_evaluation_periods: 1,
            ..ScalingPolicy::default()
        };
        let autoscaler = AutoScaler::new(scaling_manager, shard_manager, policy);

        // Initially no cooldown
        assert!(autoscaler.time_until_cooldown_expires().is_none());

        autoscaler.enable();

        // Add high metrics
        let mut metrics = SystemMetrics::new();
        metrics.cpu_percent = 90.0;
        autoscaler.update_metrics("shard1".to_string(), metrics);

        // First evaluation should trigger scale-out
        let decision = autoscaler.evaluate();
        assert_eq!(decision, ScalingDecision::ScaleOut);

        // Note: In a real scenario, execute() would be called which sets the cooldown
        // For this test, we just verify the cooldown check logic works
    }

    #[test]
    fn test_metrics_staleness() {
        let mut metrics = SystemMetrics::new();

        // Fresh metrics should not be stale
        assert!(!metrics.is_stale(Duration::from_secs(60)));

        // Make metrics old
        metrics.timestamp = std::time::Instant::now() - Duration::from_secs(120);

        // Should be stale with 60 second threshold
        assert!(metrics.is_stale(Duration::from_secs(60)));
        // Should not be stale with 180 second threshold
        assert!(!metrics.is_stale(Duration::from_secs(180)));
    }

    #[test]
    fn test_storage_percent_calculation() {
        let mut metrics = SystemMetrics::new();

        // Zero capacity
        metrics.storage_bytes = 100;
        metrics.storage_capacity_bytes = 0;
        assert_eq!(metrics.storage_percent(), 0.0);

        // 50% usage
        metrics.storage_bytes = 500;
        metrics.storage_capacity_bytes = 1000;
        assert_eq!(metrics.storage_percent(), 50.0);

        // 75% usage
        metrics.storage_bytes = 750;
        metrics.storage_capacity_bytes = 1000;
        assert_eq!(metrics.storage_percent(), 75.0);

        // 100% usage
        metrics.storage_bytes = 1000;
        metrics.storage_capacity_bytes = 1000;
        assert_eq!(metrics.storage_percent(), 100.0);
    }

    #[tokio::test]
    async fn test_execute_when_disabled() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);

        // Execute when disabled should return NoAction
        let result = autoscaler.execute().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ScalingDecision::NoAction);
    }

    #[tokio::test]
    async fn test_execute_no_action_needed() {
        let coordinator = Arc::new(Coordinator::new(100));
        let shard_manager = Arc::new(ShardGroupManager::new());
        let scaling_manager =
            Arc::new(ScalingManager::with_defaults(coordinator, shard_manager.clone()));

        let autoscaler = AutoScaler::with_defaults(scaling_manager, shard_manager);
        autoscaler.enable();

        // Add normal metrics
        let mut metrics = SystemMetrics::new();
        metrics.cpu_percent = 50.0;
        metrics.memory_percent = 50.0;
        metrics.qps = 5000;
        autoscaler.update_metrics("shard1".to_string(), metrics);

        // Execute should return NoAction
        let result = autoscaler.execute().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ScalingDecision::NoAction);
    }
}
