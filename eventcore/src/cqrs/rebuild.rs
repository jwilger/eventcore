//! Projection rebuild and migration support for CQRS.
//!
//! This module provides functionality for rebuilding projections from scratch,
//! migrating between versions, and monitoring rebuild progress.

use super::{CheckpointStore, CqrsError, CqrsProjection, CqrsResult, ReadModelStore};
use crate::{
    errors::ProjectionError,
    event_store::{EventStore, StoredEvent},
    projection::ProjectionCheckpoint,
    subscription::{EventProcessor, SubscriptionError, SubscriptionResult},
    types::EventId,
};
use async_trait::async_trait;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};
use tokio::sync::RwLock;
use tracing::{info, instrument};

/// Strategy for rebuilding projections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebuildStrategy {
    /// Rebuild from the beginning of all streams
    FromBeginning,
    /// Rebuild from a specific checkpoint
    FromCheckpoint(ProjectionCheckpoint),
    /// Rebuild from a specific event ID
    FromEvent(EventId),
    /// Rebuild only specific streams
    SpecificStreams(StreamIds),
}

/// Helper type for specifying stream IDs in rebuild strategies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamIds {
    // In a real implementation, this would hold actual stream IDs
    // For now, it's a placeholder
}

/// Progress tracking for projection rebuilds.
#[derive(Debug, Clone)]
pub struct RebuildProgress {
    /// Total number of events to process (if known)
    pub total_events: Option<u64>,
    /// Number of events processed so far
    pub events_processed: u64,
    /// Number of read models updated
    pub models_updated: u64,
    /// Start time of the rebuild
    pub started_at: Instant,
    /// Estimated completion time
    pub estimated_completion: Option<Instant>,
    /// Current processing rate (events per second)
    pub events_per_second: f64,
    /// Whether the rebuild is currently running
    pub is_running: bool,
    /// Any error that occurred during rebuild
    pub error: Option<String>,
}

impl RebuildProgress {
    /// Creates a new rebuild progress tracker.
    pub fn new() -> Self {
        Self {
            total_events: None,
            events_processed: 0,
            models_updated: 0,
            started_at: Instant::now(),
            estimated_completion: None,
            events_per_second: 0.0,
            is_running: true,
            error: None,
        }
    }

    /// Updates the progress with current statistics.
    #[allow(clippy::cast_precision_loss)]
    pub fn update(&mut self, events_processed: u64, models_updated: u64) {
        self.events_processed = events_processed;
        self.models_updated = models_updated;

        let elapsed = self.started_at.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.events_per_second = events_processed as f64 / elapsed;

            if let Some(total) = self.total_events {
                let remaining = total.saturating_sub(events_processed);
                if self.events_per_second > 0.0 {
                    let remaining_secs = remaining as f64 / self.events_per_second;
                    self.estimated_completion =
                        Some(Instant::now() + std::time::Duration::from_secs_f64(remaining_secs));
                }
            }
        }
    }

    /// Gets the completion percentage.
    #[allow(clippy::cast_precision_loss)]
    pub fn completion_percentage(&self) -> Option<f64> {
        self.total_events
            .map(|total| (self.events_processed as f64 / total as f64) * 100.0)
    }

    /// Gets the elapsed time.
    pub fn elapsed(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }
}

impl Default for RebuildProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// Coordinates projection rebuilds with progress tracking and error recovery.
pub struct RebuildCoordinator<P, E>
where
    P: CqrsProjection<Event = E>,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
{
    projection: Arc<P>,
    event_store: Arc<dyn EventStore<Event = E>>,
    read_model_store:
        Arc<dyn ReadModelStore<Model = P::ReadModel, Query = P::Query, Error = CqrsError>>,
    checkpoint_store: Arc<dyn CheckpointStore<Error = CqrsError>>,
    progress: Arc<RwLock<RebuildProgress>>,
    is_cancelled: Arc<AtomicBool>,
    events_processed: Arc<AtomicU64>,
    models_updated: Arc<AtomicU64>,
}

