//! Chaos testing framework for controlled failure injection.
//!
//! This module provides a comprehensive chaos testing framework that allows
//! controlled injection of various failure modes into EventCore operations.
//! It enables testing of system resilience under adverse conditions.
//!
//! # Architecture
//!
//! The chaos framework follows type-driven development principles:
//! - Type-safe failure injection policies
//! - Composable failure scenarios
//! - Deterministic and random failure modes
//! - Integration with existing EventStore implementations
//!
//! # Usage
//!
//! ```rust,ignore
//! let chaos_store = ChaosEventStore::new(postgres_store)
//!     .with_policy(FailurePolicy::random_errors(0.1))
//!     .with_policy(FailurePolicy::latency_injection(Duration::from_millis(100)));
//!
//! // Use chaos_store in tests to inject failures
//! ```

use crate::errors::{EventStoreError, EventStoreResult};
use crate::event_store::{EventStore, ReadOptions, StreamData, StreamEvents};
use crate::subscription::{Subscription, SubscriptionOptions};
use crate::types::{EventVersion, StreamId};
use async_trait::async_trait;
use nutype::nutype;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, warn};

/// Failure probability percentage (0.0 to 100.0).
#[nutype(
    validate(greater_or_equal = 0.0, less_or_equal = 100.0),
    derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)
)]
pub struct FailureProbability(f64);

impl Default for FailureProbability {
    fn default() -> Self {
        Self::try_new(0.0).unwrap()
    }
}

/// Latency duration in milliseconds.
#[nutype(
    validate(greater_or_equal = 0, less_or_equal = 60_000),
    derive(
        Debug,
        Clone,
        Copy,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Serialize,
        Deserialize
    )
)]
pub struct LatencyMs(u64);

impl From<LatencyMs> for Duration {
    fn from(latency: LatencyMs) -> Self {
        Self::from_millis(latency.into_inner())
    }
}

/// Types of failures that can be injected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureType {
    /// Connection failure
    ConnectionFailure,
    /// Timeout
    Timeout,
    /// Service unavailable
    Unavailable,
    /// I/O error
    IoError,
    /// Transaction rollback
    TransactionRollback,
    /// Version conflict (for testing optimistic concurrency)
    VersionConflict,
    /// Latency injection (not a failure, but a delay)
    LatencyInjection,
    /// Network partition (some operations succeed, some fail)
    NetworkPartition,
}

impl FailureType {
    /// Converts the failure type into an EventStoreError.
    pub fn to_error(&self) -> EventStoreError {
        match self {
            Self::ConnectionFailure => {
                EventStoreError::ConnectionFailed("Chaos: Simulated connection failure".into())
            }
            Self::Timeout => EventStoreError::Timeout(Duration::from_secs(30)),
            Self::Unavailable => {
                EventStoreError::Unavailable("Chaos: Service temporarily unavailable".into())
            }
            Self::IoError => EventStoreError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Chaos: Simulated I/O error",
            )),
            Self::TransactionRollback => {
                EventStoreError::TransactionRollback("Chaos: Simulated transaction rollback".into())
            }
            Self::VersionConflict => EventStoreError::VersionConflict {
                stream: StreamId::try_new("chaos-conflict").unwrap(),
                expected: EventVersion::initial(),
                current: EventVersion::try_new(1).unwrap(),
            },
            Self::LatencyInjection | Self::NetworkPartition => {
                // These don't directly map to errors
                EventStoreError::Internal("Invalid failure type conversion".into())
            }
        }
    }
}

/// A failure injection policy.
#[derive(Debug, Clone)]
pub struct FailurePolicy {
    /// Name of the policy for debugging
    pub name: String,
    /// Type of failure to inject
    pub failure_type: FailureType,
    /// Probability of failure (0.0 to 1.0)
    pub probability: FailureProbability,
    /// Operations to apply this policy to
    pub target_operations: TargetOperations,
    /// Additional configuration
    pub config: PolicyConfig,
}

/// Which operations to target with failure injection.
#[derive(Clone)]
pub enum TargetOperations {
    /// All operations
    All,
    /// Only read operations
    Reads,
    /// Only write operations
    Writes,
    /// Specific streams
    Streams(Vec<StreamId>),
    /// Custom predicate
    Custom {
        /// Name for debugging
        name: String,
        /// The predicate function
        predicate: Arc<dyn Fn(&Operation) -> bool + Send + Sync>,
    },
}

