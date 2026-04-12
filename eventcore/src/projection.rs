//! Projection runtime components for building and running read models.
//!
//! This module provides the runtime infrastructure for event projection:
//! - `ProjectionRunner`: Orchestrates projector execution with event polling

use eventcore_types::{
    BackoffMultiplier, CheckpointStore, Event, EventReader, MaxConsecutiveFailures,
    MaxRetryAttempts, Projector, StreamPosition,
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
pub(crate) struct PollConfig {
    /// Interval between polls when events are available.
    pub(crate) poll_interval: Duration,
    /// Additional backoff delay when no events are found.
    pub(crate) empty_poll_backoff: Duration,
    /// Additional backoff delay after a poll failure.
    pub(crate) poll_failure_backoff: Duration,
    /// Maximum consecutive poll failures before stopping.
    pub(crate) max_consecutive_poll_failures: MaxConsecutiveFailures,
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
///     max_retry_attempts: MaxRetryAttempts::new(3),
///     retry_delay: Duration::from_millis(100),
///     retry_backoff_multiplier: BackoffMultiplier::try_new(2.0).expect("valid"),
///     max_retry_delay: Duration::from_secs(5),
/// };
/// let runner = ProjectionRunner::new(projector, &store)
///     .with_event_retry_config(retry_config);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EventRetryConfig {
    /// Maximum number of retry attempts before escalating to Fatal.
    pub(crate) max_retry_attempts: MaxRetryAttempts,
    /// Initial delay between retry attempts.
    pub(crate) retry_delay: Duration,
    /// Multiplier for exponential backoff (e.g., 2.0 doubles delay each retry).
    pub(crate) retry_backoff_multiplier: BackoffMultiplier,
    /// Maximum delay between retry attempts (caps exponential growth).
    pub(crate) max_retry_delay: Duration,
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
pub(crate) struct ProjectionRunner<E, R, P, C>
where
    E: Event,
    R: EventReader,
    P: Projector<Event = E>,
    C: CheckpointStore,
{
    projector: P,
    store: R,
    checkpoint_store: Option<C>,
    poll_config: PollConfig,
    event_retry_config: EventRetryConfig,
    _event: std::marker::PhantomData<E>,
}

/// A no-op checkpoint store that never saves or loads checkpoints.
///
/// Used as the default checkpoint store type when no checkpoint store is configured.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NoCheckpointStore;

/// Error type for NoCheckpointStore (never actually returned).
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("no checkpoint store configured")]
pub(crate) struct NoCheckpointError;

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
    pub(crate) fn new(projector: P, store: R) -> Self {
        Self {
            projector,
            store,
            checkpoint_store: None,
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
    pub(crate) fn with_checkpoint_store<C: CheckpointStore>(
        self,
        checkpoint_store: C,
    ) -> ProjectionRunner<P::Event, R, P, C> {
        ProjectionRunner {
            projector: self.projector,
            store: self.store,
            checkpoint_store: Some(checkpoint_store),
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
    /// Run the projection, processing events until completion.
    ///
    /// Internally, this method drives a `ProjectionPipeline` state machine
    /// that yields effects (read events, load/save checkpoint, sleep). This
    /// method is the thin shell loop that dispatches those effects to the
    /// backend traits.
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
    pub(crate) async fn run(self) -> Result<(), ProjectionError>
    where
        P::Error: std::fmt::Debug,
        R::Error: std::fmt::Display,
    {
        use crate::projection_pipeline::{
            ProjectionEffect, ProjectionEffectResult, ProjectionPipeline, ProjectionStep,
        };

        let has_checkpoint_store = self.checkpoint_store.is_some();
        let mut pipeline = ProjectionPipeline::new(
            self.projector,
            has_checkpoint_store,
            self.poll_config,
            self.event_retry_config,
        );
        let mut step = pipeline.step();

        loop {
            match step {
                ProjectionStep::Done(result) => return result,
                ProjectionStep::Yield(ProjectionEffect::LoadCheckpoint { name }) => {
                    let result = match &self.checkpoint_store {
                        Some(cs) => cs.load(&name).await.map_err(|e| e.to_string()),
                        None => Ok(None),
                    };
                    step = pipeline.resume(ProjectionEffectResult::CheckpointLoaded(result));
                }
                ProjectionStep::Yield(ProjectionEffect::ReadEvents { filter, page }) => {
                    let result = self
                        .store
                        .read_events(filter, page)
                        .await
                        .map_err(|e| e.to_string());
                    step = pipeline.resume(ProjectionEffectResult::EventsRead(result));
                }
                ProjectionStep::Yield(ProjectionEffect::SaveCheckpoint { name, position }) => {
                    let result = match &self.checkpoint_store {
                        Some(cs) => cs.save(&name, position).await.map_err(|e| e.to_string()),
                        None => Ok(()),
                    };
                    step = pipeline.resume(ProjectionEffectResult::CheckpointSaved(result));
                }
                ProjectionStep::Yield(ProjectionEffect::Sleep { duration }) => {
                    tokio::time::sleep(duration).await;
                    step = pipeline.resume(ProjectionEffectResult::Slept);
                }
            }
        }
    }
}

/// Error type for projection operations.
///
/// Covers fatal processing failures and leadership acquisition errors
/// encountered during projection execution.
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
    <B as EventReader>::Error: std::fmt::Display,
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
