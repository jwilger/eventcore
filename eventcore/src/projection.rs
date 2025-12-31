//! Projection runtime components for building and running read models.
//!
//! This module provides the runtime infrastructure for event projection:
//! - `ProjectionRunner`: Orchestrates projector execution with event polling

use crate::{
    BackoffMultiplier, BatchSize, CheckpointStore, Event, EventFilter, EventPage, EventReader,
    FailureStrategy, MaxConsecutiveFailures, MaxRetryAttempts, Projector, StreamPosition,
};
use std::time::Duration;

/// Configuration for projection polling behavior.
///
/// `PollConfig` controls how the projection runner polls for new events,
/// including intervals between polls and backoff strategies for empty results
/// or failures.
///
/// # Example
///
/// ```ignore
/// let config = PollConfig::default();
/// let runner = ProjectionRunner::new(projector, &store)
///     .with_poll_config(config);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollConfig {
    /// Interval between polls when events are available.
    pub poll_interval: Duration,
    /// Additional backoff delay when no events are found.
    pub empty_poll_backoff: Duration,
    /// Additional backoff delay after a poll failure.
    pub poll_failure_backoff: Duration,
    /// Maximum consecutive poll failures before stopping.
    pub max_consecutive_poll_failures: MaxConsecutiveFailures,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(100),
            empty_poll_backoff: Duration::from_millis(50),
            poll_failure_backoff: Duration::from_millis(100),
            max_consecutive_poll_failures: MaxConsecutiveFailures::new(
                std::num::NonZeroU32::new(5).expect("5 is non-zero"),
            ),
        }
    }
}

/// Configuration for event retry behavior (application level).
///
/// `EventRetryConfig` controls HOW retries work when a projector's `on_error()`
/// callback returns `FailureStrategy::Retry`. The projector decides WHETHER to
/// retry; this configuration controls the retry mechanics.
///
/// Per ADR-024, event retry is an application-level concern, separate from
/// poll retry (infrastructure).
///
/// # Example
///
/// ```ignore
/// let retry_config = EventRetryConfig {
///     max_retry_attempts: 3,
///     retry_delay: Duration::from_millis(100),
///     retry_backoff_multiplier: 2.0,
///     max_retry_delay: Duration::from_secs(5),
/// };
/// let runner = ProjectionRunner::new(projector, &store)
///     .with_event_retry_config(retry_config);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct EventRetryConfig {
    /// Maximum number of retry attempts before escalating to Fatal.
    pub max_retry_attempts: MaxRetryAttempts,
    /// Initial delay between retry attempts.
    pub retry_delay: Duration,
    /// Multiplier for exponential backoff (e.g., 2.0 doubles delay each retry).
    pub retry_backoff_multiplier: BackoffMultiplier,
    /// Maximum delay between retry attempts (caps exponential growth).
    pub max_retry_delay: Duration,
}

impl Default for EventRetryConfig {
    fn default() -> Self {
        Self {
            max_retry_attempts: MaxRetryAttempts::new(3),
            retry_delay: Duration::from_millis(100),
            retry_backoff_multiplier: BackoffMultiplier::try_new(2.0)
                .expect("2.0 is a valid BackoffMultiplier value"),
            max_retry_delay: Duration::from_secs(5),
        }
    }
}

/// Polling mode for projection runners.
///
/// Controls how the projection runner polls for new events:
/// - `Batch`: Process all available events then stop
/// - `Continuous`: Keep polling for new events until stopped
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollMode {
    /// Process available events once then stop.
    Batch,
    /// Continuously poll for new events until stopped.
    Continuous,
}

/// Orchestrates projector execution with event polling.
///
/// **Note:** For most use cases, prefer the [`run_projection`] free function which
/// provides a simpler API and automatic leadership coordination via `ProjectorCoordinator`.
///
/// `ProjectionRunner` is the low-level building block for running projections. It:
/// - Polls the event store for new events
/// - Applies events to the projector in order
/// - Handles errors according to the projector's error strategy
/// - Checkpoints progress for resumable processing
///
/// Use `ProjectionRunner` directly only when you need fine-grained control over
/// polling configuration, event retry behavior, or when not using leadership coordination.
///
/// # Type Parameters
///
/// - `E`: The event type implementing [`Event`]
/// - `R`: The event reader type implementing [`EventReader`]
/// - `P`: The projector type implementing [`Projector`]
/// - `C`: The checkpoint store type implementing [`CheckpointStore`]
///
/// # Example
///
/// ```ignore
/// // Preferred: Use run_projection for simple cases with automatic coordination
/// run_projection(projector, &backend).await?;
///
/// // Advanced: Use ProjectionRunner for custom configuration
/// let runner = ProjectionRunner::new(projector, &store)
///     .with_poll_config(custom_config)
///     .with_event_retry_config(retry_config);
/// runner.run().await?;
/// ```
pub struct ProjectionRunner<E, R, P, C>
where
    E: Event,
    R: EventReader,
    P: Projector<Event = E>,
    C: CheckpointStore,
{
    projector: P,
    store: R,
    checkpoint_store: Option<C>,
    poll_mode: PollMode,
    poll_config: PollConfig,
    event_retry_config: EventRetryConfig,
    _event: std::marker::PhantomData<E>,
}

