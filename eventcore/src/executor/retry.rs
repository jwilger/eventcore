//! Retry logic and policies for command execution.
//! 
//! This module contains the retry configuration and policies used to handle
//! transient failures and concurrency conflicts during command execution.

use crate::errors::CommandError;
use std::time::Duration;

/// Configuration for command execution retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts.
    pub max_attempts: u32,
    /// Base delay between retry attempts.
    pub base_delay: Duration,
    /// Maximum delay between retry attempts (for exponential backoff).
    pub max_delay: Duration,
    /// Multiplier for exponential backoff.
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
}

/// Policy defining which errors should trigger a retry.
#[derive(Debug, Clone)]
pub enum RetryPolicy {
    /// Only retry on concurrency conflicts.
    ConcurrencyConflictsOnly,
    /// Retry on concurrency conflicts and transient errors.
    ConcurrencyAndTransient,
    /// Custom policy with user-defined predicate.
    Custom(fn(&CommandError) -> bool),
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::ConcurrencyConflictsOnly
    }
}

impl RetryPolicy {
    /// Determines if an error should trigger a retry.
    pub fn should_retry(&self, error: &CommandError) -> bool {
        match self {
            Self::ConcurrencyConflictsOnly => {
                matches!(error, CommandError::ConcurrencyConflict { .. })
            }
            Self::ConcurrencyAndTransient => {
                matches!(
                    error,
                    CommandError::ConcurrencyConflict { .. } | CommandError::StreamNotFound(_)
                )
            }
            Self::Custom(predicate) => predicate(error),
        }
    }
}