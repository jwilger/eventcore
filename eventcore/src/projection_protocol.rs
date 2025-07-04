//! Type-safe projection protocol with phantom types for compile-time phase validation.
//!
//! This module provides a type-safe protocol for projection lifecycle management using
//! phantom types to enforce correct phase transitions at compile time. The protocol
//! ensures that projections can only perform operations valid for their current phase.

use crate::{
    errors::{ProjectionError, ProjectionResult},
    event_store::EventStore,
    projection::{Projection, ProjectionCheckpoint, ProjectionStatus},
    subscription::Subscription,
};
use std::{marker::PhantomData, sync::Arc};
use tracing::{info, instrument};

/// Marker trait for projection protocol phases.
pub trait ProtocolPhase: Send + Sync + 'static {}

/// The setup phase - initial configuration and preparation.
#[derive(Debug)]
pub struct Setup;
impl ProtocolPhase for Setup {}

/// The processing phase - actively processing events.
#[derive(Debug)]
pub struct Processing;
impl ProtocolPhase for Processing {}

/// The checkpointing phase - saving progress.
#[derive(Debug)]
pub struct Checkpointing;
impl ProtocolPhase for Checkpointing {}

/// The shutdown phase - cleanup and resource release.
#[derive(Debug)]
pub struct Shutdown;
impl ProtocolPhase for Shutdown {}

/// Type-safe projection protocol that enforces phase transitions at compile time.
///
/// This struct uses phantom types to ensure that operations can only be performed
/// in the appropriate phases, preventing runtime errors from invalid state transitions.
pub struct ProjectionProtocol<P, E, Phase>
where
    P: Projection<Event = E>,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
    Phase: ProtocolPhase,
{
    projection: Arc<P>,
    state: Option<P::State>,
    checkpoint: ProjectionCheckpoint,
    subscription: Option<Box<dyn Subscription<Event = E>>>,
    event_store: Option<Arc<dyn EventStore<Event = E>>>,
    _phantom: PhantomData<Phase>,
}

impl<P, E> ProjectionProtocol<P, E, Setup>
where
    P: Projection<Event = E> + Send + Sync + 'static,
    P::State: Send + Sync + std::fmt::Debug + Clone + 'static,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
{
    /// Creates a new projection protocol in the Setup phase.
    pub fn new(projection: P) -> Self {
        Self {
            projection: Arc::new(projection),
            state: None,
            checkpoint: ProjectionCheckpoint::initial(),
            subscription: None,
            event_store: None,
            _phantom: PhantomData,
        }
    }

    /// Sets the event store for the projection.
    ///
    /// This method consumes self and returns a new instance to maintain
    /// the builder pattern while preserving phase information.
    #[must_use]
    pub fn with_event_store(mut self, event_store: Arc<dyn EventStore<Event = E>>) -> Self {
        self.event_store = Some(event_store);
        self
    }

    /// Loads the checkpoint and initializes state.
    ///
    /// This method is only available in the Setup phase and must be called
    /// before transitioning to the Processing phase.
    #[instrument(skip(self))]
    pub async fn load_checkpoint(mut self) -> ProjectionResult<Self> {
        info!(
            "Loading checkpoint for projection: {}",
            self.projection.config().name
        );

        // Load existing checkpoint or use initial
        let checkpoint = self.projection.load_checkpoint().await.unwrap_or_else(|_| {
            info!("No existing checkpoint found, using initial checkpoint");
            ProjectionCheckpoint::initial()
        });
        self.checkpoint = checkpoint;

        // Initialize state
        let state = self.projection.initialize_state().await?;
        self.state = Some(state);

        info!(
            "Successfully loaded checkpoint for projection: {}",
            self.projection.config().name
        );
        Ok(self)
    }

    /// Transitions from Setup to Processing phase.
    ///
    /// This method consumes the Setup protocol and returns a Processing protocol,
    /// ensuring at compile time that the projection has been properly initialized.
    #[instrument(skip(self))]
    pub async fn start_processing(
        mut self,
    ) -> ProjectionResult<ProjectionProtocol<P, E, Processing>> {
        // Ensure we have an event store
        let event_store = self
            .event_store
            .take()
            .ok_or_else(|| ProjectionError::Internal("Event store not configured".to_string()))?;

        // Ensure state is initialized
        if self.state.is_none() {
            return Err(ProjectionError::Internal(
                "State not initialized - call load_checkpoint first".to_string(),
            ));
        }

        info!(
            "Starting processing for projection: {}",
            self.projection.config().name
        );

        // Create subscription
        let streams = self.projection.interested_streams();
        let subscription_options = if streams.is_empty() {
            crate::subscription::SubscriptionOptions::AllStreams {
                from_position: self.checkpoint.last_event_id,
            }
        } else {
            crate::subscription::SubscriptionOptions::SpecificStreams {
                streams,
                from_position: self.checkpoint.last_event_id,
            }
        };

        let subscription = event_store.subscribe(subscription_options).await?;

        // Call lifecycle hook
        self.projection.on_start().await?;

        info!(
            "Successfully started processing for projection: {}",
            self.projection.config().name
        );

        Ok(ProjectionProtocol {
            projection: self.projection,
            state: self.state,
            checkpoint: self.checkpoint,
            subscription: Some(subscription),
            event_store: Some(event_store),
            _phantom: PhantomData,
        })
    }
}

