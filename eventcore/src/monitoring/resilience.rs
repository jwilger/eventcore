//! Circuit breaker implementation for resilience patterns.
//!
//! This module provides circuit breaker functionality to prevent cascading failures
//! in EventCore operations. The circuit breaker monitors failure rates and response
//! times, automatically opening the circuit to prevent overwhelming failing services.
//!
//! # Architecture
//!
//! The circuit breaker follows a type-driven approach with three main states:
//! - **Closed**: Normal operation, requests pass through
//! - **Open**: Circuit is open, requests fail fast
//! - **HalfOpen**: Testing if service has recovered
//!
//! # Integration
//!
//! Circuit breakers integrate with:
//! - EventStore operations for database resilience
//! - Health monitoring for service degradation detection
//! - Metrics collection for observability
//! - Retry policies for coordinated failure handling

use std::collections::VecDeque;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use nutype::nutype;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::errors::{CommandError, EventStoreError};

/// Circuit breaker failure threshold percentage.
/// Valid range: 0.0 to 100.0
#[nutype(
    validate(greater_or_equal = 0.0, less_or_equal = 100.0),
    derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)
)]
pub struct FailureThreshold(f64);

impl Default for FailureThreshold {
    fn default() -> Self {
        Self::try_new(50.0).unwrap()
    }
}

/// Circuit breaker timeout duration in milliseconds.
/// Must be positive and reasonable (1ms to 10 minutes).
#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 600_000),
    derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Serialize, Deserialize)
)]
pub struct TimeoutMs(u64);

impl Default for TimeoutMs {
    fn default() -> Self {
        Self::try_new(30_000).unwrap() // 30 seconds
    }
}

impl From<TimeoutMs> for Duration {
    fn from(timeout: TimeoutMs) -> Self {
        Self::from_millis(timeout.into_inner())
    }
}

/// Circuit breaker sample window size.
/// Number of recent operations to consider for failure rate calculation.
#[nutype(
    validate(greater_or_equal = 10, less_or_equal = 10_000),
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
pub struct WindowSize(usize);

impl Default for WindowSize {
    fn default() -> Self {
        Self::try_new(100).unwrap()
    }
}

/// Circuit breaker minimum requests threshold.
/// Minimum number of requests before failure rate is considered.
#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 1000),
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
pub struct MinRequests(usize);

impl Default for MinRequests {
    fn default() -> Self {
        Self::try_new(10).unwrap()
    }
}

/// Circuit breaker configuration with validated parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Failure threshold percentage (0.0-100.0)
    pub failure_threshold: FailureThreshold,

    /// Timeout before attempting to close circuit
    pub timeout: TimeoutMs,

    /// Number of recent operations to consider
    pub window_size: WindowSize,

    /// Minimum requests before considering failure rate
    pub min_requests: MinRequests,

    /// Whether to enable circuit breaker
    pub enabled: bool,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: FailureThreshold::default(),
            timeout: TimeoutMs::default(),
            window_size: WindowSize::default(),
            min_requests: MinRequests::default(),
            enabled: true,
        }
    }
}

/// Circuit breaker state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CircuitState {
    /// Circuit is closed, requests pass through normally
    Closed,
    /// Circuit is open, requests fail fast
    Open,
    /// Circuit is half-open, testing if service recovered
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "closed"),
            Self::Open => write!(f, "open"),
            Self::HalfOpen => write!(f, "half-open"),
        }
    }
}

/// Result of a circuit breaker operation.
#[derive(Debug, Clone)]
pub enum CircuitResult<T> {
    /// Operation succeeded
    Success(T),
    /// Operation failed, circuit remains closed
    Failure,
    /// Circuit is open, operation was rejected
    CircuitOpen,
}

impl<T> CircuitResult<T> {
    /// Returns true if the operation was successful.
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    /// Returns true if the operation failed.
    pub const fn is_failure(&self) -> bool {
        matches!(self, Self::Failure)
    }

