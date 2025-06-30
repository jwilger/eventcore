//! Connection pool monitoring and metrics for `PostgreSQL` event store
//!
//! This module provides comprehensive monitoring capabilities for database
//! connection pools, including metrics collection and health tracking.

#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::cast_precision_loss)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument, warn};

use crate::{PoolStatus, PostgresError};

/// Connection pool metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMetrics {
    /// Current number of connections in the pool
    pub current_connections: u32,
    /// Number of idle connections
    pub idle_connections: u32,
    /// Number of active connections
    pub active_connections: u32,
    /// Total connections created since startup
    pub total_connections_created: u64,
    /// Total connections closed since startup
    pub total_connections_closed: u64,
    /// Average connection acquisition time
    pub avg_acquisition_time: Duration,
    /// Peak number of connections
    pub peak_connections: u32,
    /// Number of connection timeouts
    pub connection_timeouts: u64,
    /// Number of connection errors
    pub connection_errors: u64,
    /// Pool utilization percentage (0-100)
    pub utilization_percent: f64,
    /// Whether the pool is healthy
    pub is_healthy: bool,
    /// Last update timestamp
    pub last_updated: DateTime<Utc>,
}

/// Pool monitor for tracking connection metrics
#[derive(Debug)]
pub struct PoolMonitor {
    /// Metrics counters
    connections_created: AtomicU64,
    connections_closed: AtomicU64,
    connection_timeouts: AtomicU64,
    connection_errors: AtomicU64,
    peak_connections: AtomicU64,

    /// Timing metrics
    total_acquisition_time: AtomicU64,
    acquisition_count: AtomicU64,

    /// Configuration
    max_connections: u32,
    warning_threshold: f64, // Utilization percentage threshold for warnings
}

impl PoolMonitor {
    /// Create a new pool monitor
    pub fn new(max_connections: u32) -> Self {
        Self {
            connections_created: AtomicU64::new(0),
            connections_closed: AtomicU64::new(0),
            connection_timeouts: AtomicU64::new(0),
            connection_errors: AtomicU64::new(0),
            peak_connections: AtomicU64::new(0),
            total_acquisition_time: AtomicU64::new(0),
            acquisition_count: AtomicU64::new(0),
            max_connections,
            warning_threshold: 80.0, // Warn when 80% utilized
        }
    }

