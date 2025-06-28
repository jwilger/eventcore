use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::errors::{EventStoreError, ProjectionError};
use crate::event_store::EventStore;
use crate::types::{EventId, StreamId};

/// Health status of a component
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    /// Component is healthy and functioning normally
    Healthy,
    /// Component is degraded but still operational
    Degraded,
    /// Component is unhealthy and not functioning properly
    Unhealthy,
}

/// Details about a health check result
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    /// The health status
    pub status: HealthStatus,
    /// Human-readable description of the health check
    pub message: String,
    /// When the health check was performed
    pub checked_at: Instant,
    /// How long the health check took to complete
    pub duration: Duration,
    /// Additional metadata about the health check
    pub metadata: HashMap<String, String>,
}

impl HealthCheckResult {
    /// Create a new healthy result
    pub fn healthy(message: impl Into<String>, duration: Duration) -> Self {
        Self {
            status: HealthStatus::Healthy,
            message: message.into(),
            checked_at: Instant::now(),
            duration,
            metadata: HashMap::new(),
        }
    }

    /// Create a new degraded result
    pub fn degraded(message: impl Into<String>, duration: Duration) -> Self {
        Self {
            status: HealthStatus::Degraded,
            message: message.into(),
            checked_at: Instant::now(),
            duration,
            metadata: HashMap::new(),
        }
    }