impl PartialEq for TargetOperations {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::All, Self::All) | (Self::Reads, Self::Reads) | (Self::Writes, Self::Writes) => {
                true
            }
            (Self::Streams(a), Self::Streams(b)) => a == b,
            (Self::Custom { name: a, .. }, Self::Custom { name: b, .. }) => a == b,
            _ => false,
        }
    }
}

impl Eq for TargetOperations {}

impl std::fmt::Debug for TargetOperations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => write!(f, "All"),
            Self::Reads => write!(f, "Reads"),
            Self::Writes => write!(f, "Writes"),
            Self::Streams(streams) => f.debug_tuple("Streams").field(streams).finish(),
            Self::Custom { name, .. } => f.debug_struct("Custom").field("name", name).finish(),
        }
    }
}

impl PartialEq for FailurePolicy {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.failure_type == other.failure_type
            && self.probability == other.probability
            && self.target_operations == other.target_operations
    }
}

/// Operation being performed.
#[derive(Debug, Clone)]
pub enum Operation {
    /// Reading from streams
    Read {
        /// The stream IDs being read
        stream_ids: Vec<StreamId>,
    },
    /// Writing to streams
    Write {
        /// The stream IDs being written to
        stream_ids: Vec<StreamId>,
    },
    /// Checking stream existence
    StreamExists {
        /// The stream ID to check
        stream_id: StreamId,
    },
    /// Getting stream version
    GetStreamVersion {
        /// The stream ID to get version for
        stream_id: StreamId,
    },
    /// Subscription
    Subscribe,
}

/// Additional configuration for failure policies.
#[derive(Debug, Clone)]
pub enum PolicyConfig {
    /// No additional configuration
    None,
    /// Latency configuration
    Latency {
        /// Base latency to inject
        base: LatencyMs,
        /// Optional jitter (random variation)
        jitter: Option<LatencyMs>,
    },
    /// Network partition configuration
    Partition {
        /// Streams that are partitioned (fail)
        affected_streams: Vec<StreamId>,
    },
}

impl FailurePolicy {
    /// Creates a policy that randomly fails operations.
    pub fn random_errors(probability: f64, failure_type: FailureType) -> Self {
        Self {
            name: format!(
                "Random {} ({}%)",
                failure_type.to_error(),
                probability * 100.0
            ),
            failure_type,
            probability: FailureProbability::try_new(probability * 100.0).unwrap(),
            target_operations: TargetOperations::All,
            config: PolicyConfig::None,
        }
    }

    /// Creates a policy that injects latency.
    pub fn latency_injection(base_latency: Duration, jitter: Option<Duration>) -> Self {
        Self {
            name: format!("Latency injection ({base_latency:?})"),
            failure_type: FailureType::LatencyInjection,
            probability: FailureProbability::try_new(100.0).unwrap(), // Always apply
            target_operations: TargetOperations::All,
            config: PolicyConfig::Latency {
                base: LatencyMs::try_new(base_latency.as_millis().try_into().unwrap()).unwrap(),
                jitter: jitter
                    .map(|j| LatencyMs::try_new(j.as_millis().try_into().unwrap()).unwrap()),
            },
        }
    }

    /// Creates a policy that simulates network partitions.
    pub fn network_partition(affected_streams: Vec<StreamId>) -> Self {
        Self {
            name: "Network partition".to_string(),
            failure_type: FailureType::NetworkPartition,
            probability: FailureProbability::try_new(100.0).unwrap(),
            target_operations: TargetOperations::Streams(affected_streams.clone()),
            config: PolicyConfig::Partition { affected_streams },
        }
    }

    /// Creates a policy targeting specific operations.
    pub fn targeted(
        name: impl Into<String>,
        failure_type: FailureType,
        probability: f64,
        target: TargetOperations,
    ) -> Self {
        Self {
            name: name.into(),
            failure_type,
            probability: FailureProbability::try_new(probability * 100.0).unwrap(),
            target_operations: target,
            config: PolicyConfig::None,
        }
    }