    /// Record a connection creation
    pub fn record_connection_created(&self) {
        self.connections_created.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a connection closure
    pub fn record_connection_closed(&self) {
        self.connections_closed.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a connection timeout
    pub fn record_connection_timeout(&self) {
        self.connection_timeouts.fetch_add(1, Ordering::Relaxed);
        debug!("Connection timeout recorded");
    }

    /// Record a connection error
    pub fn record_connection_error(&self) {
        self.connection_errors.fetch_add(1, Ordering::Relaxed);
        debug!("Connection error recorded");
    }

    /// Record connection acquisition time
    pub fn record_acquisition_time(&self, duration: Duration) {
        self.total_acquisition_time
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
        self.acquisition_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Update peak connections if current value is higher
    pub fn update_peak_connections(&self, current: u32) {
        let current_peak = self.peak_connections.load(Ordering::Relaxed) as u32;
        if current > current_peak {
            self.peak_connections
                .store(current as u64, Ordering::Relaxed);
        }
    }

    /// Get current metrics snapshot
    #[instrument(skip(self))]
    pub fn get_metrics(&self, pool_status: &PoolStatus) -> PoolMetrics {
        let current_connections = pool_status.size;
        let idle_connections = pool_status.idle;
        let active_connections = current_connections.saturating_sub(idle_connections);

        // Update peak if necessary
        self.update_peak_connections(current_connections);

        // Calculate average acquisition time
        let total_time = self.total_acquisition_time.load(Ordering::Relaxed);
        let count = self.acquisition_count.load(Ordering::Relaxed);
        let avg_acquisition_time = if count > 0 {
            Duration::from_micros(total_time / count)
        } else {
            Duration::ZERO
        };

        // Calculate utilization
        let utilization_percent = if self.max_connections > 0 {
            (current_connections as f64 / self.max_connections as f64) * 100.0
        } else {
            0.0
        };

        // Determine health status
        let is_healthy = self.is_pool_healthy(pool_status, utilization_percent);

        // Log warnings if necessary
        if utilization_percent > self.warning_threshold {
            warn!(
                "Pool utilization high: {:.1}% ({}/{})",
                utilization_percent, current_connections, self.max_connections
            );
        }

        PoolMetrics {
            current_connections,
            idle_connections,
            active_connections,
            total_connections_created: self.connections_created.load(Ordering::Relaxed),
            total_connections_closed: self.connections_closed.load(Ordering::Relaxed),
            avg_acquisition_time,
            peak_connections: self.peak_connections.load(Ordering::Relaxed) as u32,
            connection_timeouts: self.connection_timeouts.load(Ordering::Relaxed),
            connection_errors: self.connection_errors.load(Ordering::Relaxed),
            utilization_percent,
            is_healthy,
            last_updated: Utc::now(),
        }
    }

    /// Determine if the pool is healthy based on current metrics
    fn is_pool_healthy(&self, pool_status: &PoolStatus, utilization_percent: f64) -> bool {
        // Pool is unhealthy if:
        // 1. It's closed
        // 2. Utilization is too high (>95%)
        // 3. No idle connections and high utilization
        // 4. Recent errors exceed threshold

        if pool_status.is_closed {
            return false;
        }

        if utilization_percent > 95.0 {
            return false;
        }

        if pool_status.idle == 0 && utilization_percent > 80.0 {
            return false;
        }

        // Check recent error rate (simple heuristic)
        let recent_errors = self.connection_errors.load(Ordering::Relaxed);
        let recent_acquisitions = self.acquisition_count.load(Ordering::Relaxed);

        if recent_acquisitions > 0 {
            let error_rate = recent_errors as f64 / recent_acquisitions as f64;
            if error_rate > 0.1 {
                // More than 10% error rate
                return false;
            }
        }

        true
    }

    /// Reset all metrics (useful for testing or periodic resets)
    pub fn reset_metrics(&self) {
        self.connections_created.store(0, Ordering::Relaxed);
        self.connections_closed.store(0, Ordering::Relaxed);
        self.connection_timeouts.store(0, Ordering::Relaxed);
        self.connection_errors.store(0, Ordering::Relaxed);
        self.peak_connections.store(0, Ordering::Relaxed);
        self.total_acquisition_time.store(0, Ordering::Relaxed);
        self.acquisition_count.store(0, Ordering::Relaxed);
        debug!("Pool metrics reset");
    }

    /// Get metrics as JSON string for external monitoring systems
    pub fn get_metrics_json(&self, pool_status: &PoolStatus) -> Result<String, PostgresError> {
        let metrics = self.get_metrics(pool_status);
        serde_json::to_string(&metrics).map_err(PostgresError::Serialization)
    }
}

/// Utility for measuring connection acquisition time
pub struct AcquisitionTimer {
    start: Instant,
    monitor: Arc<PoolMonitor>,
}

impl AcquisitionTimer {
    /// Start a new acquisition timer
    pub fn new(monitor: Arc<PoolMonitor>) -> Self {
        Self {
            start: Instant::now(),
            monitor,
        }
    }

    /// Complete the timer and record the duration
    pub fn complete(self) {
        let duration = self.start.elapsed();
        self.monitor.record_acquisition_time(duration);
    }
}

/// Background task for periodic pool monitoring
pub struct PoolMonitoringTask {
    monitor: Arc<PoolMonitor>,
    interval: Duration,
    stop_signal: tokio::sync::watch::Receiver<bool>,
}

impl PoolMonitoringTask {
    /// Create a new monitoring task
    pub fn new(
        monitor: Arc<PoolMonitor>,
        interval: Duration,
        stop_signal: tokio::sync::watch::Receiver<bool>,
    ) -> Self {
        Self {
            monitor,
            interval,
            stop_signal,
        }
    }

    /// Run the monitoring task
    pub async fn run<F>(mut self, mut get_pool_status: F)
    where
        F: FnMut() -> PoolStatus + Send + 'static,
    {
        let mut interval_timer = tokio::time::interval(self.interval);

        loop {
            tokio::select! {
                _ = interval_timer.tick() => {
                    let pool_status = get_pool_status();
                    let metrics = self.monitor.get_metrics(&pool_status);

                    debug!("Pool metrics: {:?}", metrics);

                    // Log warnings for concerning metrics
                    if !metrics.is_healthy {
                        warn!("Pool health check failed: {:?}", metrics);
                    }

                    if metrics.connection_errors > 0 {
                        warn!("Connection errors detected: {}", metrics.connection_errors);
                    }

                    if metrics.connection_timeouts > 0 {
                        warn!("Connection timeouts detected: {}", metrics.connection_timeouts);
                    }
                }
                _ = self.stop_signal.changed() => {
                    if *self.stop_signal.borrow() {
                        debug!("Pool monitoring task stopped");
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_monitor_creation() {
        let monitor = PoolMonitor::new(10);

        // Initial values should be zero
        assert_eq!(monitor.connections_created.load(Ordering::Relaxed), 0);
        assert_eq!(monitor.connections_closed.load(Ordering::Relaxed), 0);
        assert_eq!(monitor.max_connections, 10);
    }

    #[test]
    fn test_pool_monitor_record_operations() {
        let monitor = PoolMonitor::new(10);

        // Record some operations
        monitor.record_connection_created();
        monitor.record_connection_created();
        monitor.record_connection_closed();
        monitor.record_connection_timeout();
        monitor.record_connection_error();

        // Verify counters
        assert_eq!(monitor.connections_created.load(Ordering::Relaxed), 2);
        assert_eq!(monitor.connections_closed.load(Ordering::Relaxed), 1);
        assert_eq!(monitor.connection_timeouts.load(Ordering::Relaxed), 1);
        assert_eq!(monitor.connection_errors.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_pool_monitor_metrics() {
        let monitor = PoolMonitor::new(10);

        // Set up some data
        monitor.record_connection_created();
        monitor.record_connection_created();
        monitor.record_acquisition_time(Duration::from_millis(50));
        monitor.record_acquisition_time(Duration::from_millis(100));

        let pool_status = PoolStatus {
            size: 5,
            idle: 2,
            is_closed: false,
        };

        let metrics = monitor.get_metrics(&pool_status);

        assert_eq!(metrics.current_connections, 5);
        assert_eq!(metrics.idle_connections, 2);
        assert_eq!(metrics.active_connections, 3);
        assert_eq!(metrics.total_connections_created, 2);
        assert_eq!(metrics.avg_acquisition_time, Duration::from_millis(75)); // (50 + 100) / 2
        assert!((metrics.utilization_percent - 50.0).abs() < f64::EPSILON); // 5 / 10 * 100
        assert!(metrics.is_healthy);
    }

    #[test]
    fn test_pool_health_assessment() {
        let monitor = PoolMonitor::new(10);

        // Healthy pool
        let healthy_status = PoolStatus {
            size: 5,
            idle: 2,
            is_closed: false,
        };
        assert!(monitor.is_pool_healthy(&healthy_status, 50.0));

        // Closed pool (unhealthy)
        let closed_status = PoolStatus {
            size: 5,
            idle: 2,
            is_closed: true,
        };
        assert!(!monitor.is_pool_healthy(&closed_status, 50.0));

        // Over-utilized pool (unhealthy)
        let overutil_status = PoolStatus {
            size: 10,
            idle: 0,
            is_closed: false,
        };
        assert!(!monitor.is_pool_healthy(&overutil_status, 96.0));
    }

    #[test]
    fn test_acquisition_timer() {
        let monitor = Arc::new(PoolMonitor::new(10));
        let timer = AcquisitionTimer::new(Arc::clone(&monitor));

        // Add a small delay to ensure non-zero timing
        std::thread::sleep(std::time::Duration::from_millis(1));

        // Complete the timer
        timer.complete();

        // Should have recorded at least one acquisition
        assert_eq!(monitor.acquisition_count.load(Ordering::Relaxed), 1);
        assert!(monitor.total_acquisition_time.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn test_metrics_reset() {
        let monitor = PoolMonitor::new(10);

        // Add some data
        monitor.record_connection_created();
        monitor.record_connection_error();
        monitor.record_acquisition_time(Duration::from_millis(100));

        // Verify data exists
        assert!(monitor.connections_created.load(Ordering::Relaxed) > 0);
        assert!(monitor.connection_errors.load(Ordering::Relaxed) > 0);
        assert!(monitor.acquisition_count.load(Ordering::Relaxed) > 0);

        // Reset and verify
        monitor.reset_metrics();
        assert_eq!(monitor.connections_created.load(Ordering::Relaxed), 0);
        assert_eq!(monitor.connection_errors.load(Ordering::Relaxed), 0);
        assert_eq!(monitor.acquisition_count.load(Ordering::Relaxed), 0);
    }
}