/// A no-op checkpoint store that never saves or loads checkpoints.
///
/// Used as the default checkpoint store type when no checkpoint store is configured.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCheckpointStore;

/// Error type for NoCheckpointStore (never actually returned).
#[derive(Debug, Clone, Copy)]
pub struct NoCheckpointError;

impl std::fmt::Display for NoCheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no checkpoint store configured")
    }
}

impl std::error::Error for NoCheckpointError {}

impl CheckpointStore for NoCheckpointStore {
    type Error = NoCheckpointError;

    async fn load(&self, _name: &str) -> Result<Option<StreamPosition>, Self::Error> {
        Ok(None)
    }

    async fn save(&self, _name: &str, _position: StreamPosition) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<P, R> ProjectionRunner<P::Event, R, P, NoCheckpointStore>
where
    P: Projector,
    P::Event: Event + Clone,
    P::Context: Default,
    R: EventReader,
{
    /// Create a new projection runner without checkpoint support.
    ///
    /// # Parameters
    ///
    /// - `projector`: The projector that will process events
    /// - `store`: The event store to poll for events
    ///
    /// # Returns
    ///
    /// A new `ProjectionRunner` ready to be started with `run()`.
    pub fn new(projector: P, store: R) -> Self {
        Self {
            projector,
            store,
            checkpoint_store: None,
            poll_mode: PollMode::Batch,
            poll_config: PollConfig::default(),
            event_retry_config: EventRetryConfig::default(),
            _event: std::marker::PhantomData,
        }
    }

    /// Configure a checkpoint store for resumable processing.
    ///
    /// When a checkpoint store is configured, the runner will:
    /// - Load the last checkpoint position on startup
    /// - Only process events after the checkpoint position
    /// - Save checkpoint positions after successful event processing
    ///
    /// # Parameters
    ///
    /// - `checkpoint_store`: The checkpoint store for saving/loading positions
    ///
    /// # Returns
    ///
    /// A new runner with the checkpoint store configured.
    pub fn with_checkpoint_store<C: CheckpointStore>(
        self,
        checkpoint_store: C,
    ) -> ProjectionRunner<P::Event, R, P, C> {
        ProjectionRunner {
            projector: self.projector,
            store: self.store,
            checkpoint_store: Some(checkpoint_store),
            poll_mode: self.poll_mode,
            poll_config: self.poll_config,
            event_retry_config: self.event_retry_config,
            _event: std::marker::PhantomData,
        }
    }
}

impl<E, R, P, C> ProjectionRunner<E, R, P, C>
where
    E: Event + Clone,
    R: EventReader,
    P: Projector<Event = E>,
    P::Context: Default,
    C: CheckpointStore,
{
    /// Configure the polling mode for event processing.
    ///
    /// Controls whether the runner processes events once (batch mode) or
    /// continuously polls for new events until stopped (continuous mode).
    ///
    /// # Parameters
    ///
    /// - `mode`: The polling mode (Batch or Continuous)
    ///
    /// # Returns
    ///
    /// Self for method chaining.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let runner = ProjectionRunner::new(projector, &store)
    ///     .with_poll_mode(PollMode::Continuous);
    /// ```
    pub fn with_poll_mode(mut self, mode: PollMode) -> Self {
        self.poll_mode = mode;
        self
    }

    /// Configure polling behavior and backoff strategies.
    ///
    /// Controls how the runner polls for events, including intervals between
    /// polls and backoff delays for empty results or failures.
    ///
    /// # Parameters
    ///
    /// - `config`: The polling configuration
    ///
    /// # Returns
    ///
    /// Self for method chaining.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let config = PollConfig::default();
    /// let runner = ProjectionRunner::new(projector, &store)
    ///     .with_poll_config(config);
    /// ```
    pub fn with_poll_config(mut self, config: PollConfig) -> Self {
        self.poll_config = config;
        self
    }

    /// Configure event retry behavior.
    ///
    /// Controls HOW retries work when the projector's `on_error()` callback
    /// returns `FailureStrategy::Retry`. The projector decides WHETHER to retry;
    /// this configuration controls retry mechanics (delays, backoff, limits).
    ///
    /// Per ADR-024, event retry is application-level configuration, separate
    /// from poll retry (infrastructure).
    ///
    /// # Parameters
    ///
    /// - `config`: The event retry configuration
    ///
    /// # Returns
    ///
    /// Self for method chaining.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let retry_config = EventRetryConfig {
    ///     max_retry_attempts: 5,
    ///     retry_delay: Duration::from_millis(100),
    ///     retry_backoff_multiplier: 2.0,
    ///     max_retry_delay: Duration::from_secs(10),
    /// };
    /// let runner = ProjectionRunner::new(projector, &store)
    ///     .with_event_retry_config(retry_config);
    /// ```
    pub fn with_event_retry_config(mut self, config: EventRetryConfig) -> Self {
        self.event_retry_config = config;
        self
    }

    /// Run the projection, processing events until completion.
    ///
    /// This method:
    /// 1. Polls for events starting from the last checkpoint
    /// 2. Applies each event to the projector
    /// 3. Checkpoints progress after successful processing
    /// 4. Continues until no more events are available
    ///
    /// # Returns
    ///
    /// - `Ok(())`: All available events were processed successfully
    /// - `Err(E)`: An unrecoverable error occurred during projection
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Event store operations fail
    /// - The projector returns a fatal error
    pub async fn run(mut self) -> Result<(), ProjectionError>
    where
        P::Error: std::fmt::Debug,
    {
        // Load checkpoint if checkpoint store is configured
        let mut last_checkpoint = match &self.checkpoint_store {
            Some(cs) => cs.load(self.projector.name()).await.ok().flatten(),
            None => None,
        };

        let mut ctx = P::Context::default();
        let mut consecutive_failures = 0u32;

        loop {
            // Read events from the store with retry logic for transient errors
            let events: Vec<(P::Event, _)> = loop {
                // Attempt to read events
                let filter = EventFilter::all();
                let page = match last_checkpoint {
                    Some(position) => EventPage::after(position, BatchSize::new(1000)),
                    None => EventPage::first(BatchSize::new(1000)),
                };
                let result = self.store.read_events(filter, page).await;

                match result {
                    Ok(events) => {
                        // Success - reset failure counter and return events
                        consecutive_failures = 0;
                        break events;
                    }
                    Err(_) => {
                        // Database error - check if retries exhausted
                        let max_failures: std::num::NonZeroU32 =
                            self.poll_config.max_consecutive_poll_failures.into();
                        if consecutive_failures >= max_failures.get() {
                            // Already failed max_consecutive_poll_failures times, no more retries allowed
                            return Err(ProjectionError::Failed(
                                "failed to read events after max retries".to_string(),
                            ));
                        }

                        // Track this failure and apply backoff
                        consecutive_failures += 1;

                        // Use configured poll failure backoff
                        tokio::time::sleep(self.poll_config.poll_failure_backoff).await;
                        // Continue retry loop
                    }
                }
            };

            // Track whether we found events for poll delay selection
            let found_events = !events.is_empty();

            // Apply each event to the projector
            for (event, position) in events {
                let mut retry_count = 0u32;

                loop {
                    match self.projector.apply(event.clone(), position, &mut ctx) {
                        Ok(()) => {
                            // Event processed successfully - update and save checkpoint
                            last_checkpoint = Some(position);
                            if let Some(cs) = &self.checkpoint_store {
                                // Ignore checkpoint save errors - checkpoint is best-effort
                                let _ = cs.save(self.projector.name(), position).await;
                            }
                            break; // Move to next event
                        }
                        Err(error) => {
                            // Error occurred - ask projector what to do
                            let failure_ctx = eventcore_types::FailureContext {
                                error: &error,
                                position,
                                retry_count: eventcore_types::RetryCount::new(retry_count),
                            };
                            let strategy = self.projector.on_error(failure_ctx);
                            match strategy {
                                FailureStrategy::Fatal => {
                                    // Stop processing and return error
                                    // Checkpoint is already saved up to last successful event
                                    return Err(ProjectionError::Failed(
                                        "projector apply failed".to_string(),
                                    ));
                                }
                                FailureStrategy::Skip => {
                                    // Log the error and continue processing
                                    tracing::warn!(
                                        projector = self.projector.name(),
                                        position = %position,
                                        error = ?error,
                                        "Skipping failed event"
                                    );
                                    // Update checkpoint to skip past this event
                                    //
                                    // IMPORTANT: This permanently skips the failed event across restarts.
                                    // The checkpoint is saved at the current (failed) event position.
                                    // On restart, read_after(position) will skip all events at or before
                                    // this position, meaning the failed event will never be retried.
                                    // This is intentional - Skip is for poison events that should never
                                    // be retried (e.g., malformed data, unrecoverable errors).
                                    last_checkpoint = Some(position);
                                    if let Some(cs) = &self.checkpoint_store {
                                        // Ignore checkpoint save errors - checkpoint is best-effort
                                        let _ = cs.save(self.projector.name(), position).await;
                                    }
                                    break; // Move to next event
                                }
                                FailureStrategy::Retry => {
                                    // Check if we've exceeded max retry attempts
                                    if retry_count
                                        >= self.event_retry_config.max_retry_attempts.into_inner()
                                    {
                                        // Escalate to Fatal after exhausting retries
                                        return Err(ProjectionError::Failed(
                                            "projector apply failed after max retries".to_string(),
                                        ));
                                    }

                                    retry_count += 1;

                                    // Calculate delay with exponential backoff
                                    let base_delay_ms =
                                        self.event_retry_config.retry_delay.as_millis() as f64;
                                    let multiplier = self
                                        .event_retry_config
                                        .retry_backoff_multiplier
                                        .into_inner();
                                    let delay_ms =
                                        base_delay_ms * multiplier.powi(retry_count as i32 - 1);
                                    let delay = Duration::from_millis(delay_ms as u64);

                                    // Cap at max_retry_delay
                                    let capped_delay =
                                        delay.min(self.event_retry_config.max_retry_delay);

                                    // Wait before retrying
                                    tokio::time::sleep(capped_delay).await;
                                    // Continue retry loop
                                }
                            }
                        }
                    }
                }
            }

            // For batch mode, exit after one pass
            if self.poll_mode == PollMode::Batch {
                break;
            }

            // For continuous mode, sleep before next poll
            // Use poll_interval if events were found, empty_poll_backoff if not
            let delay = if found_events {
                self.poll_config.poll_interval
            } else {
                self.poll_config.empty_poll_backoff
            };
            tokio::time::sleep(delay).await;
        }

        Ok(())
    }
}

/// Error type for projection operations.
///
/// Placeholder error type for projection failures. Will be expanded
/// with specific variants as the implementation progresses.
#[derive(thiserror::Error, Debug)]
pub enum ProjectionError {
    /// Generic projection failure.
    #[error("projection failed: {0}")]
    Failed(String),

