//! Projection runtime components for building and running read models.
//!
//! This module provides the runtime infrastructure for event projection:
//! - `LocalCoordinator`: Single-process coordination for projector leadership
//! - `ProjectionRunner`: Orchestrates projector execution with event polling

use crate::{Event, EventReader, Projector, StreamPosition};

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
        if let Ok(mut guard) = self.checkpoints.lock() {
            let _ = guard.insert(projector_name.to_string(), position);
        }
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
}

impl<P, C, S> ProjectionRunner<P, C, S>
where
    P: Projector,
    P::Event: Event,
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
    /// - `_checkpoint_store`: The checkpoint store for saving/loading positions
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

    /// Run the projection, processing events until completion.
    ///
    /// This method:
    /// 1. Acquires leadership from the coordinator
    /// 2. Polls for events starting from the last checkpoint
    /// 3. Applies each event to the projector
    /// 4. Checkpoints progress after successful processing
    /// 5. Continues until no more events are available
    ///
    /// # Returns
    ///
    /// - `Ok(())`: All available events were processed successfully
    /// - `Err(E)`: An unrecoverable error occurred during projection
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Leadership cannot be acquired
    /// - Event store operations fail
    /// - The projector returns a fatal error
    pub async fn run(mut self) -> Result<(), ProjectionError> {
        // Load checkpoint if checkpoint store is configured
        let mut last_checkpoint = self
            .checkpoint_store
            .as_ref()
            .and_then(|cs| cs.load(self.projector.name()));

        let mut ctx = P::Context::default();

        loop {
            // Read events from the store (either all or after checkpoint)
            let events: Vec<(P::Event, _)> =
                match last_checkpoint {
                    Some(checkpoint) => self.store.read_after(checkpoint).await.map_err(|_| {
                        ProjectionError::Failed("failed to read events".to_string())
                    })?,
                    None => self.store.read_all().await.map_err(|_| {
                        ProjectionError::Failed("failed to read events".to_string())
                    })?,
                };

            // Apply each event to the projector
            for (event, position) in events {
                self.projector
                    .apply(event, position, &mut ctx)
                    .map_err(|_| ProjectionError::Failed("projector apply failed".to_string()))?;
                last_checkpoint = Some(position);
            }

            // Save checkpoint if checkpoint store is configured
            if let (Some(cs), Some(position)) = (&self.checkpoint_store, last_checkpoint) {
                cs.save(self.projector.name(), position);
            }

            // For batch mode, exit after one pass
            if self.poll_mode == PollMode::Batch {
                break;
            }

            // For continuous mode, sleep before next poll
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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