    /// Returns true if the circuit is open.
    pub const fn is_circuit_open(&self) -> bool {
        matches!(self, Self::CircuitOpen)
    }

    /// Unwraps the success value or panics.
    pub fn unwrap(self) -> T {
        match self {
            Self::Success(value) => value,
            Self::Failure => {
                panic!("called `CircuitResult::unwrap()` on a `Failure` value")
            }
            Self::CircuitOpen => {
                panic!("called `CircuitResult::unwrap()` on a `CircuitOpen` value")
            }
        }
    }

    /// Returns the success value or a default.
    pub fn unwrap_or(self, default: T) -> T {
        match self {
            Self::Success(value) => value,
            _ => default,
        }
    }
}

/// Internal state for circuit breaker operation tracking.
#[derive(Debug)]
struct CircuitBreakerState {
    state: CircuitState,
    last_failure_time: Option<Instant>,
    request_count: usize,
    failure_count: usize,
    recent_results: VecDeque<bool>, // true = success, false = failure
}

impl CircuitBreakerState {
    const fn new() -> Self {
        Self {
            state: CircuitState::Closed,
            last_failure_time: None,
            request_count: 0,
            failure_count: 0,
            recent_results: VecDeque::new(),
        }
    }

    fn record_success(&mut self, config: &CircuitBreakerConfig) {
        self.request_count += 1;
        self.add_result(true, config);

        // If we're half-open and got a success, close the circuit
        if self.state == CircuitState::HalfOpen {
            self.state = CircuitState::Closed;
            self.failure_count = self.recent_results.iter().filter(|&&r| !r).count();
            debug!("Circuit breaker closed after successful test");
        }
    }

    fn record_failure(&mut self, config: &CircuitBreakerConfig) {
        self.request_count += 1;
        self.failure_count += 1;
        self.last_failure_time = Some(Instant::now());
        self.add_result(false, config);

        // Check if we should open the circuit
        if self.should_open_circuit(config) {
            self.state = CircuitState::Open;
            warn!("Circuit breaker opened due to failure threshold exceeded");
        }
    }

    fn add_result(&mut self, success: bool, config: &CircuitBreakerConfig) {
        self.recent_results.push_back(success);
        if self.recent_results.len() > config.window_size.into_inner() {
            if let Some(old_result) = self.recent_results.pop_front() {
                if !old_result {
                    self.failure_count = self.failure_count.saturating_sub(1);
                }
            }
        }
    }

    fn should_open_circuit(&self, config: &CircuitBreakerConfig) -> bool {
        if self.state == CircuitState::Open {
            return false; // Already open
        }

        let min_requests = config.min_requests.into_inner();
        if self.recent_results.len() < min_requests {
            return false; // Not enough data
        }

        #[allow(clippy::cast_precision_loss)]
        let failure_rate = (self.failure_count as f64 / self.recent_results.len() as f64) * 100.0;
        failure_rate >= config.failure_threshold.into_inner()
    }

