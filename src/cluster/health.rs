//! Health checking and failure detection for distributed shards
//!
//! This module provides periodic health checks for registered shards
//! and automatic failure detection and recovery.

use super::coordinator::Coordinator;
use super::rpc::proto::{storage_client::StorageClient, HealthCheckRequest};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

/// Configuration for health checker
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// Interval between health checks
    pub check_interval: Duration,
    /// Timeout for each health check
    pub timeout: Duration,
    /// Number of consecutive failures before marking unhealthy
    pub failure_threshold: u32,
    /// Number of consecutive successes before marking healthy
    pub success_threshold: u32,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
            failure_threshold: 3,
            success_threshold: 2,
        }
    }
}

/// Health checker for monitoring shard health
pub struct HealthChecker {
    /// Reference to the coordinator
    coordinator: Arc<Coordinator>,
    /// Health check configuration
    config: HealthCheckConfig,
    /// Running state
    running: Arc<parking_lot::RwLock<bool>>,
}

impl HealthChecker {
    /// Create a new health checker
    ///
    /// # Arguments
    /// * `coordinator` - The coordinator to monitor
    /// * `config` - Health check configuration
    pub fn new(coordinator: Arc<Coordinator>, config: HealthCheckConfig) -> Self {
        Self { coordinator, config, running: Arc::new(parking_lot::RwLock::new(false)) }
    }

    /// Start the health checker
    ///
    /// This will spawn a background task that periodically checks shard health
    pub fn start(&self) {
        {
            let mut running = self.running.write();
            if *running {
                log::warn!("Health checker already running");
                return;
            }
            *running = true;
        }

        let coordinator = self.coordinator.clone();
        let config = self.config.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            let mut check_interval = interval(config.check_interval);
            let mut failure_counts = std::collections::HashMap::new();
            let mut success_counts = std::collections::HashMap::new();

            loop {
                check_interval.tick().await;

                if !*running.read() {
                    log::info!("Health checker stopped");
                    break;
                }

                let shards = coordinator.list_shards();

                for shard_info in shards {
                    let shard_id = shard_info.id.clone();
                    let address = format!("http://{}", shard_info.address);

                    // Perform health check
                    let is_healthy = Self::check_shard_health(&address, config.timeout).await;

                    if is_healthy {
                        // Reset failure count
                        failure_counts.insert(shard_id.clone(), 0);

                        // Increment success count
                        let success_count = success_counts.entry(shard_id.clone()).or_insert(0);
                        *success_count += 1;

                        // If enough successes and currently marked unhealthy, mark as healthy
                        if *success_count >= config.success_threshold && !shard_info.healthy {
                            coordinator.mark_healthy(&shard_id);
                            *success_count = 0; // Reset count
                            log::info!("Shard {} recovered and marked healthy", shard_id);
                        }
                    } else {
                        // Reset success count
                        success_counts.insert(shard_id.clone(), 0);

                        // Increment failure count
                        let failure_count = failure_counts.entry(shard_id.clone()).or_insert(0);
                        *failure_count += 1;

                        log::warn!(
                            "Health check failed for shard {} (failures: {})",
                            shard_id,
                            failure_count
                        );

                        // If enough failures and currently marked healthy, mark as unhealthy
                        if *failure_count >= config.failure_threshold && shard_info.healthy {
                            coordinator.mark_unhealthy(&shard_id);
                            log::error!(
                                "Shard {} marked unhealthy after {} consecutive failures",
                                shard_id,
                                failure_count
                            );

                            // Optionally auto-remove unhealthy shard
                            // coordinator.unregister_shard(&shard_id);
                        }
                    }
                }
            }
        });

        log::info!("Health checker started with interval: {:?}", self.config.check_interval);
    }

    /// Stop the health checker
    pub fn stop(&self) {
        let mut running = self.running.write();
        *running = false;
        log::info!("Health checker stop requested");
    }

    /// Check if a shard is healthy
    ///
    /// # Arguments
    /// * `address` - Full address of the shard (e.g., "http://127.0.0.1:50051")
    /// * `timeout` - Timeout for the health check
    ///
    /// # Returns
    /// `true` if the shard is healthy, `false` otherwise
    async fn check_shard_health(address: &str, timeout: Duration) -> bool {
        let connect_result =
            tokio::time::timeout(timeout, StorageClient::connect(address.to_string())).await;

        let mut client = match connect_result {
            Ok(Ok(client)) => client,
            Ok(Err(e)) => {
                log::debug!("Failed to connect to {}: {}", address, e);
                return false;
            }
            Err(_) => {
                log::debug!("Connection timeout to {}", address);
                return false;
            }
        };

        // Perform health check RPC
        let request = tonic::Request::new(HealthCheckRequest { service: "aidb".to_string() });

        let check_result = tokio::time::timeout(timeout, client.health_check(request)).await;

        match check_result {
            Ok(Ok(response)) => {
                let status = response.into_inner().status;
                // status == 1 means SERVING
                status == 1
            }
            Ok(Err(e)) => {
                log::debug!("Health check RPC failed for {}: {}", address, e);
                false
            }
            Err(_) => {
                log::debug!("Health check timeout for {}", address);
                false
            }
        }
    }

    /// Check if the health checker is running
    pub fn is_running(&self) -> bool {
        *self.running.read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::Coordinator;

    #[test]
    fn test_health_check_config_default() {
        let config = HealthCheckConfig::default();
        assert_eq!(config.check_interval, Duration::from_secs(10));
        assert_eq!(config.timeout, Duration::from_secs(5));
        assert_eq!(config.failure_threshold, 3);
        assert_eq!(config.success_threshold, 2);
    }

    #[test]
    fn test_health_checker_creation() {
        let coordinator = Arc::new(Coordinator::new(150));
        let config = HealthCheckConfig::default();
        let checker = HealthChecker::new(coordinator, config);

        assert!(!checker.is_running());
    }

    #[tokio::test]
    async fn test_health_checker_start_stop() {
        let coordinator = Arc::new(Coordinator::new(150));
        let config = HealthCheckConfig {
            check_interval: Duration::from_millis(100),
            timeout: Duration::from_secs(1),
            failure_threshold: 2,
            success_threshold: 1,
        };
        let checker = HealthChecker::new(coordinator, config);

        checker.start();
        assert!(checker.is_running());

        // Wait a bit for the task to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        checker.stop();

        // Wait for the task to stop
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!checker.is_running());
    }

    #[tokio::test]
    async fn test_check_shard_health_invalid_address() {
        let result = HealthChecker::check_shard_health(
            "http://invalid-address-that-does-not-exist:99999",
            Duration::from_secs(1),
        )
        .await;

        assert!(!result);
    }
}
