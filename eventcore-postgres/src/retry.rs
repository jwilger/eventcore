//! Retry strategies and utilities for `PostgreSQL` operations
//!
//! This module provides robust retry mechanisms with exponential backoff
//! for handling transient database failures.

#![allow(clippy::option_if_let_else)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::match_same_arms)]

use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, warn};

use crate::PostgresError;

/// Retry strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryStrategy {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Base delay between attempts
    pub base_delay: Duration,
    /// Maximum delay (exponential backoff cap)
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Whether to add jitter to prevent thundering herd
    pub use_jitter: bool,
}

impl Default for RetryStrategy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 2.0,
            use_jitter: true,
        }
    }
}

impl RetryStrategy {
    /// Create a conservative retry strategy for critical operations
    pub const fn conservative() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 1.5,
            use_jitter: true,
        }
    }

    /// Create an aggressive retry strategy for non-critical operations
    pub const fn aggressive() -> Self {
        Self {
            max_attempts: 2,
            base_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(2),
            backoff_multiplier: 3.0,
            use_jitter: false,
        }
    }

    /// Calculate delay for a given attempt number
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }

        let delay_ms =
            self.base_delay.as_millis() as f64 * self.backoff_multiplier.powi((attempt - 1) as i32);

        let delay = Duration::from_millis(delay_ms as u64);
        let capped_delay = std::cmp::min(delay, self.max_delay);

        if self.use_jitter {
            add_jitter(capped_delay)
        } else {
            capped_delay
        }
    }
}

/// Add random jitter to prevent thundering herd effect
fn add_jitter(delay: Duration) -> Duration {
    use rand::Rng;
    let jitter_factor = rand::thread_rng().gen_range(0.8..1.2);
    let jittered_ms = (delay.as_millis() as f64 * jitter_factor) as u64;
    Duration::from_millis(jittered_ms)
}

/// Errors that can occur during retry operations
#[derive(Debug, Error)]
pub enum RetryError {
    /// All retry attempts exhausted
    #[error("All retry attempts exhausted after {attempts} tries. Last error: {last_error}")]
    ExhaustedAttempts {
        /// Number of attempts made
        attempts: u32,
        /// The last error encountered
        last_error: PostgresError,
    },

    /// Non-retryable error encountered
    #[error("Non-retryable error: {0}")]
    NonRetryable(PostgresError),
}

impl From<RetryError> for PostgresError {
    fn from(error: RetryError) -> Self {
        match error {
            RetryError::ExhaustedAttempts { last_error, .. } => last_error,
            RetryError::NonRetryable(error) => error,
        }
    }
}

/// Determines if an error is retryable
pub fn is_retryable_error(error: &PostgresError) -> bool {
    match error {
        PostgresError::Connection(sqlx_error) => {
            use sqlx::Error;
            match sqlx_error {
                // Connection issues are retryable
                Error::Io(_) | Error::Protocol(_) | Error::PoolTimedOut | Error::PoolClosed => true,
                // Database errors might be retryable depending on the type
                Error::Database(db_err) => {
                    if let Some(code) = db_err.code() {
                        // PostgreSQL error codes that are retryable
                        matches!(
                            code.as_ref(),
                            "40001" | // serialization_failure
                            "40P01" | // deadlock_detected  
                            "53300" | // too_many_connections
                            "08000" | // connection_exception
                            "08003" | // connection_does_not_exist
                            "08006" | // connection_failure
                            "08001" | // sqlclient_unable_to_establish_sqlconnection
                            "08004" // sqlserver_rejected_establishment_of_sqlconnection
                        )
                    } else {
                        false
                    }
                }
                // Configuration and other errors are not retryable
                _ => false,
            }
        }
        PostgresError::PoolCreation(_) => true, // Pool creation can be retried
        PostgresError::Transaction(_) => true,  // Transaction errors might be temporary
        PostgresError::Migration(_) => false,   // Migration errors are not retryable
        PostgresError::Serialization(_) => false, // Serialization errors are not retryable
    }
}