    /// Checks if this policy should be applied to the given operation.
    fn should_apply(&self, operation: &Operation) -> bool {
        match &self.target_operations {
            TargetOperations::All => true,
            TargetOperations::Reads => matches!(operation, Operation::Read { .. }),
            TargetOperations::Writes => matches!(operation, Operation::Write { .. }),
            TargetOperations::Streams(streams) => match operation {
                Operation::Read { stream_ids } | Operation::Write { stream_ids } => {
                    stream_ids.iter().any(|id| streams.contains(id))
                }
                Operation::StreamExists { stream_id }
                | Operation::GetStreamVersion { stream_id } => streams.contains(stream_id),
                Operation::Subscribe => false,
            },
            TargetOperations::Custom { predicate, .. } => predicate(operation),
        }
    }

    /// Determines if a failure should occur based on probability.
    fn should_fail(&self) -> bool {
        let mut rng = rand::rng();
        let roll: f64 = rng.random_range(0.0..100.0);
        roll < self.probability.into_inner()
    }
}

/// Statistics about chaos injection.
#[derive(Debug, Clone, Default)]
pub struct ChaosStats {
    /// Total operations
    pub total_operations: u64,
    /// Operations that were failed
    pub failed_operations: u64,
    /// Operations that had latency injected
    pub delayed_operations: u64,
    /// Average injected latency
    pub average_latency_ms: f64,
    /// Breakdown by failure type
    pub failure_breakdown: HashMap<String, u64>,
}

/// Event store wrapper that injects chaos.
pub struct ChaosEventStore<S: EventStore> {
    /// The underlying event store
    inner: S,
    /// Active failure policies
    policies: Arc<Mutex<Vec<FailurePolicy>>>,
    /// Statistics
    stats: Arc<Mutex<ChaosStats>>,
    /// Whether chaos is enabled
    enabled: Arc<Mutex<bool>>,
}

impl<S: EventStore> ChaosEventStore<S> {
    /// Creates a new chaos event store wrapping the given store.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            policies: Arc::new(Mutex::new(Vec::new())),
            stats: Arc::new(Mutex::new(ChaosStats::default())),
            enabled: Arc::new(Mutex::new(true)),
        }
    }

    /// Adds a failure injection policy.
    #[must_use]
    pub fn with_policy(self, policy: FailurePolicy) -> Self {
        if let Ok(mut policies) = self.policies.lock() {
            policies.push(policy);
        }
        self
    }

    /// Enables or disables chaos injection.
    pub fn set_enabled(&self, enabled: bool) {
        if let Ok(mut flag) = self.enabled.lock() {
            *flag = enabled;
        }
    }

    /// Gets current statistics.
    pub fn stats(&self) -> ChaosStats {
        self.stats.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Resets statistics.
    pub fn reset_stats(&self) {
        if let Ok(mut stats) = self.stats.lock() {
            *stats = ChaosStats::default();
        }
    }

    /// Applies chaos policies to an operation.
    #[allow(clippy::cast_precision_loss)]
    async fn apply_chaos<T>(
        &self,
        operation: Operation,
        action: impl std::future::Future<Output = EventStoreResult<T>>,
    ) -> EventStoreResult<T> {
        // Check if chaos is enabled
        let enabled = self.enabled.lock().map(|e| *e).unwrap_or(false);
        if !enabled {
            return action.await;
        }

        // Update stats
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_operations += 1;
        }

        // Check applicable policies
        let applicable_policies: Vec<_> = self
            .policies
            .lock()
            .map(|policies| {
                policies
                    .iter()
                    .filter(|p| p.should_apply(&operation))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        // Apply latency injection first
        let start = Instant::now();
        for policy in &applicable_policies {
            if policy.failure_type == FailureType::LatencyInjection {
                if let PolicyConfig::Latency { base, jitter } = &policy.config {
                    let mut delay = Duration::from(*base);
                    if let Some(jitter) = jitter {
                        let mut rng = rand::rng();
                        let jitter_ms = rng.random_range(0..jitter.into_inner());
                        delay += Duration::from_millis(jitter_ms);
                    }
                    debug!("Chaos: Injecting latency of {:?}", delay);
                    sleep(delay).await;

                    if let Ok(mut stats) = self.stats.lock() {
                        stats.delayed_operations += 1;
                        let delayed_ops_f64 = stats.delayed_operations as f64;
                        let total_latency = stats.average_latency_ms * (delayed_ops_f64 - 1.0);
                        let delay_ms = delay.as_millis() as f64;
                        stats.average_latency_ms = (total_latency + delay_ms) / delayed_ops_f64;
                    }
                }
            }
        }

        // Check for failure injection
        for policy in &applicable_policies {
            if policy.failure_type != FailureType::LatencyInjection && policy.should_fail() {
                let error = policy.failure_type.to_error();
                warn!("Chaos: Injecting failure: {:?}", error);

                if let Ok(mut stats) = self.stats.lock() {
                    stats.failed_operations += 1;
                    *stats
                        .failure_breakdown
                        .entry(policy.failure_type.to_error().to_string())
                        .or_insert(0) += 1;
                }

                return Err(error);
            }
        }

        // Execute the actual operation
        let result = action.await;

        // Log operation duration
        let duration = start.elapsed();
        if duration > Duration::from_secs(1) {
            warn!(
                "Chaos: Operation took {:?} (including injected delays)",
                duration
            );
        }

        result
    }
}

impl<S: EventStore + Clone> Clone for ChaosEventStore<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            policies: self.policies.clone(),
            stats: self.stats.clone(),
            enabled: self.enabled.clone(),
        }
    }
}