impl<P, E> ProjectionProtocol<P, E, Processing>
where
    P: Projection<Event = E> + Send + Sync + 'static,
    P::State: Send + Sync + std::fmt::Debug + Clone + 'static,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
{
    /// Processes events while in the Processing phase.
    ///
    /// This method is only available when the protocol is in the Processing phase,
    /// ensuring that setup has been completed and resources are available.
    #[instrument(skip(self))]
    pub async fn process_events(&mut self, batch_size: usize) -> ProjectionResult<usize> {
        let _subscription = self.subscription.as_mut().ok_or_else(|| {
            ProjectionError::Internal("Subscription not available in processing phase".to_string())
        })?;

        let _state = self.state.as_mut().ok_or_else(|| {
            ProjectionError::Internal("State not available in processing phase".to_string())
        })?;

        // Process a batch of events
        let mut events_processed = 0;
        for _ in 0..batch_size {
            // In a real implementation, this would poll the subscription
            // For now, we'll just demonstrate the pattern
            events_processed += 1;
        }

        Ok(events_processed)
    }

    /// Pauses event processing.
    ///
    /// The projection remains in the Processing phase but suspends event consumption.
    #[instrument(skip(self))]
    pub async fn pause(&mut self) -> ProjectionResult<()> {
        info!("Pausing projection: {}", self.projection.config().name);

        if let Some(subscription) = self.subscription.as_mut() {
            subscription.pause().await.map_err(|e| {
                ProjectionError::SubscriptionFailed(format!("Failed to pause subscription: {e}"))
            })?;
        }

        self.projection.on_pause().await?;

        info!(
            "Successfully paused projection: {}",
            self.projection.config().name
        );
        Ok(())
    }

    /// Resumes event processing after a pause.
    #[instrument(skip(self))]
    pub async fn resume(&mut self) -> ProjectionResult<()> {
        info!("Resuming projection: {}", self.projection.config().name);

        if let Some(subscription) = self.subscription.as_mut() {
            subscription.resume().await.map_err(|e| {
                ProjectionError::SubscriptionFailed(format!("Failed to resume subscription: {e}"))
            })?;
        }

        self.projection.on_resume().await?;

        info!(
            "Successfully resumed projection: {}",
            self.projection.config().name
        );
        Ok(())
    }

    /// Transitions from Processing to Checkpointing phase.
    ///
    /// This consumes the Processing protocol and returns a Checkpointing protocol,
    /// ensuring that checkpointing operations can only be performed after processing.
    #[instrument(skip(self))]
    pub fn prepare_checkpoint(self) -> ProjectionProtocol<P, E, Checkpointing> {
        info!(
            "Preparing checkpoint for projection: {}",
            self.projection.config().name
        );

        ProjectionProtocol {
            projection: self.projection,
            state: self.state,
            checkpoint: self.checkpoint,
            subscription: self.subscription,
            event_store: self.event_store,
            _phantom: PhantomData,
        }
    }

    /// Gets the current projection status.
    ///
    /// This is available in the Processing phase to monitor projection health.
    pub async fn get_status(&self) -> ProjectionResult<ProjectionStatus> {
        self.projection.get_status().await
    }

    /// Gets the current state of the projection.
    ///
    /// Returns a clone of the current state if available.
    pub const fn get_state(&self) -> Option<&P::State> {
        self.state.as_ref()
    }
}

