use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::errors::{CommandError, EventStoreError};
use crate::types::{EventId, StreamId};
use tracing::{debug, info, warn};

/// Core metric types for observability.
///
/// Provides a unified interface for different types of metrics used throughout
/// the `EventCore` library for monitoring and observability.
#[derive(Debug, Clone)]
pub enum MetricValue {
    /// A monotonically increasing counter value
    Counter(u64),
    /// A gauge value that can increase or decrease
    Gauge(f64),
    /// A timer measurement representing a duration
    Timer(Duration),
}

/// Thread-safe counter metric for tracking incrementing values.
///
/// Provides atomic operations for incrementing values and is suitable
/// for tracking counts like command executions, events processed, etc.
#[derive(Debug)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    /// Creates a new counter initialized to zero.
    pub const fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    /// Increments the counter by 1.
    pub fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Increments the counter by the specified amount.
    pub fn increment_by(&self, amount: u64) {
        self.value.fetch_add(amount, Ordering::Relaxed);
    }

    /// Gets the current counter value.
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Resets the counter to zero.
    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }
}

impl Default for Counter {
    fn default() -> Self {
        Self::new()
    }
}

/// Gauge metric for current values that can go up or down
#[derive(Debug)]
pub struct Gauge {
    value: Arc<RwLock<f64>>,
}

impl Gauge {
    pub fn new() -> Self {
        Self {
            value: Arc::new(RwLock::new(0.0)),
        }
    }

    pub fn set(&self, value: f64) {
        if let Ok(mut v) = self.value.write() {
            *v = value;
        }
    }

    pub fn increment(&self) {
        self.increment_by(1.0);
    }

    pub fn increment_by(&self, amount: f64) {
        if let Ok(mut v) = self.value.write() {
            *v += amount;
        }
    }

    pub fn decrement(&self) {
        self.decrement_by(1.0);
    }

    pub fn decrement_by(&self, amount: f64) {
        if let Ok(mut v) = self.value.write() {
            *v -= amount;
        }
    }

    pub fn get(&self) -> f64 {
        self.value.read().map(|v| *v).unwrap_or(0.0)
    }
}

impl Default for Gauge {
    fn default() -> Self {
        Self::new()
    }
}

/// Timer metric for tracking durations
#[derive(Debug)]
pub struct Timer {
    samples: Arc<RwLock<Vec<Duration>>>,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            samples: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn record(&self, duration: Duration) {
        if let Ok(mut samples) = self.samples.write() {
            samples.push(duration);
            // Keep only the last 1000 samples to prevent memory growth
            if samples.len() > 1000 {
                let drain_count = samples.len() - 1000;
                samples.drain(0..drain_count);
            }
        }
    }

