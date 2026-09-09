use std::time::Duration;

use eventcore_types::{BatchSize, ProjectionSelection};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// Controls retry behavior after an application requests a retry.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectionRetryPolicy {
    max_retries: u32,
    initial_delay: Duration,
    multiplier: f64,
    maximum_delay: Duration,
}

impl ProjectionRetryPolicy {
    /// Creates a bounded retry policy.
    pub fn new(
        max_retries: u32,
        initial_delay: Duration,
        multiplier: f64,
        maximum_delay: Duration,
    ) -> Result<Self, ProjectionConfigurationError> {
        if !multiplier.is_finite() || multiplier < 1.0 {
            return Err(ProjectionConfigurationError::InvalidRetryMultiplier);
        }

        Ok(Self {
            max_retries,
            initial_delay,
            multiplier,
            maximum_delay,
        })
    }

    /// Returns the number of retries after the initial attempt.
    pub fn max_retries(&self) -> u32 {
        self.max_retries
    }

    /// Returns the delay before the first retry.
    pub fn initial_delay(&self) -> Duration {
        self.initial_delay
    }

    /// Returns the multiplier used to increase retry delays.
    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    /// Returns the maximum retry delay.
    pub fn maximum_delay(&self) -> Duration {
        self.maximum_delay
    }
}

/// Selects whether a runner stops at its initial catch-up frontier or keeps polling.
#[derive(Debug, Clone)]
pub enum PostgresProjectionMode {
    /// Drain the committed source frontier captured at run start, then return.
    Batch,
    /// Keep polling after each catch-up frontier until cancellation.
    Continuous(CancellationToken),
}

/// Configuration rejected before a runner is started.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProjectionConfigurationError {
    /// A continuous projection must wait for a positive duration between empty polls.
    #[error("continuous poll interval must be positive")]
    ZeroContinuousPollInterval,
    /// Retry multipliers must be finite and at least one.
    #[error("retry multiplier must be finite and at least one")]
    InvalidRetryMultiplier,
}

/// Settings for a transactional PostgreSQL projection run.
#[derive(Debug, Clone)]
pub struct PostgresProjectionConfig {
    selection: ProjectionSelection,
    batch_size: BatchSize,
    mode: PostgresProjectionMode,
    retry_policy: ProjectionRetryPolicy,
    continuous_poll_interval: Duration,
}

impl PostgresProjectionConfig {
    /// Creates batch-mode configuration with the documented defaults.
    pub fn new(selection: ProjectionSelection) -> Self {
        Self {
            selection,
            batch_size: BatchSize::new(100),
            mode: PostgresProjectionMode::Batch,
            retry_policy: ProjectionRetryPolicy {
                max_retries: 0,
                initial_delay: Duration::from_millis(100),
                multiplier: 2.0,
                maximum_delay: Duration::from_secs(30),
            },
            continuous_poll_interval: Duration::from_secs(1),
        }
    }

    /// Changes this configuration to continuously poll until the token is cancelled.
    pub fn continuous(mut self, cancellation: CancellationToken) -> Self {
        self.mode = PostgresProjectionMode::Continuous(cancellation);
        self
    }

    /// Replaces the maximum number of source envelopes read in one page.
    pub fn with_batch_size(mut self, batch_size: BatchSize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Replaces the retry policy.
    pub fn with_retry_policy(mut self, retry_policy: ProjectionRetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    /// Replaces the interval between empty continuous polls.
    pub fn with_continuous_poll_interval(
        mut self,
        continuous_poll_interval: Duration,
    ) -> Result<Self, ProjectionConfigurationError> {
        if continuous_poll_interval.is_zero() {
            return Err(ProjectionConfigurationError::ZeroContinuousPollInterval);
        }

        self.continuous_poll_interval = continuous_poll_interval;
        Ok(self)
    }

    /// Returns the selected source contract.
    pub fn selection(&self) -> &ProjectionSelection {
        &self.selection
    }

    /// Returns the page size.
    pub fn batch_size(&self) -> BatchSize {
        self.batch_size
    }

    /// Returns the execution mode.
    pub fn mode(&self) -> &PostgresProjectionMode {
        &self.mode
    }

    /// Returns the retry policy.
    pub fn retry_policy(&self) -> &ProjectionRetryPolicy {
        &self.retry_policy
    }

    /// Returns the interval between empty continuous polls.
    pub fn continuous_poll_interval(&self) -> Duration {
        self.continuous_poll_interval
    }
}
