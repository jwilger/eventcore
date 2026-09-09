use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use eventcore_types::{BatchSize, ProjectionSelection};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// Asynchronous delay used between transactional projection retry attempts.
///
/// Applications normally use the default Tokio-backed implementation. The abstraction permits
/// deterministic observation of requested retry delays without pausing the Tokio runtime that
/// also drives live database I/O.
pub trait ProjectionRetrySleeper: std::fmt::Debug + Send + Sync {
    /// Returns a future that completes after the requested retry delay.
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// Default retry sleeper backed by [`tokio::time::sleep`].
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioProjectionRetrySleeper;

impl ProjectionRetrySleeper for TokioProjectionRetrySleeper {
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(tokio::time::sleep(duration))
    }
}

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
        if max_retries == u32::MAX {
            return Err(ProjectionConfigurationError::TooManyRetries);
        }
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
    /// The retry count must leave room for the one-based initial attempt.
    #[error("retry count is too large to represent the initial attempt plus retries")]
    TooManyRetries,
}

/// Settings for a transactional PostgreSQL projection run.
#[derive(Debug, Clone)]
pub struct PostgresProjectionConfig {
    selection: ProjectionSelection,
    batch_size: BatchSize,
    mode: PostgresProjectionMode,
    retry_policy: ProjectionRetryPolicy,
    retry_sleeper: Arc<dyn ProjectionRetrySleeper>,
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
            retry_sleeper: Arc::new(TokioProjectionRetrySleeper),
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

    /// Replaces the asynchronous delay implementation used between retry attempts.
    pub fn with_retry_sleeper(
        mut self,
        retry_sleeper: impl ProjectionRetrySleeper + 'static,
    ) -> Self {
        self.retry_sleeper = Arc::new(retry_sleeper);
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

    /// Returns the configured retry delay implementation.
    pub fn retry_sleeper(&self) -> &(dyn ProjectionRetrySleeper + 'static) {
        self.retry_sleeper.as_ref()
    }

    /// Returns the interval between empty continuous polls.
    pub fn continuous_poll_interval(&self) -> Duration {
        self.continuous_poll_interval
    }
}