impl<P, E> RebuildCoordinator<P, E>
where
    P: CqrsProjection<Event = E> + Send + Sync + 'static,
    P::State: Send + Sync + std::fmt::Debug + Clone + 'static,
    P::ReadModel: Send + Sync + 'static,
    P::Query: Send + Sync + 'static,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
{
    /// Creates a new rebuild coordinator.
    pub fn new(
        projection: P,
        event_store: Arc<dyn EventStore<Event = E>>,
        read_model_store: Arc<
            dyn ReadModelStore<Model = P::ReadModel, Query = P::Query, Error = CqrsError>,
        >,
        checkpoint_store: Arc<dyn CheckpointStore<Error = CqrsError>>,
    ) -> Self {
        Self {
            projection: Arc::new(projection),
            event_store,
            read_model_store,
            checkpoint_store,
            progress: Arc::new(RwLock::new(RebuildProgress::new())),
            is_cancelled: Arc::new(AtomicBool::new(false)),
            events_processed: Arc::new(AtomicU64::new(0)),
            models_updated: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Rebuilds the projection using the specified strategy.
    #[instrument(skip(self), fields(projection = %self.projection.config().name))]
    pub async fn rebuild(&self, strategy: RebuildStrategy) -> CqrsResult<RebuildProgress> {
        info!("Starting projection rebuild with strategy: {:?}", strategy);

        // Reset progress
        {
            let mut progress = self.progress.write().await;
            *progress = RebuildProgress::new();
        }
        self.events_processed.store(0, Ordering::SeqCst);
        self.models_updated.store(0, Ordering::SeqCst);
        self.is_cancelled.store(false, Ordering::SeqCst);

        // Clear existing state based on strategy
        match strategy {
            RebuildStrategy::FromBeginning => {
                info!("Clearing all read models and checkpoints");
                self.read_model_store
                    .clear()
                    .await
                    .map_err(|e| CqrsError::rebuild(format!("Failed to clear read models: {e}")))?;
                self.checkpoint_store
                    .delete(&self.projection.config().name)
                    .await?;
            }
            RebuildStrategy::FromCheckpoint(checkpoint) => {
                info!("Rebuilding from checkpoint: {:?}", checkpoint);
                // In a real implementation, we'd selectively clear models
                // that would be affected by events after the checkpoint
            }
            RebuildStrategy::FromEvent(event_id) => {
                info!("Rebuilding from event: {:?}", event_id);
                // Similar to FromCheckpoint
            }
            RebuildStrategy::SpecificStreams(_) => {
                info!("Rebuilding specific streams");
                // Would clear only models affected by specified streams
            }
        }

        // TODO: Implement subscription-based rebuild
        // For now, return an error indicating this is not yet implemented
        Err(CqrsError::rebuild(
            "Subscription-based rebuild not yet implemented",
        ))
    }

    /// Cancels an ongoing rebuild.
    pub fn cancel(&self) {
        info!("Cancelling projection rebuild");
        self.is_cancelled.store(true, Ordering::SeqCst);
    }

    /// Gets the current rebuild progress.
    pub async fn get_progress(&self) -> RebuildProgress {
        self.progress.read().await.clone()
    }

    /// Rebuilds from the beginning.
    pub async fn rebuild_from_beginning(&self) -> CqrsResult<RebuildProgress> {
        self.rebuild(RebuildStrategy::FromBeginning).await
    }

    /// Rebuilds from a specific checkpoint.
    pub async fn rebuild_from_checkpoint(
        &self,
        checkpoint: ProjectionCheckpoint,
    ) -> CqrsResult<RebuildProgress> {
        self.rebuild(RebuildStrategy::FromCheckpoint(checkpoint))
            .await
    }
}

/// Event processor for rebuild operations.
struct RebuildProcessor<P, E>
where
    P: CqrsProjection<Event = E>,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone,
{
    projection: Arc<P>,
    read_model_store:
        Arc<dyn ReadModelStore<Model = P::ReadModel, Query = P::Query, Error = CqrsError>>,
    checkpoint_store: Arc<dyn CheckpointStore<Error = CqrsError>>,
    progress: Arc<RwLock<RebuildProgress>>,
    is_cancelled: Arc<AtomicBool>,
    events_processed: Arc<AtomicU64>,
    models_updated: Arc<AtomicU64>,
    state: Arc<RwLock<Option<P::State>>>,
}

#[async_trait]
impl<P, E> EventProcessor for RebuildProcessor<P, E>
where
    P: CqrsProjection<Event = E> + Send + Sync + 'static,
    P::State: Send + Sync + std::fmt::Debug + Clone + 'static,
    P::ReadModel: Send + Sync + 'static,
    P::Query: Send + Sync + 'static,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
{
    type Event = E;

    async fn process_event(&mut self, event: StoredEvent<Self::Event>) -> SubscriptionResult<()> {
        if self.is_cancelled.load(Ordering::Acquire) {
            return Err(SubscriptionError::Cancelled);
        }

        // Initialize state if needed
        let mut state = {
            let state_guard = self.state.read().await;
            if let Some(state) = &*state_guard {
                state.clone()
            } else {
                drop(state_guard);
                let new_state = self
                    .projection
                    .initialize_state()
                    .await
                    .map_err(SubscriptionError::Projection)?;
                *self.state.write().await = Some(new_state.clone());
                new_state
            }
        };

        // Convert StoredEvent to Event for the projection
        let event_wrapper = crate::event::Event {
            id: event.event_id,
            stream_id: event.stream_id.clone(),
            payload: event.payload.clone(),
            metadata: event.metadata.clone().unwrap_or_default(),
            created_at: event.timestamp,
        };

        if self.projection.should_process_event(&event_wrapper) {
            // Apply to projection state
            self.projection
                .apply_event(&mut state, &event_wrapper)
                .await
                .map_err(|e| SubscriptionError::CheckpointSaveFailed(e.to_string()))?;

            // Update read model if applicable
            if let Some(model_id) = self.projection.extract_model_id(&event_wrapper) {
                let existing_model = self
                    .read_model_store
                    .get(&model_id)
                    .await
                    .map_err(|e| SubscriptionError::CheckpointSaveFailed(e.to_string()))?;

                let updated_model = self
                    .projection
                    .apply_to_model(existing_model, &event_wrapper)
                    .await
                    .map_err(|e| SubscriptionError::CheckpointSaveFailed(e.to_string()))?;

                match updated_model {
                    Some(model) => {
                        self.read_model_store
                            .upsert(&model_id, model)
                            .await
                            .map_err(|e| {
                                SubscriptionError::Projection(ProjectionError::Internal(
                                    e.to_string(),
                                ))
                            })?;
                        self.models_updated.fetch_add(1, Ordering::Relaxed);
                    }
                    None => {
                        self.read_model_store.delete(&model_id).await.map_err(|e| {
                            SubscriptionError::Projection(ProjectionError::Internal(e.to_string()))
                        })?;
                    }
                }
            }
        }

        // Update state and progress
        *self.state.write().await = Some(state);
        self.events_processed.fetch_add(1, Ordering::Relaxed);

        // Save checkpoint periodically (every 100 events)
        let events_processed = self.events_processed.load(Ordering::Relaxed);
        if events_processed % 100 == 0 {
            let checkpoint = ProjectionCheckpoint::from_event_id(event.event_id);
            self.checkpoint_store
                .save(&self.projection.config().name, checkpoint)
                .await
                .map_err(|e| SubscriptionError::CheckpointSaveFailed(e.to_string()))?;
        }

        Ok(())
    }

    async fn on_live(&mut self) -> SubscriptionResult<()> {
        info!("Rebuild caught up to live position");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuild_progress_calculations() {
        let mut progress = RebuildProgress::new();
        progress.total_events = Some(1000);
        progress.update(250, 100);

        assert_eq!(progress.completion_percentage(), Some(25.0));
        assert_eq!(progress.events_processed, 250);
        assert_eq!(progress.models_updated, 100);
        assert!(progress.events_per_second > 0.0);
    }

    #[test]
    fn rebuild_strategy_equality() {
        let checkpoint = ProjectionCheckpoint::initial();
        let strategy1 = RebuildStrategy::FromCheckpoint(checkpoint.clone());
        let strategy2 = RebuildStrategy::FromCheckpoint(checkpoint);
        assert_eq!(strategy1, strategy2);

        let event_id = EventId::new();
        let strategy3 = RebuildStrategy::FromEvent(event_id);
        let strategy4 = RebuildStrategy::FromEvent(event_id);
        assert_eq!(strategy3, strategy4);
    }
}