    pub fn time<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let start = Instant::now();
        let result = f();
        self.record(start.elapsed());
        result
    }

    pub async fn time_async<F, Fut, R>(&self, f: F) -> R
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = R>,
    {
        let start = Instant::now();
        let result = f().await;
        self.record(start.elapsed());
        result
    }

    pub fn get_samples(&self) -> Vec<Duration> {
        self.samples.read().map(|v| v.clone()).unwrap_or_default()
    }

    pub fn mean(&self) -> Option<Duration> {
        let samples = self.get_samples();
        if samples.is_empty() {
            return None;
        }

        let total: Duration = samples.iter().sum();
        Some(total / u32::try_from(samples.len()).unwrap_or(1))
    }

    pub fn percentile(&self, p: f64) -> Option<Duration> {
        let mut samples = self.get_samples();
        if samples.is_empty() {
            return None;
        }

        samples.sort();
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let index = ((samples.len() as f64 - 1.0) * p / 100.0).round() as usize;
        samples.get(index).copied()
    }

    pub fn p95(&self) -> Option<Duration> {
        self.percentile(95.0)
    }

    pub fn p99(&self) -> Option<Duration> {
        self.percentile(99.0)
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

/// Command execution metrics
#[derive(Debug)]
pub struct CommandMetrics {
    pub commands_executed: Counter,
    pub commands_succeeded: Counter,
    pub commands_failed: Counter,
    pub command_duration: Timer,
    pub concurrent_commands: Gauge,
    pub commands_by_type: Arc<RwLock<HashMap<String, Counter>>>,
    pub errors_by_type: Arc<RwLock<HashMap<String, Counter>>>,
}

impl CommandMetrics {
    pub fn new() -> Self {
        Self {
            commands_executed: Counter::new(),
            commands_succeeded: Counter::new(),
            commands_failed: Counter::new(),
            command_duration: Timer::new(),
            concurrent_commands: Gauge::new(),
            commands_by_type: Arc::new(RwLock::new(HashMap::new())),
            errors_by_type: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn record_command_start(&self, command_type: &str) {
        self.commands_executed.increment();
        self.concurrent_commands.increment();

        if let Ok(mut counters) = self.commands_by_type.write() {
            counters
                .entry(command_type.to_string())
                .or_insert_with(Counter::new)
                .increment();
        }
    }

    pub fn record_command_success(&self, duration: Duration) {
        self.commands_succeeded.increment();
        self.concurrent_commands.decrement();
        self.command_duration.record(duration);
    }

    pub fn record_command_failure(&self, error: &CommandError) {
        self.commands_failed.increment();
        self.concurrent_commands.decrement();

        let error_type = match error {
            CommandError::ValidationFailed(_) => "validation_failed",
            CommandError::BusinessRuleViolation(_) => "business_rule_violation",
            CommandError::DomainError { .. } => "domain_error",
            CommandError::ConcurrencyConflict { streams: _ } => "concurrency_conflict",
            CommandError::StreamNotFound(_) => "stream_not_found",
            CommandError::InvalidStreamAccess { .. } => "invalid_stream_access",
            CommandError::StreamNotDeclared { .. } => "stream_not_declared",
            CommandError::TypeMismatch { .. } => "type_mismatch",
            CommandError::Unauthorized(_) => "unauthorized",
            CommandError::EventStore(_) => "event_store_error",
            CommandError::Internal(_) => "internal_error",
            CommandError::Timeout(_) => "timeout",
        };

        if let Ok(mut counters) = self.errors_by_type.write() {
            counters
                .entry(error_type.to_string())
                .or_insert_with(Counter::new)
                .increment();
        }
    }

    pub fn get_command_count_by_type(&self, command_type: &str) -> u64 {
        self.commands_by_type
            .read()
            .ok()
            .and_then(|map| map.get(command_type).map(Counter::get))
            .unwrap_or(0)
    }

    pub fn get_error_count_by_type(&self, error_type: &str) -> u64 {
        self.errors_by_type
            .read()
            .ok()
            .and_then(|map| map.get(error_type).map(Counter::get))
            .unwrap_or(0)
    }
}

impl Default for CommandMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Event store operation metrics
#[derive(Debug)]
pub struct EventStoreMetrics {
    pub reads_total: Counter,
    pub writes_total: Counter,
    pub events_written: Counter,
    pub events_read: Counter,
    pub read_duration: Timer,
    pub write_duration: Timer,
    pub concurrent_operations: Gauge,
    pub stream_count: Gauge,
    pub operations_by_stream: Arc<RwLock<HashMap<StreamId, Counter>>>,
}

impl EventStoreMetrics {
    pub fn new() -> Self {
        Self {
            reads_total: Counter::new(),
            writes_total: Counter::new(),
            events_written: Counter::new(),
            events_read: Counter::new(),
            read_duration: Timer::new(),
            write_duration: Timer::new(),
            concurrent_operations: Gauge::new(),
            stream_count: Gauge::new(),
            operations_by_stream: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn record_read_start(&self, stream_ids: &[StreamId]) {
        self.reads_total.increment();
        self.concurrent_operations.increment();

        if let Ok(mut counters) = self.operations_by_stream.write() {
            for stream_id in stream_ids {
                counters
                    .entry(stream_id.clone())
                    .or_insert_with(Counter::new)
                    .increment();
            }
        }
    }

    pub fn record_read_complete(&self, events_count: usize, duration: Duration) {
        self.events_read.increment_by(events_count as u64);
        self.read_duration.record(duration);
        self.concurrent_operations.decrement();
    }

    pub fn record_write_start(&self, stream_ids: &[StreamId]) {
        self.writes_total.increment();
        self.concurrent_operations.increment();

        if let Ok(mut counters) = self.operations_by_stream.write() {
            for stream_id in stream_ids {
                counters
                    .entry(stream_id.clone())
                    .or_insert_with(Counter::new)
                    .increment();
            }
        }
    }

    pub fn record_write_complete(&self, events_count: usize, duration: Duration) {
        self.events_written.increment_by(events_count as u64);
        self.write_duration.record(duration);
        self.concurrent_operations.decrement();
    }

    pub fn update_stream_count(&self, count: usize) {
        #[allow(clippy::cast_precision_loss)]
        self.stream_count.set(count as f64);
    }

    pub fn get_operations_for_stream(&self, stream_id: &StreamId) -> u64 {
        self.operations_by_stream
            .read()
            .ok()
            .and_then(|map| map.get(stream_id).map(Counter::get))
            .unwrap_or(0)
    }
}

impl Default for EventStoreMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Projection lag and processing metrics
#[derive(Debug)]
pub struct ProjectionMetrics {
    pub events_processed: Counter,
    pub events_skipped: Counter,
    pub projection_errors: Counter,
    pub processing_duration: Timer,
    pub lag_by_projection: Arc<RwLock<HashMap<String, Gauge>>>,
    pub last_processed_event: Arc<RwLock<HashMap<String, EventId>>>,
    pub checkpoint_updates: Counter,
    pub active_projections: Gauge,
}

impl ProjectionMetrics {
    pub fn new() -> Self {
        Self {
            events_processed: Counter::new(),
            events_skipped: Counter::new(),
            projection_errors: Counter::new(),
            processing_duration: Timer::new(),
            lag_by_projection: Arc::new(RwLock::new(HashMap::new())),
            last_processed_event: Arc::new(RwLock::new(HashMap::new())),
            checkpoint_updates: Counter::new(),
            active_projections: Gauge::new(),
        }
    }

    pub fn record_event_processed(
        &self,
        projection_name: &str,
        event_id: EventId,
        duration: Duration,
    ) {
        self.events_processed.increment();
        self.processing_duration.record(duration);

        if let Ok(mut last_events) = self.last_processed_event.write() {
            last_events.insert(projection_name.to_string(), event_id);
        }
    }

    pub fn record_event_skipped(&self, _projection_name: &str) {
        self.events_skipped.increment();
    }

    pub fn record_projection_error(&self, _projection_name: &str) {
        self.projection_errors.increment();
    }

    pub fn update_projection_lag(&self, projection_name: &str, lag_duration: Duration) {
        if let Ok(mut lag_gauges) = self.lag_by_projection.write() {
            lag_gauges
                .entry(projection_name.to_string())
                .or_insert_with(Gauge::new)
                .set({
                    #[allow(clippy::cast_precision_loss)]
                    {
                        lag_duration.as_millis() as f64
                    }
                });
        }
    }

    pub fn record_checkpoint_update(&self, _projection_name: &str) {
        self.checkpoint_updates.increment();
    }

    pub fn set_active_projections(&self, count: usize) {
        #[allow(clippy::cast_precision_loss)]
        self.active_projections.set(count as f64);
    }

    pub fn get_projection_lag(&self, projection_name: &str) -> f64 {
        self.lag_by_projection
            .read()
            .ok()
            .and_then(|map| map.get(projection_name).map(Gauge::get))
            .unwrap_or(0.0)
    }

    pub fn get_last_processed_event(&self, projection_name: &str) -> Option<EventId> {
        self.last_processed_event
            .read()
            .ok()
            .and_then(|map| map.get(projection_name).copied())
    }
}

impl Default for ProjectionMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// System performance metrics for overall health monitoring
#[derive(Debug)]
pub struct SystemMetrics {
    pub memory_usage: Gauge,
    pub cpu_usage: Gauge,
    pub connection_pool_size: Gauge,
    pub connection_pool_available: Gauge,
    pub gc_collections: Counter,
    pub gc_pause_time: Timer,
    pub disk_io_operations: Counter,
    pub network_connections: Gauge,
    pub circuit_breaker_states: Arc<RwLock<HashMap<String, String>>>,
    pub subscription_lag: Arc<RwLock<HashMap<String, Gauge>>>,
}

#[allow(clippy::cast_precision_loss)]
impl SystemMetrics {
    pub fn new() -> Self {
        Self {
            memory_usage: Gauge::new(),
            cpu_usage: Gauge::new(),
            connection_pool_size: Gauge::new(),
            connection_pool_available: Gauge::new(),
            gc_collections: Counter::new(),
            gc_pause_time: Timer::new(),
            disk_io_operations: Counter::new(),
            network_connections: Gauge::new(),
            circuit_breaker_states: Arc::new(RwLock::new(HashMap::new())),
            subscription_lag: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn record_memory_usage(&self, bytes: f64) {
        self.memory_usage.set(bytes);
        debug!(memory_usage_bytes = bytes, "Memory usage updated");
    }

    pub fn record_cpu_usage(&self, percentage: f64) {
        self.cpu_usage.set(percentage);
        if percentage > 80.0 {
            warn!(cpu_usage_percent = percentage, "High CPU usage detected");
        }
    }

    pub fn update_connection_pool(&self, total: usize, available: usize) {
        self.connection_pool_size.set(total as f64);
        self.connection_pool_available.set(available as f64);

        let utilization = if total > 0 {
            ((total - available) as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        if utilization > 90.0 {
            warn!(
                pool_size = total,
                available = available,
                utilization_percent = utilization,
                "Connection pool nearing capacity"
            );
        }
    }

    pub fn record_gc_collection(&self, pause_time: Duration) {
        self.gc_collections.increment();
        self.gc_pause_time.record(pause_time);

        if pause_time > Duration::from_millis(100) {
            warn!(
                pause_time_ms = pause_time.as_millis(),
                "Long GC pause detected"
            );
        }
    }

    pub fn update_circuit_breaker_state(&self, name: &str, state: &str) {
        if let Ok(mut states) = self.circuit_breaker_states.write() {
            let old_state = states.insert(name.to_string(), state.to_string());
            if old_state.as_deref() != Some(state) {
                info!(
                    circuit_breaker = name,
                    old_state = old_state.as_deref().unwrap_or("unknown"),
                    new_state = state,
                    "Circuit breaker state changed"
                );
            }
        }
    }

    pub fn update_subscription_lag(&self, subscription_name: &str, lag: Duration) {
        if let Ok(mut lag_gauges) = self.subscription_lag.write() {
            lag_gauges
                .entry(subscription_name.to_string())
                .or_insert_with(Gauge::new)
                .set(lag.as_millis() as f64);

            if lag > Duration::from_secs(60) {
                warn!(
                    subscription = subscription_name,
                    lag_ms = lag.as_millis(),
                    "High subscription lag detected"
                );
            }
        }
    }
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Error metrics for comprehensive error tracking
#[derive(Debug)]
pub struct ErrorMetrics {
    pub errors_by_type: Arc<RwLock<HashMap<String, Counter>>>,
    pub errors_by_operation: Arc<RwLock<HashMap<String, Counter>>>,
    pub error_rate: Gauge,
    pub mean_time_to_resolution: Timer,
    pub critical_errors: Counter,
    pub transient_errors: Counter,
    pub permanent_errors: Counter,
}

#[allow(clippy::unused_self, clippy::cast_precision_loss)]
impl ErrorMetrics {
    pub fn new() -> Self {
        Self {
            errors_by_type: Arc::new(RwLock::new(HashMap::new())),
            errors_by_operation: Arc::new(RwLock::new(HashMap::new())),
            error_rate: Gauge::new(),
            mean_time_to_resolution: Timer::new(),
            critical_errors: Counter::new(),
            transient_errors: Counter::new(),
            permanent_errors: Counter::new(),
        }
    }

    pub fn record_command_error(&self, error: &CommandError, operation: &str) {
        let error_type = self.classify_command_error(error);
        self.record_error(
            &error_type,
            operation,
            self.is_critical_command_error(error),
        );
    }

    pub fn record_event_store_error(&self, error: &EventStoreError, operation: &str) {
        let error_type = self.classify_event_store_error(error);
        self.record_error(
            &error_type,
            operation,
            self.is_critical_event_store_error(error),
        );
    }

    fn record_error(&self, error_type: &str, operation: &str, is_critical: bool) {
        // Record by type
        if let Ok(mut errors_by_type) = self.errors_by_type.write() {
            errors_by_type
                .entry(error_type.to_string())
                .or_insert_with(Counter::new)
                .increment();
        }

        // Record by operation
        if let Ok(mut errors_by_operation) = self.errors_by_operation.write() {
            errors_by_operation
                .entry(operation.to_string())
                .or_insert_with(Counter::new)
                .increment();
        }

        // Classify error severity
        if is_critical {
            self.critical_errors.increment();
            warn!(
                error_type = error_type,
                operation = operation,
                "Critical error recorded"
            );
        } else if self.is_transient_error(error_type) {
            self.transient_errors.increment();
        } else {
            self.permanent_errors.increment();
        }
    }

    fn classify_command_error(&self, error: &CommandError) -> String {
        match error {
            CommandError::ValidationFailed(_) => "validation_failed".to_string(),
            CommandError::BusinessRuleViolation(_) => "business_rule_violation".to_string(),
            CommandError::DomainError { .. } => "domain_error".to_string(),
            CommandError::ConcurrencyConflict { .. } => "concurrency_conflict".to_string(),
            CommandError::StreamNotFound(_) => "stream_not_found".to_string(),
            CommandError::InvalidStreamAccess { .. } => "invalid_stream_access".to_string(),
            CommandError::StreamNotDeclared { .. } => "stream_not_declared".to_string(),
            CommandError::TypeMismatch { .. } => "type_mismatch".to_string(),
            CommandError::Unauthorized(_) => "unauthorized".to_string(),
            CommandError::EventStore(_) => "event_store_error".to_string(),
            CommandError::Internal(_) => "internal_error".to_string(),
            CommandError::Timeout(_) => "timeout".to_string(),
        }
    }

    fn classify_event_store_error(&self, error: &EventStoreError) -> String {
        match error {
            EventStoreError::StreamNotFound(_) => "stream_not_found".to_string(),
            EventStoreError::VersionConflict { .. } => "version_conflict".to_string(),
            EventStoreError::DuplicateEventId(_) => "duplicate_event_id".to_string(),
            EventStoreError::ConnectionFailed(_) => "connection_failed".to_string(),
            EventStoreError::Configuration(_) => "configuration_error".to_string(),
            EventStoreError::TransactionRollback(_) => "transaction_rollback".to_string(),
            EventStoreError::SerializationFailed(_) => "serialization_failed".to_string(),
            EventStoreError::DeserializationFailed(_) => "deserialization_failed".to_string(),
            EventStoreError::SchemaEvolutionError(_) => "schema_evolution_error".to_string(),
            EventStoreError::Io(_) => "io_error".to_string(),
            EventStoreError::Timeout(_) => "timeout".to_string(),
            EventStoreError::Unavailable(_) => "unavailable".to_string(),
            EventStoreError::Internal(_) => "internal_error".to_string(),
        }
    }

    const fn is_critical_command_error(&self, error: &CommandError) -> bool {
        matches!(
            error,
            CommandError::Internal(_) | CommandError::EventStore(_)
        )
    }

    const fn is_critical_event_store_error(&self, error: &EventStoreError) -> bool {
        matches!(
            error,
            EventStoreError::Internal(_)
                | EventStoreError::Configuration(_)
                | EventStoreError::Io(_)
        )
    }

    fn is_transient_error(&self, error_type: &str) -> bool {
        matches!(
            error_type,
            "timeout"
                | "connection_failed"
                | "version_conflict"
                | "unavailable"
                | "transaction_rollback"
        )
    }

    pub fn calculate_error_rate(&self, total_operations: u64) -> f64 {
        let total_errors =
            self.critical_errors.get() + self.transient_errors.get() + self.permanent_errors.get();
        if total_operations > 0 {
            (total_errors as f64 / total_operations as f64) * 100.0
        } else {
            0.0
        }
    }

    pub fn get_error_count_by_type(&self, error_type: &str) -> u64 {
        self.errors_by_type
            .read()
            .ok()
            .and_then(|map| map.get(error_type).map(Counter::get))
            .unwrap_or(0)
    }

    pub fn get_error_count_by_operation(&self, operation: &str) -> u64 {
        self.errors_by_operation
            .read()
            .ok()
            .and_then(|map| map.get(operation).map(Counter::get))
            .unwrap_or(0)
    }
}

impl Default for ErrorMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Centralized metrics registry with comprehensive observability
#[derive(Debug)]
#[allow(clippy::struct_field_names)]
pub struct MetricsRegistry {
    pub command_metrics: CommandMetrics,
    pub event_store_metrics: EventStoreMetrics,
    pub projection_metrics: ProjectionMetrics,
    pub system_metrics: SystemMetrics,
    pub error_metrics: ErrorMetrics,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        Self {
            command_metrics: CommandMetrics::new(),
            event_store_metrics: EventStoreMetrics::new(),
            projection_metrics: ProjectionMetrics::new(),
            system_metrics: SystemMetrics::new(),
            error_metrics: ErrorMetrics::new(),
        }
    }

    /// Records comprehensive metrics for command execution
    pub fn record_command_execution(
        &self,
        command_type: &str,
        result: &Result<(), CommandError>,
        duration: Duration,
        retry_count: u32,
        stream_count: usize,
    ) {
        // Record basic command metrics
        self.command_metrics.record_command_start(command_type);

        match result {
            Ok(()) => {
                self.command_metrics.record_command_success(duration);
                info!(
                    command_type = command_type,
                    duration_ms = duration.as_millis(),
                    retry_count = retry_count,
                    stream_count = stream_count,
                    "Command execution succeeded"
                );
            }
            Err(error) => {
                self.command_metrics.record_command_failure(error);
                self.error_metrics
                    .record_command_error(error, "command_execution");
                warn!(
                    command_type = command_type,
                    error = %error,
                    duration_ms = duration.as_millis(),
                    retry_count = retry_count,
                    stream_count = stream_count,
                    "Command execution failed"
                );
            }
        }
    }

    /// Records comprehensive metrics for event store operations
    pub fn record_event_store_operation(
        &self,
        operation: &str,
        stream_ids: &[StreamId],
        result: &Result<usize, EventStoreError>,
        duration: Duration,
    ) {
        match operation {
            "read" => {
                self.event_store_metrics.record_read_start(stream_ids);
                match result {
                    Ok(event_count) => {
                        self.event_store_metrics
                            .record_read_complete(*event_count, duration);
                        debug!(
                            operation = operation,
                            stream_count = stream_ids.len(),
                            event_count = event_count,
                            duration_ms = duration.as_millis(),
                            "Event store read completed"
                        );
                    }
                    Err(error) => {
                        self.error_metrics
                            .record_event_store_error(error, operation);
                        warn!(
                            operation = operation,
                            stream_count = stream_ids.len(),
                            error = %error,
                            duration_ms = duration.as_millis(),
                            "Event store read failed"
                        );
                    }
                }
            }
            "write" => {
                self.event_store_metrics.record_write_start(stream_ids);
                match result {
                    Ok(event_count) => {
                        self.event_store_metrics
                            .record_write_complete(*event_count, duration);
                        debug!(
                            operation = operation,
                            stream_count = stream_ids.len(),
                            event_count = event_count,
                            duration_ms = duration.as_millis(),
                            "Event store write completed"
                        );
                    }
                    Err(error) => {
                        self.error_metrics
                            .record_event_store_error(error, operation);
                        warn!(
                            operation = operation,
                            stream_count = stream_ids.len(),
                            error = %error,
                            duration_ms = duration.as_millis(),
                            "Event store write failed"
                        );
                    }
                }
            }
            _ => {
                debug!(operation = operation, "Unknown event store operation");
            }
        }
    }

    /// Updates health metrics for alerting
    pub fn update_health_metrics(&self) {
        let command_error_rate = self
            .error_metrics
            .calculate_error_rate(self.command_metrics.commands_executed.get());

        if command_error_rate > 5.0 {
            warn!(
                error_rate_percent = command_error_rate,
                "High command error rate detected"
            );
        }

        let event_store_throughput = self.event_store_metrics.events_written.get() as f64 / 60.0; // per minute
        if event_store_throughput < 100.0 {
            warn!(
                throughput_per_minute = event_store_throughput,
                "Low event store throughput detected"
            );
        }
    }

    pub fn reset_all(&self) {
        // Reset command metrics
        self.command_metrics.commands_executed.reset();
        self.command_metrics.commands_succeeded.reset();
        self.command_metrics.commands_failed.reset();

        // Reset event store metrics
        self.event_store_metrics.reads_total.reset();
        self.event_store_metrics.writes_total.reset();
        self.event_store_metrics.events_written.reset();
        self.event_store_metrics.events_read.reset();

        // Reset projection metrics
        self.projection_metrics.events_processed.reset();
        self.projection_metrics.events_skipped.reset();
        self.projection_metrics.projection_errors.reset();
        self.projection_metrics.checkpoint_updates.reset();
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn counter_increments_correctly() {
        let counter = Counter::new();
        assert_eq!(counter.get(), 0);

        counter.increment();
        assert_eq!(counter.get(), 1);

        counter.increment_by(5);
        assert_eq!(counter.get(), 6);

        counter.reset();
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn gauge_updates_correctly() {
        let gauge = Gauge::new();
        assert_eq!(gauge.get(), 0.0);

        gauge.set(10.5);
        assert_eq!(gauge.get(), 10.5);

        gauge.increment();
        assert_eq!(gauge.get(), 11.5);

        gauge.increment_by(2.5);
        assert_eq!(gauge.get(), 14.0);

        gauge.decrement();
        assert_eq!(gauge.get(), 13.0);

        gauge.decrement_by(3.0);
        assert_eq!(gauge.get(), 10.0);
    }

    #[test]
    fn timer_records_durations() {
        let timer = Timer::new();

        timer.record(Duration::from_millis(100));
        timer.record(Duration::from_millis(200));
        timer.record(Duration::from_millis(300));

        let samples = timer.get_samples();
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0], Duration::from_millis(100));
        assert_eq!(samples[1], Duration::from_millis(200));
        assert_eq!(samples[2], Duration::from_millis(300));

        let mean = timer.mean().unwrap();
        assert_eq!(mean, Duration::from_millis(200));
    }

    #[test]
    fn timer_calculates_percentiles() {
        let timer = Timer::new();

        // Add 100 samples from 1ms to 100ms
        for i in 1..=100 {
            timer.record(Duration::from_millis(i));
        }

        let p95 = timer.p95().unwrap();
        let p99 = timer.p99().unwrap();

        // P95 should be around 95ms
        assert!(p95.as_millis() >= 94 && p95.as_millis() <= 96);
        // P99 should be around 99ms
        assert!(p99.as_millis() >= 98 && p99.as_millis() <= 100);
    }

    #[test]
    fn timer_limits_sample_count() {
        let timer = Timer::new();

        // Add more than 1000 samples
        for i in 1..=1500 {
            timer.record(Duration::from_millis(i));
        }

        let samples = timer.get_samples();
        assert_eq!(samples.len(), 1000);

        // Should contain the most recent 1000 samples
        assert_eq!(samples[0], Duration::from_millis(501));
        assert_eq!(samples[999], Duration::from_millis(1500));
    }

    #[test]
    fn command_metrics_track_execution() {
        let metrics = CommandMetrics::new();

        metrics.record_command_start("TestCommand");
        assert_eq!(metrics.commands_executed.get(), 1);
        assert_eq!(metrics.concurrent_commands.get(), 1.0);
        assert_eq!(metrics.get_command_count_by_type("TestCommand"), 1);

        metrics.record_command_success(Duration::from_millis(100));
        assert_eq!(metrics.commands_succeeded.get(), 1);
        assert_eq!(metrics.concurrent_commands.get(), 0.0);

        let error = CommandError::ValidationFailed("test".to_string());
        metrics.record_command_failure(&error);
        assert_eq!(metrics.commands_failed.get(), 1);
        assert_eq!(metrics.get_error_count_by_type("validation_failed"), 1);
    }

    #[test]
    fn event_store_metrics_track_operations() {
        let metrics = EventStoreMetrics::new();
        let stream_id = crate::types::StreamId::try_new("test-stream").unwrap();

        metrics.record_read_start(&[stream_id.clone()]);
        assert_eq!(metrics.reads_total.get(), 1);
        assert_eq!(metrics.concurrent_operations.get(), 1.0);
        assert_eq!(metrics.get_operations_for_stream(&stream_id), 1);

        metrics.record_read_complete(5, Duration::from_millis(50));
        assert_eq!(metrics.events_read.get(), 5);
        assert_eq!(metrics.concurrent_operations.get(), 0.0);

        metrics.record_write_start(&[stream_id.clone()]);
        metrics.record_write_complete(3, Duration::from_millis(30));
        assert_eq!(metrics.writes_total.get(), 1);
        assert_eq!(metrics.events_written.get(), 3);
        assert_eq!(metrics.get_operations_for_stream(&stream_id), 2);
    }

    #[test]
    fn projection_metrics_track_processing() {
        let metrics = ProjectionMetrics::new();
        let event_id = crate::types::EventId::new();

        metrics.record_event_processed("TestProjection", event_id, Duration::from_millis(10));
        assert_eq!(metrics.events_processed.get(), 1);
        assert_eq!(
            metrics.get_last_processed_event("TestProjection"),
            Some(event_id)
        );

        metrics.record_event_skipped("TestProjection");
        assert_eq!(metrics.events_skipped.get(), 1);

        metrics.record_projection_error("TestProjection");
        assert_eq!(metrics.projection_errors.get(), 1);

        metrics.update_projection_lag("TestProjection", Duration::from_millis(500));
        assert_eq!(metrics.get_projection_lag("TestProjection"), 500.0);

        metrics.record_checkpoint_update("TestProjection");
        assert_eq!(metrics.checkpoint_updates.get(), 1);
    }

    #[test]
    fn metrics_registry_centralizes_all_metrics() {
        let registry = MetricsRegistry::new();

        // Test that all metric types are accessible
        registry.command_metrics.commands_executed.increment();
        registry.event_store_metrics.reads_total.increment();
        registry.projection_metrics.events_processed.increment();

        assert_eq!(registry.command_metrics.commands_executed.get(), 1);
        assert_eq!(registry.event_store_metrics.reads_total.get(), 1);
        assert_eq!(registry.projection_metrics.events_processed.get(), 1);

        // Test reset functionality
        registry.reset_all();
        assert_eq!(registry.command_metrics.commands_executed.get(), 0);
        assert_eq!(registry.event_store_metrics.reads_total.get(), 0);
        assert_eq!(registry.projection_metrics.events_processed.get(), 0);
    }
}