    /// Create a new unhealthy result
    pub fn unhealthy(message: impl Into<String>, duration: Duration) -> Self {
        Self {
            status: HealthStatus::Unhealthy,
            message: message.into(),
            checked_at: Instant::now(),
            duration,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the health check result
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Check if the result is healthy
    pub fn is_healthy(&self) -> bool {
        self.status == HealthStatus::Healthy
    }

    /// Check if the result is degraded
    pub fn is_degraded(&self) -> bool {
        self.status == HealthStatus::Degraded
    }

    /// Check if the result is unhealthy
    pub fn is_unhealthy(&self) -> bool {
        self.status == HealthStatus::Unhealthy
    }
}

/// Trait for performing health checks on system components
#[async_trait]
pub trait HealthCheck: Send + Sync {
    /// The name of this health check
    fn name(&self) -> &str;

    /// Perform the health check
    async fn check(&self) -> HealthCheckResult;

    /// Get the timeout for this health check
    fn timeout(&self) -> Duration {
        Duration::from_secs(5)
    }
}

/// Health check for event store connectivity
pub struct EventStoreHealthCheck<E>
where
    E: EventStore + Send + Sync,
{
    event_store: Arc<E>,
    test_stream_id: StreamId,
}

impl<E> EventStoreHealthCheck<E>
where
    E: EventStore + Send + Sync,
{
    /// Create a new event store health check
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(event_store: Arc<E>, test_stream_id: StreamId) -> Self {
        Self {
            event_store,
            test_stream_id,
        }
    }
}

#[async_trait]
impl<E> HealthCheck for EventStoreHealthCheck<E>
where
    E: EventStore + Send + Sync,
{
    fn name(&self) -> &'static str {
        "event_store_connectivity"
    }

    async fn check(&self) -> HealthCheckResult {
        let start = Instant::now();

        match self.event_store.stream_exists(&self.test_stream_id).await {
            Ok(_exists) => {
                let duration = start.elapsed();
                HealthCheckResult::healthy("Event store is accessible", duration)
                    .with_metadata("stream_id", self.test_stream_id.as_ref())
                    .with_metadata("response_time_ms", duration.as_millis().to_string())
            }
            Err(EventStoreError::ConnectionFailed(_)) => {
                let duration = start.elapsed();
                HealthCheckResult::unhealthy("Event store connection failed", duration)
                    .with_metadata("error_type", "connection")
            }
            Err(EventStoreError::Timeout(_)) => {
                let duration = start.elapsed();
                HealthCheckResult::degraded("Event store response timeout", duration)
                    .with_metadata("error_type", "timeout")
            }
            Err(err) => {
                let duration = start.elapsed();
                HealthCheckResult::degraded(format!("Event store error: {err}"), duration)
                    .with_metadata("error_type", "other")
            }
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }
}

/// Health check for projection status monitoring
pub struct ProjectionHealthCheck {
    projection_name: String,
    max_lag_threshold: Duration,
    last_processed_event: Arc<RwLock<Option<EventId>>>,
    last_processing_time: Arc<RwLock<Option<Instant>>>,
    error_count: Arc<RwLock<u64>>,
}

impl ProjectionHealthCheck {
    /// Create a new projection health check
    pub fn new(projection_name: impl Into<String>, max_lag_threshold: Duration) -> Self {
        Self {
            projection_name: projection_name.into(),
            max_lag_threshold,
            last_processed_event: Arc::new(RwLock::new(None)),
            last_processing_time: Arc::new(RwLock::new(None)),
            error_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Record that an event was processed
    pub fn record_event_processed(&self, event_id: EventId) {
        if let Ok(mut last_event) = self.last_processed_event.write() {
            *last_event = Some(event_id);
        }
        if let Ok(mut last_time) = self.last_processing_time.write() {
            *last_time = Some(Instant::now());
        }
    }

    /// Record a processing error
    pub fn record_error(&self, _error: &ProjectionError) {
        if let Ok(mut count) = self.error_count.write() {
            *count += 1;
        }
    }

    /// Get the current lag
    pub fn get_lag(&self) -> Option<Duration> {
        self.last_processing_time
            .read()
            .ok()
            .and_then(|time| time.map(|t| t.elapsed()))
    }

    /// Get the error count
    pub fn get_error_count(&self) -> u64 {
        self.error_count.read().map(|count| *count).unwrap_or(0)
    }
}

#[async_trait]
impl HealthCheck for ProjectionHealthCheck {
    fn name(&self) -> &str {
        &self.projection_name
    }

    async fn check(&self) -> HealthCheckResult {
        let start = Instant::now();

        let lag = self.get_lag();
        let error_count = self.get_error_count();

        let status = match lag {
            None => HealthStatus::Unhealthy,
            Some(lag_duration) if lag_duration > self.max_lag_threshold => {
                if error_count > 0 {
                    HealthStatus::Unhealthy
                } else {
                    HealthStatus::Degraded
                }
            }
            Some(_) if error_count > 10 => HealthStatus::Degraded,
            Some(_) => HealthStatus::Healthy,
        };

        let message = match (&status, lag) {
            (HealthStatus::Healthy, Some(lag_duration)) => {
                format!("Projection is healthy, lag: {}ms", lag_duration.as_millis())
            }
            (HealthStatus::Degraded, Some(lag_duration)) => {
                format!(
                    "Projection is degraded, lag: {}ms, errors: {}",
                    lag_duration.as_millis(),
                    error_count
                )
            }
            (HealthStatus::Unhealthy, Some(lag_duration)) => {
                format!(
                    "Projection is unhealthy, lag: {}ms, errors: {}",
                    lag_duration.as_millis(),
                    error_count
                )
            }
            (_, None) => "Projection has not processed any events".to_string(),
        };

        let duration = start.elapsed();

        let mut result = match status {
            HealthStatus::Healthy => HealthCheckResult::healthy(message, duration),
            HealthStatus::Degraded => HealthCheckResult::degraded(message, duration),
            HealthStatus::Unhealthy => HealthCheckResult::unhealthy(message, duration),
        };

        if let Some(lag_duration) = lag {
            result = result.with_metadata("lag_ms", lag_duration.as_millis().to_string());
        }
        result = result.with_metadata("error_count", error_count.to_string());

        result
    }
}

/// Health check for memory usage monitoring
pub struct MemoryUsageHealthCheck {
    name: String,
    warning_threshold_mb: u64,
    critical_threshold_mb: u64,
}

impl MemoryUsageHealthCheck {
    /// Create a new memory usage health check
    pub fn new(
        name: impl Into<String>,
        warning_threshold_mb: u64,
        critical_threshold_mb: u64,
    ) -> Self {
        Self {
            name: name.into(),
            warning_threshold_mb,
            critical_threshold_mb,
        }
    }

    /// Get current memory usage in MB (simplified implementation)
    #[allow(clippy::unused_self)]
    const fn get_memory_usage_mb(&self) -> u64 {
        // In a real implementation, this would use system APIs
        // For now, we'll return a mock value
        // TODO: Implement actual memory usage detection
        100
    }
}

#[async_trait]
impl HealthCheck for MemoryUsageHealthCheck {
    fn name(&self) -> &str {
        &self.name
    }

    async fn check(&self) -> HealthCheckResult {
        let start = Instant::now();
        let memory_usage_mb = self.get_memory_usage_mb();

        let (status, message) = if memory_usage_mb >= self.critical_threshold_mb {
            (
                HealthStatus::Unhealthy,
                format!("Memory usage critical: {memory_usage_mb}MB"),
            )
        } else if memory_usage_mb >= self.warning_threshold_mb {
            (
                HealthStatus::Degraded,
                format!("Memory usage high: {memory_usage_mb}MB"),
            )
        } else {
            (
                HealthStatus::Healthy,
                format!("Memory usage normal: {memory_usage_mb}MB"),
            )
        };

        let duration = start.elapsed();

        let result = match status {
            HealthStatus::Healthy => HealthCheckResult::healthy(message, duration),
            HealthStatus::Degraded => HealthCheckResult::degraded(message, duration),
            HealthStatus::Unhealthy => HealthCheckResult::unhealthy(message, duration),
        };

        result
            .with_metadata("memory_usage_mb", memory_usage_mb.to_string())
            .with_metadata(
                "warning_threshold_mb",
                self.warning_threshold_mb.to_string(),
            )
            .with_metadata(
                "critical_threshold_mb",
                self.critical_threshold_mb.to_string(),
            )
    }
}

/// Registry for managing multiple health checks
pub struct HealthCheckRegistry {
    checks: Arc<RwLock<HashMap<String, Arc<dyn HealthCheck>>>>,
}

impl HealthCheckRegistry {
    /// Create a new health check registry
    pub fn new() -> Self {
        Self {
            checks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a health check
    pub fn register(&self, check: Arc<dyn HealthCheck>) {
        if let Ok(mut checks) = self.checks.write() {
            checks.insert(check.name().to_string(), check);
        }
    }

    /// Unregister a health check
    pub fn unregister(&self, name: &str) {
        if let Ok(mut checks) = self.checks.write() {
            checks.remove(name);
        }
    }

    /// Run all health checks
    pub async fn check_all(&self) -> HashMap<String, HealthCheckResult> {
        let checks = {
            if let Ok(checks) = self.checks.read() {
                checks.clone()
            } else {
                return HashMap::new();
            }
        };

        let mut results = HashMap::new();

        for (name, check) in checks {
            let result = tokio::time::timeout(check.timeout(), check.check()).await;

            let health_result = result.unwrap_or_else(|_| {
                HealthCheckResult::unhealthy("Health check timed out", check.timeout())
            });

            results.insert(name, health_result);
        }

        results
    }

    /// Run a specific health check by name
    pub async fn check_one(&self, name: &str) -> Option<HealthCheckResult> {
        let check = {
            if let Ok(checks) = self.checks.read() {
                checks.get(name).cloned()
            } else {
                return None;
            }
        };

        if let Some(check) = check {
            let result = tokio::time::timeout(check.timeout(), check.check()).await;

            let health_result = result.unwrap_or_else(|_| {
                HealthCheckResult::unhealthy("Health check timed out", check.timeout())
            });

            Some(health_result)
        } else {
            None
        }
    }

    /// Get the overall health status across all checks
    pub async fn overall_health(&self) -> HealthStatus {
        let results = self.check_all().await;

        if results.is_empty() {
            return HealthStatus::Unhealthy;
        }

        let mut has_unhealthy = false;
        let mut has_degraded = false;

        for result in results.values() {
            match result.status {
                HealthStatus::Unhealthy => has_unhealthy = true,
                HealthStatus::Degraded => has_degraded = true,
                HealthStatus::Healthy => {}
            }
        }

        if has_unhealthy {
            HealthStatus::Unhealthy
        } else if has_degraded {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        }
    }

    /// Get the names of all registered health checks
    pub fn list_checks(&self) -> Vec<String> {
        self.checks
            .read()
            .map(|checks| checks.keys().cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for HealthCheckRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::harness::MockEventStore;
    use std::time::Duration;
    use tokio;

    #[test]
    fn health_status_equality() {
        assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_eq!(HealthStatus::Degraded, HealthStatus::Degraded);
        assert_eq!(HealthStatus::Unhealthy, HealthStatus::Unhealthy);

        assert_ne!(HealthStatus::Healthy, HealthStatus::Degraded);
        assert_ne!(HealthStatus::Healthy, HealthStatus::Unhealthy);
        assert_ne!(HealthStatus::Degraded, HealthStatus::Unhealthy);
    }

    #[test]
    fn health_check_result_creation() {
        let duration = Duration::from_millis(100);

        let healthy = HealthCheckResult::healthy("All good", duration);
        assert!(healthy.is_healthy());
        assert!(!healthy.is_degraded());
        assert!(!healthy.is_unhealthy());
        assert_eq!(healthy.message, "All good");
        assert_eq!(healthy.duration, duration);

        let degraded = HealthCheckResult::degraded("Some issues", duration);
        assert!(!degraded.is_healthy());
        assert!(degraded.is_degraded());
        assert!(!degraded.is_unhealthy());

        let unhealthy = HealthCheckResult::unhealthy("Critical error", duration);
        assert!(!unhealthy.is_healthy());
        assert!(!unhealthy.is_degraded());
        assert!(unhealthy.is_unhealthy());
    }

    #[test]
    fn health_check_result_with_metadata() {
        let result = HealthCheckResult::healthy("Test", Duration::from_millis(50))
            .with_metadata("key1", "value1")
            .with_metadata("key2", "value2");

        assert_eq!(result.metadata.get("key1"), Some(&"value1".to_string()));
        assert_eq!(result.metadata.get("key2"), Some(&"value2".to_string()));
    }

    #[tokio::test]
    async fn event_store_health_check_healthy() {
        let mock_store = MockEventStore::<crate::testing::fixtures::TestEvent>::new();
        let health_check = EventStoreHealthCheck::new(
            Arc::new(mock_store),
            StreamId::try_new("health-check-stream").unwrap(),
        );

        let result = health_check.check().await;
        assert!(result.is_healthy());
        assert_eq!(health_check.name(), "event_store_connectivity");
    }

    #[test]
    fn projection_health_check_creation() {
        let health_check = ProjectionHealthCheck::new("test_projection", Duration::from_secs(30));

        assert_eq!(health_check.name(), "test_projection");
        assert_eq!(health_check.get_error_count(), 0);
        assert!(health_check.get_lag().is_none());
    }

    #[tokio::test]
    async fn projection_health_check_with_events() {
        let health_check = ProjectionHealthCheck::new("test_projection", Duration::from_secs(30));

        let event_id = crate::types::EventId::new();
        health_check.record_event_processed(event_id);

        let result = health_check.check().await;
        assert!(result.is_healthy());
        assert!(health_check.get_lag().is_some());
    }

    #[test]
    fn memory_usage_health_check_creation() {
        let health_check = MemoryUsageHealthCheck::new("memory", 500, 1000);
        assert_eq!(health_check.name(), "memory");
        assert_eq!(health_check.warning_threshold_mb, 500);
        assert_eq!(health_check.critical_threshold_mb, 1000);
    }

    #[tokio::test]
    async fn memory_usage_health_check_normal() {
        let health_check = MemoryUsageHealthCheck::new("memory", 500, 1000);
        let result = health_check.check().await;

        // Since our mock returns 100MB, this should be healthy
        assert!(result.is_healthy());
        assert!(result.message.contains("Memory usage normal"));
    }

    #[test]
    fn health_check_registry_creation() {
        let registry = HealthCheckRegistry::new();
        assert!(registry.list_checks().is_empty());
    }

    #[tokio::test]
    async fn health_check_registry_register_and_check() {
        let registry = HealthCheckRegistry::new();
        let memory_check = Arc::new(MemoryUsageHealthCheck::new("memory", 500, 1000));

        registry.register(memory_check);

        let checks = registry.list_checks();
        assert_eq!(checks.len(), 1);
        assert!(checks.contains(&"memory".to_string()));

        let results = registry.check_all().await;
        assert_eq!(results.len(), 1);
        assert!(results.contains_key("memory"));

        let result = registry.check_one("memory").await;
        assert!(result.is_some());
        assert!(result.unwrap().is_healthy());

        let nonexistent = registry.check_one("nonexistent").await;
        assert!(nonexistent.is_none());
    }

    #[tokio::test]
    async fn health_check_registry_overall_health() {
        let registry = HealthCheckRegistry::new();

        // Empty registry should be unhealthy
        let status = registry.overall_health().await;
        assert_eq!(status, HealthStatus::Unhealthy);

        // Add a healthy check
        registry.register(Arc::new(MemoryUsageHealthCheck::new("memory", 500, 1000)));
        let status = registry.overall_health().await;
        assert_eq!(status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn health_check_registry_unregister() {
        let registry = HealthCheckRegistry::new();
        let memory_check = Arc::new(MemoryUsageHealthCheck::new("memory", 500, 1000));

        registry.register(memory_check);
        assert_eq!(registry.list_checks().len(), 1);

        registry.unregister("memory");
        assert_eq!(registry.list_checks().len(), 0);
    }
}
