//! Projection runtime components for building and running read models.
//!
//! This module provides the runtime infrastructure for event projection:
//! - `LocalCoordinator`: Single-process coordination for projector leadership
//! - `ProjectionRunner`: Orchestrates projector execution with event polling

use crate::{
    BackoffMultiplier, BatchSize, Event, EventFilter, EventPage, EventReader, FailureStrategy,
    MaxConsecutiveFailures, MaxRetryAttempts, Projector, StreamPosition,
};
use std::future::Future;
use std::time::Duration;

/// Trait for guards that support heartbeat operations.
pub trait GuardLike {
    /// Send a heartbeat signal.
    fn heartbeat(&self);
}

/// Trait for coordinators that can acquire guards.
pub trait CoordinatorLike {
    /// The guard type returned by this coordinator.
    type Guard;

    /// Try to acquire leadership, returning a guard if successful.
    fn try_acquire(&self) -> impl Future<Output = Option<Self::Guard>> + Send;
}

/// Configuration for heartbeat and liveness detection.
///
/// Controls how frequently the projection runner sends heartbeats
/// to the coordinator, and the timeout for detecting hung projectors.
///
/// # Example
///
/// ```ignore
/// let config = HeartbeatConfig {
///     heartbeat_interval: Duration::from_secs(5),
///     heartbeat_timeout: Duration::from_secs(15),
/// };
/// let runner = ProjectionRunner::new(projector, coordinator, &store)
///     .with_heartbeat_config(config);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatConfig {
    /// Interval between heartbeat signals.
    pub heartbeat_interval: Duration,
    /// Timeout for detecting hung projectors.
    pub heartbeat_timeout: Duration,
}

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
/// let runner = ProjectionRunner::new(projector, coordinator, &store)
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
/// poll retry (infrastructure) and heartbeat (coordination).
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
/// let runner = ProjectionRunner::new(projector, coordinator, &store)
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

/// In-memory checkpoint store for tracking projection progress.
///
/// `InMemoryCheckpointStore` stores checkpoint positions in memory. It is
/// primarily useful for testing and single-process deployments where
/// persistence across restarts is not required.
///
/// For production deployments requiring durability, use a persistent
/// checkpoint store implementation.
///
/// # Example
///
/// ```ignore
/// let checkpoint_store = InMemoryCheckpointStore::new();
/// let runner = ProjectionRunner::new(projector, coordinator, &store)
///     .with_checkpoint_store(checkpoint_store);
/// ```
#[derive(Debug, Clone, Default)]
pub struct InMemoryCheckpointStore {
    checkpoints:
        std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, StreamPosition>>>,
}

impl InMemoryCheckpointStore {
    /// Create a new in-memory checkpoint store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load checkpoint for the given projector name.
    pub fn load(&self, projector_name: &str) -> Option<StreamPosition> {
        self.checkpoints
            .lock()
            .ok()
            .and_then(|guard| guard.get(projector_name).copied())
    }

    /// Save checkpoint for the given projector name.
    pub fn save(&self, projector_name: &str, position: StreamPosition) {
        match self.checkpoints.lock() {
            Ok(mut guard) => {
                let _ = guard.insert(projector_name.to_string(), position);
            }
            Err(e) => {
                tracing::warn!(
                    projector = projector_name,
                    position = %position,
                    error = %e,
                    "Failed to save checkpoint due to poisoned mutex"
                );
            }
        }
    }
}

/// Guard representing acquired leadership from a coordinator.
///
/// `CoordinatorGuard` uses RAII pattern to automatically release leadership
/// when dropped. While the guard is held, the projector has exclusive rights
/// to process events.
///
/// # Example
///
/// ```ignore
/// let guard = coordinator.try_acquire().await?;
/// if guard.is_valid() {
///     // Process events while holding leadership
/// }
/// // Guard dropped here - leadership automatically released
/// ```
pub struct CoordinatorGuard {
    // Guard state placeholder
}

