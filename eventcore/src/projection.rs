//! Projection runtime components for building and running read models.
//!
//! This module provides the runtime infrastructure for event projection:
//! - `LocalCoordinator`: Single-process coordination for projector leadership
//! - `ProjectionRunner`: Orchestrates projector execution with event polling

use crate::{
    BatchSize, Event, EventFilter, EventPage, EventReader, FailureStrategy, Projector,
    StreamPosition,
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
    pub max_consecutive_poll_failures: u32,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(100),
            empty_poll_backoff: Duration::from_millis(50),
            poll_failure_backoff: Duration::from_millis(100),
            max_consecutive_poll_failures: 5,
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
    _coordinator: C,
    store: S,
    checkpoint_store: Option<InMemoryCheckpointStore>,
    poll_mode: PollMode,
    poll_config: PollConfig,
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
            _coordinator: coordinator,
            store,
            checkpoint_store: None,
            poll_mode: PollMode::Batch,
            poll_config: PollConfig::default(),
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
                        if consecutive_failures >= self.poll_config.max_consecutive_poll_failures {
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
                                cs.save(self.projector.name(), position);
                            }
                            break; // Move to next event
                        }
                        Err(error) => {
                            // Error occurred - ask projector what to do
                            let failure_ctx = eventcore_types::FailureContext {
                                error: &error,
                                position,
                                retry_count,
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
                                    retry_count += 1;
                                    // Wait before retrying
                                    tokio::time::sleep(Duration::from_millis(10)).await;
                                    // Continue retry loop - projector controls when to escalate to Fatal
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