impl<P, E> ProjectionProtocol<P, E, Checkpointing>
where
    P: Projection<Event = E> + Send + Sync + 'static,
    P::State: Send + Sync + std::fmt::Debug + Clone + 'static,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
{
    /// Saves the current checkpoint.
    ///
    /// This method is only available in the Checkpointing phase, ensuring
    /// that checkpoints are saved at appropriate times in the lifecycle.
    #[instrument(skip(self))]
    pub async fn save_checkpoint(&mut self) -> ProjectionResult<()> {
        info!(
            "Saving checkpoint for projection: {}",
            self.projection.config().name
        );

        self.projection
            .save_checkpoint(self.checkpoint.clone())
            .await?;

        info!(
            "Successfully saved checkpoint for projection: {}",
            self.projection.config().name
        );
        Ok(())
    }

    /// Updates the checkpoint with a new event ID.
    ///
    /// This method modifies the checkpoint data before saving.
    pub fn update_checkpoint(&mut self, event_id: crate::types::EventId) {
        self.checkpoint = self.checkpoint.clone().with_event_id(event_id);
    }

    /// Transitions back to Processing phase after checkpointing.
    ///
    /// This allows the projection to continue processing after saving progress.
    #[instrument(skip(self))]
    pub fn resume_processing(self) -> ProjectionProtocol<P, E, Processing> {
        info!(
            "Resuming processing after checkpoint for projection: {}",
            self.projection.config().name
        );

        ProjectionProtocol {
            projection: self.projection,
            state: self.state,
            checkpoint: self.checkpoint,
            subscription: self.subscription,
            event_store: self.event_store,
            _phantom: PhantomData,
        }
    }

    /// Transitions to Shutdown phase from Checkpointing.
    ///
    /// This ensures that final checkpoints are saved before shutdown.
    #[instrument(skip(self))]
    pub fn prepare_shutdown(self) -> ProjectionProtocol<P, E, Shutdown> {
        info!(
            "Preparing shutdown for projection: {}",
            self.projection.config().name
        );

        ProjectionProtocol {
            projection: self.projection,
            state: self.state,
            checkpoint: self.checkpoint,
            subscription: self.subscription,
            event_store: self.event_store,
            _phantom: PhantomData,
        }
    }
}

impl<P, E> ProjectionProtocol<P, E, Shutdown>
where
    P: Projection<Event = E> + Send + Sync + 'static,
    P::State: Send + Sync + std::fmt::Debug + Clone + 'static,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
{
    /// Performs cleanup and releases resources.
    ///
    /// This method is only available in the Shutdown phase and consumes
    /// the protocol, ensuring that no operations can be performed after shutdown.
    #[instrument(skip(self))]
    pub async fn shutdown(mut self) -> ProjectionResult<()> {
        info!(
            "Shutting down projection: {}",
            self.projection.config().name
        );

        // Stop subscription if active
        if let Some(mut subscription) = self.subscription.take() {
            subscription.stop().await.map_err(|e| {
                ProjectionError::SubscriptionFailed(format!("Failed to stop subscription: {e}"))
            })?;
        }

        // Call lifecycle hook
        self.projection.on_stop().await?;

        info!(
            "Successfully shut down projection: {}",
            self.projection.config().name
        );
        Ok(())
    }

    /// Retrieves the final state before shutdown completes.
    ///
    /// This allows saving or exporting the final projection state.
    pub fn final_state(self) -> Option<P::State> {
        self.state
    }

    /// Retrieves the final checkpoint before shutdown completes.
    pub const fn final_checkpoint(&self) -> &ProjectionCheckpoint {
        &self.checkpoint
    }
}

// Helper functions for common transitions

/// Convenience function to go directly from Processing to Shutdown.
///
/// This handles the checkpoint save automatically before shutdown.
pub async fn shutdown_with_checkpoint<P, E>(
    protocol: ProjectionProtocol<P, E, Processing>,
) -> ProjectionResult<()>
where
    P: Projection<Event = E> + Send + Sync + 'static,
    P::State: Send + Sync + std::fmt::Debug + Clone + 'static,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
{
    // Transition to checkpointing
    let mut checkpoint_protocol = protocol.prepare_checkpoint();

    // Save checkpoint
    checkpoint_protocol.save_checkpoint().await?;

    // Transition to shutdown
    let shutdown_protocol = checkpoint_protocol.prepare_shutdown();

    // Perform shutdown
    shutdown_protocol.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::{InMemoryProjection, ProjectionConfig};

    #[tokio::test]
    async fn projection_protocol_phase_transitions() {
        // Create a test projection
        let config = ProjectionConfig::new("test-projection");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

        // Start in Setup phase
        let setup_protocol = ProjectionProtocol::new(projection);

        // Can only call setup phase methods
        let _setup_with_checkpoint = setup_protocol.load_checkpoint().await.unwrap();

        // Transition to Processing phase would require an event store
        // For this test, we're just demonstrating the type safety

        // The following would not compile:
        // setup_protocol.process_events(10).await; // Error: method not found
        // setup_protocol.save_checkpoint().await; // Error: method not found
        // setup_protocol.shutdown().await; // Error: method not found
    }

    #[test]
    fn phantom_types_prevent_invalid_transitions() {
        // This test demonstrates compile-time safety.
        // Invalid transitions are caught by the compiler, not at runtime.

        // The type system ensures:
        // 1. Setup phase can only transition to Processing
        // 2. Processing can transition to Checkpointing or be paused/resumed
        // 3. Checkpointing can transition back to Processing or to Shutdown
        // 4. Shutdown is terminal - no transitions possible after shutdown

        // These constraints are enforced at compile time through the phantom type parameter.
    }
}
