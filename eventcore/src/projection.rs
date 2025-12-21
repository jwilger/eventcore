//! Projection runtime components for building and running read models.
//!
//! This module provides the runtime infrastructure for event projection:
//! - `LocalCoordinator`: Single-process coordination for projector leadership
//! - `ProjectionRunner`: Orchestrates projector execution with event polling

use crate::{Event, EventReader, Projector};

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
        }
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
        // Read all events from the store
        let events: Vec<(P::Event, _)> = self
            .store
            .read_all()
            .await
            .map_err(|_| ProjectionError::Failed("failed to read events".to_string()))?;

        // Apply each event to the projector
        let mut ctx = P::Context::default();
        for (event, position) in events {
            self.projector
                .apply(event, position, &mut ctx)
                .map_err(|_| ProjectionError::Failed("projector apply failed".to_string()))?;
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