/// Execute an operation with retry logic
pub async fn retry_operation<F, Fut, T, E>(
    strategy: &RetryStrategy,
    operation_name: &str,
    mut operation: F,
) -> Result<T, RetryError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: Into<PostgresError> + std::fmt::Debug,
{
    let mut last_error = None;

    for attempt in 0..strategy.max_attempts {
        match operation().await {
            Ok(result) => {
                if attempt > 0 {
                    debug!(
                        "Operation '{}' succeeded on attempt {} after retries",
                        operation_name,
                        attempt + 1
                    );
                }
                return Ok(result);
            }
            Err(error) => {
                let postgres_error = error.into();

                // Check if this error is retryable
                if !is_retryable_error(&postgres_error) {
                    warn!(
                        "Operation '{}' failed with non-retryable error: {:?}",
                        operation_name, postgres_error
                    );
                    return Err(RetryError::NonRetryable(postgres_error));
                }

                last_error = Some(postgres_error);

                // If this is not the last attempt, wait before retrying
                if attempt < strategy.max_attempts - 1 {
                    let delay = strategy.calculate_delay(attempt + 1);
                    warn!(
                        "Operation '{}' failed on attempt {}, retrying in {:?}. Error: {:?}",
                        operation_name,
                        attempt + 1,
                        delay,
                        last_error.as_ref().unwrap()
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    // All attempts exhausted
    let final_error = last_error.expect("Should have at least one error");
    Err(RetryError::ExhaustedAttempts {
        attempts: strategy.max_attempts,
        last_error: final_error,
    })
}

/// Macro for retrying database operations with default strategy
#[macro_export]
macro_rules! retry_db_operation {
    ($strategy:expr, $operation_name:expr, $operation:expr) => {
        $crate::retry::retry_operation($strategy, $operation_name, || async { $operation }).await
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_retry_strategy_delay_calculation() {
        let strategy = RetryStrategy {
            max_attempts: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
            backoff_multiplier: 2.0,
            use_jitter: false,
        };

        // First attempt should have no delay
        assert_eq!(strategy.calculate_delay(0), Duration::ZERO);

        // Second attempt: base delay
        assert_eq!(strategy.calculate_delay(1), Duration::from_millis(100));

        // Third attempt: base * 2
        assert_eq!(strategy.calculate_delay(2), Duration::from_millis(200));

        // Fourth attempt: base * 4
        assert_eq!(strategy.calculate_delay(3), Duration::from_millis(400));

        // Fifth attempt: base * 8 = 800ms, but capped at max_delay (2s)
        assert_eq!(strategy.calculate_delay(4), Duration::from_millis(800));
    }

    #[test]
    fn test_retry_strategy_presets() {
        let conservative = RetryStrategy::conservative();
        assert_eq!(conservative.max_attempts, 5);
        assert!(conservative.use_jitter);

        let aggressive = RetryStrategy::aggressive();
        assert_eq!(aggressive.max_attempts, 2);
        assert!(!aggressive.use_jitter);
    }

    #[tokio::test]
    async fn test_retry_operation_success_first_attempt() {
        let strategy = RetryStrategy::default();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let result = retry_operation(&strategy, "test_operation", || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok::<i32, PostgresError>(42)
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_operation_success_after_retries() {
        let strategy = RetryStrategy {
            max_attempts: 3,
            base_delay: Duration::from_millis(1), // Fast for testing
            max_delay: Duration::from_millis(10),
            backoff_multiplier: 2.0,
            use_jitter: false,
        };

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let result = retry_operation(&strategy, "test_operation", || {
            let counter = Arc::clone(&counter_clone);
            async move {
                let count = counter.fetch_add(1, Ordering::SeqCst);
                if count < 2 {
                    // Fail first two attempts
                    Err(PostgresError::Connection(sqlx::Error::PoolTimedOut))
                } else {
                    // Succeed on third attempt
                    Ok::<i32, PostgresError>(42)
                }
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_operation_exhausted_attempts() {
        let strategy = RetryStrategy {
            max_attempts: 2,
            base_delay: Duration::from_millis(1), // Fast for testing
            max_delay: Duration::from_millis(10),
            backoff_multiplier: 2.0,
            use_jitter: false,
        };

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let result = retry_operation(&strategy, "test_operation", || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Err::<i32, PostgresError>(PostgresError::Connection(sqlx::Error::PoolTimedOut))
            }
        })
        .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RetryError::ExhaustedAttempts { attempts: 2, .. }
        ));
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_is_retryable_error() {
        // Retryable errors
        assert!(is_retryable_error(&PostgresError::Connection(
            sqlx::Error::PoolTimedOut
        )));
        assert!(is_retryable_error(&PostgresError::PoolCreation(
            "test".to_string()
        )));
        assert!(is_retryable_error(&PostgresError::Transaction(
            "test".to_string()
        )));

        // Non-retryable errors
        assert!(!is_retryable_error(&PostgresError::Migration(
            "test".to_string()
        )));
        assert!(!is_retryable_error(&PostgresError::Serialization(
            serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::Other, "test"))
        )));
    }
}