    fn can_attempt_request(&mut self, config: &CircuitBreakerConfig) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => false, // Only allow one test request at a time
            CircuitState::Open => {
                if let Some(last_failure) = self.last_failure_time {
                    let timeout_duration: Duration = config.timeout.into();
                    if last_failure.elapsed() >= timeout_duration {
                        self.state = CircuitState::HalfOpen;
                        debug!("Circuit breaker moved to half-open state");
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        }
    }

    const fn current_state(&self) -> CircuitState {
        self.state
    }

    fn failure_rate(&self) -> f64 {
        if self.recent_results.is_empty() {
            0.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            let rate = (self.failure_count as f64 / self.recent_results.len() as f64) * 100.0;
            rate
        }
    }
}

/// Circuit breaker implementation for preventing cascading failures.
#[derive(Debug)]
pub struct CircuitBreaker {
    name: String,
    config: CircuitBreakerConfig,
    state: Arc<RwLock<CircuitBreakerState>>,
}

impl CircuitBreaker {
    /// Creates a new circuit breaker with the given configuration.
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            name: name.into(),
            config,
            state: Arc::new(RwLock::new(CircuitBreakerState::new())),
        }
    }

    /// Executes an operation through the circuit breaker.
    pub async fn call<T, F, Fut, E>(&self, operation: F) -> CircuitResult<Result<T, E>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: IsCircuitBreakerError,
    {
        if !self.config.enabled {
            return CircuitResult::Success(operation().await);
        }

        // Check if we can attempt the request
        let can_attempt = self
            .state
            .write()
            .map_or(true, |mut state| state.can_attempt_request(&self.config));

        if !can_attempt {
            return CircuitResult::CircuitOpen;
        }

        // Execute the operation
        let result = operation().await;

        // Record the result
        match &result {
            Ok(_) => {
                self.record_success();
            }
            Err(error) if error.should_trigger_circuit_breaker() => {
                self.record_failure();
            }
            Err(_) => {
                // Error that doesn't trigger circuit breaker
                self.record_success();
            }
        }

        CircuitResult::Success(result)
    }

    /// Records a successful operation result.
    fn record_success(&self) {
        if let Ok(mut state) = self.state.write() {
            state.record_success(&self.config);
        }
    }

    /// Records a failed operation result.
    fn record_failure(&self) {
        if let Ok(mut state) = self.state.write() {
            state.record_failure(&self.config);
        }
    }

    /// Gets the current circuit breaker state.
    pub fn state(&self) -> CircuitState {
        self.state
            .read()
            .map(|state| state.current_state())
            .unwrap_or(CircuitState::Closed)
    }

    /// Gets the current failure rate percentage.
    pub fn failure_rate(&self) -> f64 {
        self.state
            .read()
            .map(|state| state.failure_rate())
            .unwrap_or(0.0)
    }

    /// Gets circuit breaker statistics.
    pub fn stats(&self) -> CircuitBreakerStats {
        let state = self
            .state
            .read()
            .unwrap_or_else(|_| panic!("Circuit breaker state lock poisoned"));

        CircuitBreakerStats {
            name: self.name.clone(),
            state: state.current_state(),
            request_count: state.request_count,
            failure_count: state.failure_count,
            failure_rate: state.failure_rate(),
            config: self.config.clone(),
        }
    }
}

/// Circuit breaker statistics for monitoring and debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerStats {
    /// Circuit breaker name
    pub name: String,
    /// Current state
    pub state: CircuitState,
    /// Total request count
    pub request_count: usize,
    /// Total failure count
    pub failure_count: usize,
    /// Current failure rate percentage
    pub failure_rate: f64,
    /// Circuit breaker configuration
    pub config: CircuitBreakerConfig,
}

/// Trait for determining if an error should trigger circuit breaker state changes.
pub trait IsCircuitBreakerError {
    /// Returns true if this error should trigger circuit breaker failure handling.
    fn should_trigger_circuit_breaker(&self) -> bool;
}

impl IsCircuitBreakerError for EventStoreError {
    fn should_trigger_circuit_breaker(&self) -> bool {
        match self {
            Self::ConnectionFailed(_)
            | Self::Timeout(_)
            | Self::Unavailable(_)
            | Self::TransactionRollback(_)
            | Self::Io(_)
            | Self::Internal(_) => true,
            Self::SerializationFailed(_)
            | Self::DeserializationFailed(_)
            | Self::VersionConflict { .. }
            | Self::StreamNotFound(_)
            | Self::DuplicateEventId(_)
            | Self::Configuration(_)
            | Self::SchemaEvolutionError(_) => false,
        }
    }
}