#[async_trait]
impl<S: EventStore> EventStore for ChaosEventStore<S> {
    type Event = S::Event;

    async fn read_streams(
        &self,
        stream_ids: &[StreamId],
        options: &ReadOptions,
    ) -> EventStoreResult<StreamData<Self::Event>> {
        let operation = Operation::Read {
            stream_ids: stream_ids.to_vec(),
        };
        self.apply_chaos(operation, self.inner.read_streams(stream_ids, options))
            .await
    }

    async fn write_events_multi(
        &self,
        stream_events: Vec<StreamEvents<Self::Event>>,
    ) -> EventStoreResult<HashMap<StreamId, EventVersion>> {
        let stream_ids: Vec<_> = stream_events
            .iter()
            .map(|se| se.stream_id.clone())
            .collect();
        let operation = Operation::Write { stream_ids };
        self.apply_chaos(operation, self.inner.write_events_multi(stream_events))
            .await
    }

    async fn stream_exists(&self, stream_id: &StreamId) -> EventStoreResult<bool> {
        let operation = Operation::StreamExists {
            stream_id: stream_id.clone(),
        };
        self.apply_chaos(operation, self.inner.stream_exists(stream_id))
            .await
    }

    async fn get_stream_version(
        &self,
        stream_id: &StreamId,
    ) -> EventStoreResult<Option<EventVersion>> {
        let operation = Operation::GetStreamVersion {
            stream_id: stream_id.clone(),
        };
        self.apply_chaos(operation, self.inner.get_stream_version(stream_id))
            .await
    }

    async fn subscribe(
        &self,
        options: SubscriptionOptions,
    ) -> EventStoreResult<Box<dyn Subscription<Event = Self::Event>>> {
        let operation = Operation::Subscribe;
        self.apply_chaos(operation, self.inner.subscribe(options))
            .await
    }
}

/// Builder for creating complex chaos scenarios.
pub struct ChaosScenarioBuilder<S: EventStore> {
    store: S,
    policies: Vec<FailurePolicy>,
    name: String,
}

impl<S: EventStore> ChaosScenarioBuilder<S> {
    /// Creates a new chaos scenario builder.
    pub fn new(store: S, name: impl Into<String>) -> Self {
        Self {
            store,
            policies: Vec::new(),
            name: name.into(),
        }
    }

    /// Adds a failure policy to the scenario.
    #[must_use]
    pub fn with_policy(mut self, policy: FailurePolicy) -> Self {
        self.policies.push(policy);
        self
    }

    /// Adds random connection failures.
    #[must_use]
    pub fn with_connection_failures(self, probability: f64) -> Self {
        self.with_policy(FailurePolicy::random_errors(
            probability,
            FailureType::ConnectionFailure,
        ))
    }

    /// Adds random timeouts.
    #[must_use]
    pub fn with_timeouts(self, probability: f64) -> Self {
        self.with_policy(FailurePolicy::random_errors(
            probability,
            FailureType::Timeout,
        ))
    }

    /// Adds latency to all operations.
    #[must_use]
    pub fn with_latency(self, base: Duration, jitter: Option<Duration>) -> Self {
        self.with_policy(FailurePolicy::latency_injection(base, jitter))
    }