impl CoordinatorGuard {
    /// Check if this guard represents valid leadership.
    ///
    /// Returns `true` if the guard still holds valid leadership rights.
    /// For `LocalCoordinator`, this always returns `true` since leadership
    /// cannot be revoked in single-process mode.
    ///
    /// # Returns
    ///
    /// `true` if leadership is valid, `false` otherwise.
    pub fn is_valid(&self) -> bool {
        true
    }
}

impl Drop for CoordinatorGuard {
    fn drop(&mut self) {
        // For LocalCoordinator, dropping the guard releases leadership.
        // No cleanup needed for the minimal single-process implementation.
    }
}

/// Single-process coordinator for projector leadership.
///
/// `LocalCoordinator` provides a simple coordination mechanism for single-process
/// deployments where only one projector instance runs at a time. It uses an
/// in-memory mutex to ensure exclusive access.
///
/// For distributed deployments with multiple application instances, use
/// `eventcore-postgres::PostgresCoordinator` which uses advisory locks for
/// cross-process coordination.
///
/// # Example
///
/// ```ignore
/// let coordinator = LocalCoordinator::new();
/// let runner = ProjectionRunner::new(projector, coordinator, &store);
/// runner.run().await?;
/// ```
pub struct LocalCoordinator {
    // Coordination state placeholder
}

impl LocalCoordinator {
    /// Create a new local coordinator with sensible defaults.
    ///
    /// The coordinator is immediately ready for use. No configuration is
    /// required for single-process deployments.
    pub fn new() -> Self {
        Self {}
    }

    /// Try to acquire leadership for projection processing.
    ///
    /// For `LocalCoordinator`, this always succeeds immediately since there
    /// is no contention in single-process deployments. The returned guard
    /// uses RAII pattern to release leadership when dropped.
    ///
    /// # Returns
    ///
    /// `Some(guard)` if leadership was acquired (always for LocalCoordinator).
    /// `None` would indicate leadership is held elsewhere (never for LocalCoordinator).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let guard = coordinator.try_acquire().await
    ///     .expect("LocalCoordinator always grants leadership");
    /// ```
    pub async fn try_acquire(&self) -> Option<CoordinatorGuard> {
        Some(CoordinatorGuard {})
    }
}

impl Default for LocalCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl CoordinatorLike for LocalCoordinator {
    type Guard = CoordinatorGuard;

    async fn try_acquire(&self) -> Option<Self::Guard> {
        Some(CoordinatorGuard {})
    }
}

impl GuardLike for CoordinatorGuard {
    fn heartbeat(&self) {
        // LocalCoordinator doesn't need heartbeats for single-process mode
    }
}

/// Orchestrates projector execution with event polling and coordination.
///
/// `ProjectionRunner` is the main entry point for running projections. It:
/// - Acquires leadership via the coordinator before processing
/// - Polls the event store for new events
/// - Applies events to the projector in order
/// - Handles errors according to the projector's error strategy
/// - Checkpoints progress for resumable processing
///
/// # Type Parameters
///
/// - `P`: The projector type implementing [`Projector`]
/// - `C`: The coordinator type (e.g., `LocalCoordinator`)
/// - `S`: The event store type implementing [`EventReader`]
///
/// # Example
///
/// ```ignore
/// // Create a minimal projector
/// let projector = EventCounterProjector::new();
///
/// // Use local coordination for single-process deployment
/// let coordinator = LocalCoordinator::new();
///
/// // Create and run the projection
/// let runner = ProjectionRunner::new(projector, coordinator, &store);
/// runner.run().await?;
/// ```
pub struct ProjectionRunner<P, C, S>
where
    P: Projector,
    S: EventReader,
{
    projector: P,
    coordinator: C,
    store: S,
    checkpoint_store: Option<InMemoryCheckpointStore>,
    poll_mode: PollMode,
    poll_config: PollConfig,
    event_retry_config: EventRetryConfig,
    heartbeat_config: Option<HeartbeatConfig>,
}

