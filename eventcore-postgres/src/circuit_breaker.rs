//! Circuit breaker pattern implementation for `PostgreSQL` operations
//!
//! This module provides resilient circuit breaker functionality to prevent
//! cascading failures when database operations are failing consistently.

#![allow(clippy::wildcard_in_or_patterns)]
#![allow(clippy::significant_drop_tightening)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::significant_drop_in_scrutinee)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::float_cmp)]

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, error, instrument, warn};

use crate::PostgresError;

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum CircuitState {
    /// Circuit is closed, operations flow normally
    Closed = 0,
    /// Circuit is open, operations are rejected immediately
    Open = 1,
    /// Circuit is half-open, testing if service has recovered
    HalfOpen = 2,
}

impl From<u8> for CircuitState {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Closed,
            2 => Self::HalfOpen,
            1 | _ => Self::Open, // Default to safest state
        }
    }
}

/// Circuit breaker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Number of failures required to open the circuit
    pub failure_threshold: u64,
    /// Number of requests in half-open state before deciding to close circuit
    pub success_threshold: u64,
    /// Duration to wait before transitioning from open to half-open
    pub timeout_duration: Duration,
    /// Rolling window duration for failure counting
    pub rolling_window: Duration,
    /// Minimum number of requests in window before considering failure rate
    pub minimum_requests: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            timeout_duration: Duration::from_secs(30),
            rolling_window: Duration::from_secs(60),
            minimum_requests: 10,
        }
    }
}

impl CircuitBreakerConfig {
    /// Create a conservative configuration for critical operations
    pub const fn conservative() -> Self {
        Self {
            failure_threshold: 3,
            success_threshold: 5,
            timeout_duration: Duration::from_secs(60),
            rolling_window: Duration::from_secs(120),
            minimum_requests: 5,
        }
    }

    /// Create an aggressive configuration for non-critical operations
    pub const fn aggressive() -> Self {
        Self {
            failure_threshold: 10,
            success_threshold: 2,
            timeout_duration: Duration::from_secs(10),
            rolling_window: Duration::from_secs(30),
            minimum_requests: 20,
        }
    }
}

/// Errors related to circuit breaker operation
#[derive(Debug, Error)]
pub enum CircuitBreakerError {
    /// Circuit breaker is open, operation rejected
    #[error("Circuit breaker is open, operation rejected. Last failure: {last_failure:?}")]
    Open {
        /// Reason for the last failure that caused the circuit to open
        last_failure: Option<String>,
    },

    /// Operation failed and circuit breaker recorded the failure
    #[error("Operation failed: {source}")]
    OperationFailed {
        /// The underlying error that caused the operation to fail
        #[source]
        source: PostgresError,
    },
}

impl From<CircuitBreakerError> for PostgresError {
    fn from(error: CircuitBreakerError) -> Self {
        match error {
            CircuitBreakerError::Open { .. } => Self::Connection(sqlx::Error::PoolClosed),
            CircuitBreakerError::OperationFailed { source } => source,
        }
    }
}

/// Sliding window for tracking request metrics
#[derive(Debug)]
struct SlidingWindow {
    requests: RwLock<Vec<(Instant, bool)>>, // (timestamp, was_success)
    window_duration: Duration,
}

impl SlidingWindow {
    fn new(window_duration: Duration) -> Self {
        Self {
            requests: RwLock::new(Vec::new()),
            window_duration,
        }
    }

    async fn record_request(&self, success: bool) {
        let now = Instant::now();
        let mut requests = self.requests.write().await;

        // Add new request
        requests.push((now, success));

        // Clean old requests outside the window
        let cutoff = now.checked_sub(self.window_duration).unwrap();
        requests.retain(|(timestamp, _)| *timestamp > cutoff);
    }

    async fn get_metrics(&self) -> (u64, u64) {
        let now = Instant::now();
        let cutoff = now.checked_sub(self.window_duration).unwrap();
        let requests = self.requests.read().await;

        let recent_requests: Vec<_> = requests
            .iter()
            .filter(|(timestamp, _)| *timestamp > cutoff)
            .collect();

        let total = recent_requests.len() as u64;
        let failures = recent_requests
            .iter()
            .filter(|(_, success)| !*success)
            .count() as u64;

        (total, failures)
    }
}