    /// Leadership acquisition failed.
    #[error("failed to acquire leadership: {0}")]
    LeadershipError(String),
}

/// Runs a projector against a backend that provides events, checkpoints, and coordination.
///
/// This is the primary entry point for running projections in EventCore. It orchestrates:
/// - Leadership acquisition via `ProjectorCoordinator`
/// - Event reading via `EventReader`
/// - Checkpoint management via `CheckpointStore`
///
/// # Arguments
///
/// * `projector` - The projector implementation to run
/// * `backend` - A reference to a backend implementing EventReader, CheckpointStore, and ProjectorCoordinator
///
/// # Returns
///
/// Returns when the projector completes processing all events (batch mode), is cancelled,
/// or encounters a fatal error.
///
/// # Example
///
/// ```ignore
/// // PostgreSQL provides all three traits
/// run_projection(my_projector, &postgres_store).await?;
/// ```
pub async fn run_projection<P, B>(projector: P, backend: &B) -> Result<(), ProjectionError>
where
    P: Projector,
    P::Event: Event + Clone,
    P::Context: Default,
    P::Error: std::fmt::Debug,
    B: EventReader + CheckpointStore + eventcore_types::ProjectorCoordinator,
{
    // Acquire leadership for this projector
    let _guard = backend
        .try_acquire(projector.name())
        .await
        .map_err(|e| ProjectionError::LeadershipError(e.to_string()))?;

    // Build and run the projection using the existing ProjectionRunner
    let runner = ProjectionRunner::new(projector, backend).with_checkpoint_store(backend);

    runner.run().await
}