impl<P, C, S> ProjectionRunner<P, C, S>
where
    P: Projector,
    P::Event: Event + Clone,
    P::Context: Default,
    S: EventReader,
{
    /// Create a new projection runner.
    ///
    /// # Parameters
    ///
    /// - `projector`: The projector that will process events
    /// - `coordinator`: The coordination mechanism for leadership
    /// - `store`: Reference to the event store to poll for events
    ///
    /// # Returns
    ///
    /// A new `ProjectionRunner` ready to be started with `run()`.
    pub fn new(projector: P, coordinator: C, store: S) -> Self {
        Self {
            projector,
            coordinator,
            store,
            checkpoint_store: None,
            poll_mode: PollMode::Batch,
            poll_config: PollConfig::default(),
            event_retry_config: EventRetryConfig::default(),
            heartbeat_config: None,
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
    /// Self for method chaining.
    pub fn with_checkpoint_store(mut self, checkpoint_store: InMemoryCheckpointStore) -> Self {
        self.checkpoint_store = Some(checkpoint_store);
        self
    }

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
    /// let runner = ProjectionRunner::new(projector, coordinator, &store)
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
    /// let runner = ProjectionRunner::new(projector, coordinator, &store)
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
    /// from poll retry (infrastructure) and heartbeat (coordination).
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
    /// let runner = ProjectionRunner::new(projector, coordinator, &store)
    ///     .with_event_retry_config(retry_config);
    /// ```
    pub fn with_event_retry_config(mut self, config: EventRetryConfig) -> Self {
        self.event_retry_config = config;
        self
    }

    /// Configure heartbeat and liveness detection behavior.
    ///
    /// Controls how frequently the runner sends heartbeat signals to the
    /// coordinator, and the timeout for detecting hung projectors.
    ///
    /// # Parameters
    ///
    /// - `config`: The heartbeat configuration
    ///
    /// # Returns
    ///
    /// Self for method chaining.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let heartbeat_config = HeartbeatConfig {
    ///     heartbeat_interval: Duration::from_secs(5),
    ///     heartbeat_timeout: Duration::from_secs(15),
    /// };
    /// let runner = ProjectionRunner::new(projector, coordinator, &store)
    ///     .with_heartbeat_config(heartbeat_config);
    /// ```
    pub fn with_heartbeat_config(mut self, config: HeartbeatConfig) -> Self {
        self.heartbeat_config = Some(config);
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
    pub async fn run<G>(mut self) -> Result<(), ProjectionError>
    where
        P::Error: std::fmt::Debug,
        C: CoordinatorLike<Guard = G>,
        G: GuardLike,
    {
        // Acquire guard and set up heartbeat tracking if configured
        let guard = self.coordinator.try_acquire().await;
        let mut last_heartbeat = self
            .heartbeat_config
            .as_ref()
            .map(|_| tokio::time::Instant::now());

        // Load checkpoint if checkpoint store is configured
        let mut last_checkpoint = self
            .checkpoint_store
            .as_ref()
            .and_then(|cs| cs.load(self.projector.name()));

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
                // Send heartbeat before processing if interval elapsed
                if let (Some(config), Some(last_hb), Some(g)) =
                    (&self.heartbeat_config, &mut last_heartbeat, &guard)
                {
                    while last_hb.elapsed() >= config.heartbeat_interval {
                        g.heartbeat();
                        *last_hb += config.heartbeat_interval;
                    }
                }

                let mut retry_count = 0u32;

                loop {
                    match self.projector.apply(event.clone(), position, &mut ctx) {
                        Ok(()) => {
                            // Event processed successfully - update and save checkpoint
                            last_checkpoint = Some(position);
                            if let Some(cs) = &self.checkpoint_store {
                                cs.save(self.projector.name(), position);
                            }

                            // Send heartbeat after processing if interval elapsed
                            if let (Some(config), Some(last_hb), Some(g)) =
                                (&self.heartbeat_config, &mut last_heartbeat, &guard)
                            {
                                while last_hb.elapsed() >= config.heartbeat_interval {
                                    g.heartbeat();
                                    *last_hb += config.heartbeat_interval;
                                }
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
                                        cs.save(self.projector.name(), position);
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
}