/// Circuit breaker implementation
#[derive(Debug)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: AtomicU8, // Uses CircuitState representation
    failure_count: AtomicU64,
    success_count: AtomicU64,
    last_failure_time: RwLock<Option<Instant>>,
    last_failure_reason: RwLock<Option<String>>,
    sliding_window: SlidingWindow,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            sliding_window: SlidingWindow::new(config.rolling_window),
            config,
            state: AtomicU8::new(CircuitState::Closed as u8),
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            last_failure_time: RwLock::new(None),
            last_failure_reason: RwLock::new(None),
        }
    }

    /// Get current circuit breaker state
    pub fn state(&self) -> CircuitState {
        CircuitState::from(self.state.load(Ordering::Acquire))
    }

    /// Get current metrics
    pub async fn metrics(&self) -> CircuitBreakerMetrics {
        let (total_requests, total_failures) = self.sliding_window.get_metrics().await;
        let failure_rate = if total_requests > 0 {
            total_failures as f64 / total_requests as f64
        } else {
            0.0
        };

        let last_failure_time = *self.last_failure_time.read().await;
        let last_failure_reason = self.last_failure_reason.read().await.clone();

        // Convert Instant to seconds since epoch for serialization
        let last_failure_timestamp = last_failure_time.map(|instant| instant.elapsed().as_secs());

        CircuitBreakerMetrics {
            state: self.state(),
            failure_count: self.failure_count.load(Ordering::Relaxed),
            success_count: self.success_count.load(Ordering::Relaxed),
            total_requests,
            total_failures,
            failure_rate,
            last_failure_time: last_failure_timestamp,
            last_failure_reason,
        }
    }

    /// Execute an operation through the circuit breaker
    #[instrument(skip(self, operation))]
    pub async fn execute<F, Fut, T>(&self, operation: F) -> Result<T, CircuitBreakerError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, PostgresError>>,
    {
        // Check if circuit should allow the request
        if !self.should_allow_request().await {
            let last_failure = self.last_failure_reason.read().await.clone();
            return Err(CircuitBreakerError::Open { last_failure });
        }

        // Execute the operation
        match operation().await {
            Ok(result) => {
                self.record_success().await;
                Ok(result)
            }
            Err(error) => {
                let error_msg = error.to_string();
                self.record_failure(error_msg).await;
                Err(CircuitBreakerError::OperationFailed { source: error })
            }
        }
    }

    /// Check if the circuit should allow a request
    async fn should_allow_request(&self) -> bool {
        match self.state() {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout has elapsed to transition to half-open
                if let Some(last_failure) = *self.last_failure_time.read().await {
                    if last_failure.elapsed() >= self.config.timeout_duration {
                        debug!("Circuit breaker transitioning from Open to HalfOpen");
                        self.transition_to_half_open();
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record a successful operation
    async fn record_success(&self) {
        self.sliding_window.record_request(true).await;

        let current_state = self.state();
        match current_state {
            CircuitState::Closed => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::Relaxed);
            }
            CircuitState::HalfOpen => {
                let success_count = self.success_count.fetch_add(1, Ordering::Relaxed) + 1;
                debug!("Circuit breaker half-open success count: {}", success_count);

                if success_count >= self.config.success_threshold {
                    debug!("Circuit breaker transitioning from HalfOpen to Closed");
                    self.transition_to_closed();
                }
            }
            CircuitState::Open => {
                // Shouldn't happen, but handle gracefully
                warn!("Recorded success while circuit was open");
            }
        }
    }

    /// Record a failed operation
    async fn record_failure(&self, error_msg: String) {
        self.sliding_window.record_request(false).await;

        // Update failure tracking
        *self.last_failure_time.write().await = Some(Instant::now());
        *self.last_failure_reason.write().await = Some(error_msg);

        let current_state = self.state();
        match current_state {
            CircuitState::Closed => {
                let failure_count = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
                debug!("Circuit breaker failure count: {}", failure_count);

                // Check if we should open the circuit based on recent failure rate
                let (total_requests, total_failures) = self.sliding_window.get_metrics().await;

                if total_requests >= self.config.minimum_requests
                    && total_failures >= self.config.failure_threshold
                {
                    warn!(
                        "Circuit breaker opening due to failure threshold. Failures: {}/{}",
                        total_failures, total_requests
                    );
                    self.transition_to_open();
                }
            }
            CircuitState::HalfOpen => {
                debug!("Circuit breaker transitioning from HalfOpen to Open due to failure");
                self.transition_to_open();
            }
            CircuitState::Open => {
                // Already open, just update counters
                self.failure_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Transition to closed state
    fn transition_to_closed(&self) {
        self.state
            .store(CircuitState::Closed as u8, Ordering::Release);
        self.failure_count.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);
        debug!("Circuit breaker state changed to Closed");
    }

    /// Transition to open state
    fn transition_to_open(&self) {
        self.state
            .store(CircuitState::Open as u8, Ordering::Release);
        self.success_count.store(0, Ordering::Relaxed);
        error!("Circuit breaker state changed to Open");
    }

    /// Transition to half-open state
    fn transition_to_half_open(&self) {
        self.state
            .store(CircuitState::HalfOpen as u8, Ordering::Release);
        self.success_count.store(0, Ordering::Relaxed);
        debug!("Circuit breaker state changed to HalfOpen");
    }

    /// Manually reset the circuit breaker to closed state
    pub async fn reset(&self) {
        debug!("Manually resetting circuit breaker");
        self.transition_to_closed();
        *self.last_failure_time.write().await = None;
        *self.last_failure_reason.write().await = None;
    }

    /// Force the circuit breaker to open (for testing or manual intervention)
    pub async fn force_open(&self) {
        warn!("Manually forcing circuit breaker to open state");
        self.transition_to_open();
        *self.last_failure_time.write().await = Some(Instant::now());
        *self.last_failure_reason.write().await = Some("Manually forced open".to_string());
    }
}

/// Circuit breaker metrics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerMetrics {
    /// Current state of the circuit breaker
    pub state: CircuitState,
    /// Total number of failures
    pub failure_count: u64,
    /// Total number of successes in current state
    pub success_count: u64,
    /// Total requests in the rolling window
    pub total_requests: u64,
    /// Total failures in the rolling window
    pub total_failures: u64,
    /// Current failure rate (0.0 to 1.0)
    pub failure_rate: f64,
    /// Timestamp of last failure (as seconds since epoch)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failure_time: Option<u64>,
    /// Reason for last failure
    pub last_failure_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_closed_to_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 3,
            minimum_requests: 3,
            ..CircuitBreakerConfig::default()
        };

        let breaker = CircuitBreaker::new(config);
        assert_eq!(breaker.state(), CircuitState::Closed);

        // Simulate failures
        for i in 0..3 {
            let result = breaker
                .execute(|| async {
                    Err::<(), _>(PostgresError::Connection(sqlx::Error::PoolTimedOut))
                })
                .await;

            assert!(result.is_err());

            if i < 2 {
                assert_eq!(breaker.state(), CircuitState::Closed);
            } else {
                assert_eq!(breaker.state(), CircuitState::Open);
            }
        }
    }

    #[tokio::test]
    async fn test_circuit_breaker_open_to_half_open() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            minimum_requests: 1,
            timeout_duration: Duration::from_millis(50),
            ..CircuitBreakerConfig::default()
        };

        let breaker = CircuitBreaker::new(config);

        // Cause failure to open circuit
        let _ = breaker
            .execute(|| async {
                Err::<(), _>(PostgresError::Connection(sqlx::Error::PoolTimedOut))
            })
            .await;

        assert_eq!(breaker.state(), CircuitState::Open);

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(60)).await;

        // Next request should transition to half-open
        let result = breaker
            .execute(|| async { Ok::<(), PostgresError>(()) })
            .await;

        assert!(result.is_ok());
        // After first success in half-open, should still be half-open (need success_threshold successes)
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_to_closed() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            minimum_requests: 1,
            success_threshold: 2,
            timeout_duration: Duration::from_millis(50),
            ..CircuitBreakerConfig::default()
        };

        let breaker = CircuitBreaker::new(config);

        // Open the circuit
        let _ = breaker
            .execute(|| async {
                Err::<(), _>(PostgresError::Connection(sqlx::Error::PoolTimedOut))
            })
            .await;

        // Wait and execute successful operations
        tokio::time::sleep(Duration::from_millis(60)).await;

        // First success (should be half-open)
        let _ = breaker
            .execute(|| async { Ok::<(), PostgresError>(()) })
            .await;

        // Second success (should close circuit)
        let _ = breaker
            .execute(|| async { Ok::<(), PostgresError>(()) })
            .await;

        assert_eq!(breaker.state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_metrics() {
        let breaker = CircuitBreaker::new(CircuitBreakerConfig::default());

        // Execute some operations
        let _ = breaker
            .execute(|| async { Ok::<(), PostgresError>(()) })
            .await;

        let _ = breaker
            .execute(|| async {
                Err::<(), _>(PostgresError::Connection(sqlx::Error::PoolTimedOut))
            })
            .await;

        let metrics = breaker.metrics().await;
        assert_eq!(metrics.state, CircuitState::Closed);
        assert_eq!(metrics.total_requests, 2);
        assert_eq!(metrics.total_failures, 1);
        assert_eq!(metrics.failure_rate, 0.5);
    }

    #[tokio::test]
    async fn test_circuit_breaker_reset() {
        let config = CircuitBreakerConfig {
            failure_threshold: 1,
            minimum_requests: 1,
            ..CircuitBreakerConfig::default()
        };

        let breaker = CircuitBreaker::new(config);

        // Open the circuit
        let _ = breaker
            .execute(|| async {
                Err::<(), _>(PostgresError::Connection(sqlx::Error::PoolTimedOut))
            })
            .await;

        assert_eq!(breaker.state(), CircuitState::Open);

        // Reset the circuit
        breaker.reset().await;
        assert_eq!(breaker.state(), CircuitState::Closed);

        let metrics = breaker.metrics().await;
        assert!(metrics.last_failure_time.is_none());
        assert!(metrics.last_failure_reason.is_none());
    }

    #[tokio::test]
    async fn test_sliding_window() {
        let window = SlidingWindow::new(Duration::from_millis(100));

        // Record some requests
        window.record_request(true).await;
        window.record_request(false).await;
        window.record_request(true).await;

        let (total, failures) = window.get_metrics().await;
        assert_eq!(total, 3);
        assert_eq!(failures, 1);

        // Wait for window to expire
        tokio::time::sleep(Duration::from_millis(150)).await;

        let (total, failures) = window.get_metrics().await;
        assert_eq!(total, 0);
        assert_eq!(failures, 0);
    }
}