    /// Simulates a network partition affecting specific streams.
    #[must_use]
    pub fn with_partition(self, affected_streams: Vec<StreamId>) -> Self {
        self.with_policy(FailurePolicy::network_partition(affected_streams))
    }

    /// Builds the chaos event store with all configured policies.
    pub fn build(self) -> ChaosEventStore<S> {
        let mut chaos_store = ChaosEventStore::new(self.store);
        for policy in self.policies {
            chaos_store = chaos_store.with_policy(policy);
        }
        chaos_store
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::harness::MockEventStore;

    #[derive(Debug, Clone, PartialEq)]
    struct TestEvent;

    #[tokio::test]
    async fn test_chaos_probability() {
        let mock_store = MockEventStore::<TestEvent>::new();
        let chaos_store = ChaosEventStore::new(mock_store)
            .with_policy(FailurePolicy::random_errors(1.0, FailureType::Timeout));

        let result = chaos_store
            .read_streams(
                &[StreamId::try_new("test").unwrap()],
                &ReadOptions::default(),
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), EventStoreError::Timeout(_)));
    }

    #[tokio::test]
    async fn test_chaos_disabled() {
        let mock_store = MockEventStore::<TestEvent>::new();
        let chaos_store = ChaosEventStore::new(mock_store)
            .with_policy(FailurePolicy::random_errors(1.0, FailureType::Timeout));

        chaos_store.set_enabled(false);

        let result = chaos_store
            .read_streams(
                &[StreamId::try_new("test").unwrap()],
                &ReadOptions::default(),
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_latency_injection() {
        let mock_store = MockEventStore::<TestEvent>::new();
        let chaos_store = ChaosEventStore::new(mock_store).with_policy(
            FailurePolicy::latency_injection(Duration::from_millis(100), None),
        );

        let start = Instant::now();
        let _result = chaos_store
            .read_streams(
                &[StreamId::try_new("test").unwrap()],
                &ReadOptions::default(),
            )
            .await;
        let duration = start.elapsed();

        assert!(duration >= Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_stats_collection() {
        let mock_store = MockEventStore::<TestEvent>::new();

        // Test with deterministic failure rates
        // First test: 30% failure rate
        let chaos_store = ChaosEventStore::new(mock_store.clone())
            .with_policy(FailurePolicy::random_errors(0.3, FailureType::Timeout));

        // Perform 100 operations to ensure statistical significance
        for _ in 0..100 {
            let _ = chaos_store
                .read_streams(
                    &[StreamId::try_new("test").unwrap()],
                    &ReadOptions::default(),
                )
                .await;
        }

        let stats = chaos_store.stats();
        assert_eq!(stats.total_operations, 100);

        // With 30% failure rate and 100 operations, we expect approximately 30 failures
        // Allow for variance: between 15 and 45 failures (15% to 45%)
        assert!(
            stats.failed_operations >= 15,
            "Expected at least 15 failures, got {}",
            stats.failed_operations
        );
        assert!(
            stats.failed_operations <= 45,
            "Expected at most 45 failures, got {}",
            stats.failed_operations
        );

        // Reset and test with 0% failure rate
        chaos_store.reset_stats();
        let chaos_store_no_fail = ChaosEventStore::new(mock_store.clone())
            .with_policy(FailurePolicy::random_errors(0.0, FailureType::Timeout));

        for _ in 0..10 {
            let _ = chaos_store_no_fail
                .read_streams(
                    &[StreamId::try_new("test").unwrap()],
                    &ReadOptions::default(),
                )
                .await;
        }

        let stats_no_fail = chaos_store_no_fail.stats();
        assert_eq!(stats_no_fail.total_operations, 10);
        assert_eq!(
            stats_no_fail.failed_operations, 0,
            "Expected no failures with 0% failure rate"
        );

        // Test with 100% failure rate
        let chaos_store_all_fail = ChaosEventStore::new(mock_store)
            .with_policy(FailurePolicy::random_errors(1.0, FailureType::Timeout));

        for _ in 0..10 {
            let _ = chaos_store_all_fail
                .read_streams(
                    &[StreamId::try_new("test").unwrap()],
                    &ReadOptions::default(),
                )
                .await;
        }

        let stats_all_fail = chaos_store_all_fail.stats();
        assert_eq!(stats_all_fail.total_operations, 10);
        assert_eq!(
            stats_all_fail.failed_operations, 10,
            "Expected all operations to fail with 100% failure rate"
        );
    }
}