impl IsCircuitBreakerError for CommandError {
    fn should_trigger_circuit_breaker(&self) -> bool {
        match self {
            Self::EventStore(error) => error.should_trigger_circuit_breaker(),
            Self::Timeout(_) | Self::Internal(_) => true,
            Self::ValidationFailed(_)
            | Self::BusinessRuleViolation(_)
            | Self::DomainError { .. }
            | Self::ConcurrencyConflict { .. }
            | Self::StreamNotFound(_)
            | Self::Unauthorized(_)
            | Self::InvalidStreamAccess { .. }
            | Self::StreamNotDeclared { .. }
            | Self::TypeMismatch { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[derive(Debug, thiserror::Error)]
    enum TestError {
        #[error("Transient error")]
        Transient,
        #[error("Permanent error")]
        Permanent,
    }

    impl IsCircuitBreakerError for TestError {
        fn should_trigger_circuit_breaker(&self) -> bool {
            matches!(self, Self::Transient)
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_closed_state() {
        let config = CircuitBreakerConfig::default();
        let cb = CircuitBreaker::new("test", config);

        // Initially closed
        assert_eq!(cb.state(), CircuitState::Closed);

        // Successful operations should keep it closed
        let result = cb.call(|| async { Ok::<_, TestError>(42) }).await;
        assert!(result.is_success());
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_opens_on_failures() {
        let config = CircuitBreakerConfig {
            failure_threshold: FailureThreshold::try_new(50.0).unwrap(),
            min_requests: MinRequests::try_new(2).unwrap(),
            window_size: WindowSize::try_new(10).unwrap(),
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // First failure - should stay closed
        let result = cb
            .call(|| async { Err::<i32, _>(TestError::Transient) })
            .await;
        assert!(result.is_success());
        assert_eq!(cb.state(), CircuitState::Closed);

        // Second failure - should open circuit
        let result = cb
            .call(|| async { Err::<i32, _>(TestError::Transient) })
            .await;
        assert!(result.is_success());
        assert_eq!(cb.state(), CircuitState::Open);

        // Next request should be rejected
        let result = cb.call(|| async { Ok::<_, TestError>(42) }).await;
        assert!(result.is_circuit_open());
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_recovery() {
        let config = CircuitBreakerConfig {
            failure_threshold: FailureThreshold::try_new(50.0).unwrap(),
            timeout: TimeoutMs::try_new(100).unwrap(), // 100ms timeout
            min_requests: MinRequests::try_new(2).unwrap(),
            window_size: WindowSize::try_new(10).unwrap(),
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // Trigger circuit to open
        let _ = cb
            .call(|| async { Err::<i32, _>(TestError::Transient) })
            .await;
        let _ = cb
            .call(|| async { Err::<i32, _>(TestError::Transient) })
            .await;
        assert_eq!(cb.state(), CircuitState::Open);

        // Wait for timeout
        sleep(Duration::from_millis(150)).await;

        // Next request should be allowed (half-open)
        let result = cb.call(|| async { Ok::<_, TestError>(42) }).await;
        assert!(result.is_success());
        assert_eq!(cb.state(), CircuitState::Closed); // Should close after success
    }

    #[tokio::test]
    async fn test_circuit_breaker_ignores_non_triggering_errors() {
        let config = CircuitBreakerConfig {
            failure_threshold: FailureThreshold::try_new(50.0).unwrap(),
            min_requests: MinRequests::try_new(2).unwrap(),
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // Non-triggering errors should not open circuit
        for _ in 0..10 {
            let result = cb
                .call(|| async { Err::<i32, _>(TestError::Permanent) })
                .await;
            assert!(result.is_success());
        }

        assert_eq!(cb.state(), CircuitState::Closed);
        assert!((cb.failure_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_circuit_breaker_disabled() {
        let config = CircuitBreakerConfig {
            enabled: false,
            ..Default::default()
        };
        let cb = CircuitBreaker::new("test", config);

        // Should always pass through when disabled
        for _ in 0..10 {
            let result = cb
                .call(|| async { Err::<i32, _>(TestError::Transient) })
                .await;
            assert!(result.is_success());
        }

        assert_eq!(cb.state(), CircuitState::Closed);
    }
}
