//! CQRS-specific projection abstractions.
//!
//! This module provides enhanced projection traits and implementations
//! that integrate with persistent read model storage and checkpoint management.

use super::{CheckpointStore, CqrsError, CqrsResult, Query, ReadModelStore};
use crate::{
    errors::{ProjectionError, ProjectionResult},
    event::Event,
    event_store::EventStore,
    projection::{Projection, ProjectionCheckpoint, ProjectionConfig, ProjectionStatus},
    projection_runner::{ProjectionRunner, ProjectionRunnerConfig},
};
use async_trait::async_trait;
use std::{marker::PhantomData, sync::Arc};
use tokio::sync::RwLock;
use tracing::{debug, error, info, instrument};

/// Extended projection trait for CQRS systems.
///
/// This trait extends the base `Projection` trait with capabilities
/// for managing persistent read models and handling queries.
#[async_trait]
pub trait CqrsProjection: Projection {
    /// The type of read model this projection maintains
    type ReadModel: Send + Sync;

    /// The type of queries this projection can handle
    type Query: Query<Model = Self::ReadModel> + Send + Sync;

    /// Extract the read model ID from an event.
    ///
    /// Returns `None` if this event doesn't affect any read model.
    fn extract_model_id(&self, event: &Event<Self::Event>) -> Option<String>;

    /// Apply an event to a read model.
    ///
    /// The model parameter will be `None` if this is the first event
    /// for this model ID. Return `None` to delete the model.
    async fn apply_to_model(
        &self,
        model: Option<Self::ReadModel>,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<Option<Self::ReadModel>>;

    /// Handle a query against the read model store.
    ///
    /// This allows projections to implement custom query logic beyond
    /// what the store provides by default.
    async fn handle_query(
        &self,
        store: &dyn ReadModelStore<Model = Self::ReadModel, Query = Self::Query, Error = CqrsError>,
        query: Self::Query,
    ) -> ProjectionResult<Vec<Self::ReadModel>> {
        store
            .query(query)
            .await
            .map_err(|e| ProjectionError::Internal(format!("Query failed: {e}")))
    }

    /// Initialize a new read model.
    ///
    /// Called when a model ID is encountered for the first time.
    /// Default implementation returns None, letting apply_to_model handle initialization.
    async fn initialize_model(&self, _id: &str) -> ProjectionResult<Option<Self::ReadModel>> {
        Ok(None)
    }

    /// Called before a batch of events is processed.
    ///
    /// Can be used for optimizations like loading all affected models at once.
    async fn before_batch(
        &self,
        _events: &[Event<Self::Event>],
        _store: &dyn ReadModelStore<
            Model = Self::ReadModel,
            Query = Self::Query,
            Error = CqrsError,
        >,
    ) -> ProjectionResult<()> {
        Ok(())
    }

    /// Called after a batch of events is processed.
    ///
    /// Can be used for batch optimizations like bulk updates.
    async fn after_batch(
        &self,
        _events: &[Event<Self::Event>],
        _store: &dyn ReadModelStore<
            Model = Self::ReadModel,
            Query = Self::Query,
            Error = CqrsError,
        >,
    ) -> ProjectionResult<()> {
        Ok(())
    }
}

/// CQRS-aware projection runner that integrates with read model and checkpoint stores.
pub struct CqrsProjectionRunner<P, E>
where
    P: CqrsProjection<Event = E> + 'static,
    P::State: 'static,
    P::ReadModel: 'static,
    P::Query: 'static,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
{
    projection: Arc<P>,
    read_model_store:
        Arc<dyn ReadModelStore<Model = P::ReadModel, Query = P::Query, Error = CqrsError>>,
    checkpoint_store: Arc<dyn CheckpointStore<Error = CqrsError>>,
    inner_runner: ProjectionRunner<CqrsProjectionAdapter<P, E>, E>,
    _phantom: PhantomData<E>,
}

impl<P, E> CqrsProjectionRunner<P, E>
where
    P: CqrsProjection<Event = E> + Send + Sync + 'static,
    P::State: Send + Sync + std::fmt::Debug + Clone + 'static,
    P::ReadModel: Send + Sync + 'static,
    P::Query: Send + Sync + 'static,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
{
    /// Creates a new CQRS projection runner.
    pub fn new(
        projection: P,
        read_model_store: Arc<
            dyn ReadModelStore<Model = P::ReadModel, Query = P::Query, Error = CqrsError>,
        >,
        checkpoint_store: Arc<dyn CheckpointStore<Error = CqrsError>>,
    ) -> Self {
        Self::with_config(
            projection,
            read_model_store,
            checkpoint_store,
            ProjectionRunnerConfig::default(),
        )
    }

    /// Creates a new CQRS projection runner with custom configuration.
    pub fn with_config(
        projection: P,
        read_model_store: Arc<
            dyn ReadModelStore<Model = P::ReadModel, Query = P::Query, Error = CqrsError>,
        >,
        checkpoint_store: Arc<dyn CheckpointStore<Error = CqrsError>>,
        config: ProjectionRunnerConfig,
    ) -> Self {
        let projection = Arc::new(projection);
        let adapter = CqrsProjectionAdapter {
            projection: projection.clone(),
            read_model_store: read_model_store.clone(),
            checkpoint_store: checkpoint_store.clone(),
            state: Arc::new(RwLock::new(None)),
        };

        let inner_runner = ProjectionRunner::with_config(adapter, config);

        Self {
            projection,
            read_model_store,
            checkpoint_store,
            inner_runner,
            _phantom: PhantomData,
        }
    }

    /// Start the projection runner.
    pub async fn start(&self, event_store: Arc<dyn EventStore<Event = E>>) -> ProjectionResult<()> {
        info!(
            "Starting CQRS projection runner for: {}",
            self.projection.config().name
        );
        self.inner_runner.start(event_store).await
    }

    /// Stop the projection runner.
    pub async fn stop(&self) -> ProjectionResult<()> {
        info!(
            "Stopping CQRS projection runner for: {}",
            self.projection.config().name
        );
        self.inner_runner.stop().await
    }

    /// Pause the projection runner.
    pub async fn pause(&self) -> ProjectionResult<()> {
        self.inner_runner.pause().await
    }

    /// Resume the projection runner.
    pub async fn resume(&self) -> ProjectionResult<()> {
        self.inner_runner.resume().await
    }

    /// Get the current status of the projection.
    #[allow(clippy::missing_const_for_fn)]
    pub fn status(&self) -> ProjectionResult<ProjectionStatus> {
        // TODO: Implement status tracking
        Ok(ProjectionStatus::Running)
    }

    /// Handle a query using this projection's read models.
    pub async fn query(&self, query: P::Query) -> ProjectionResult<Vec<P::ReadModel>> {
        self.projection
            .handle_query(self.read_model_store.as_ref(), query)
            .await
    }

    /// Get a specific read model by ID.
    pub async fn get_model(&self, id: &str) -> CqrsResult<Option<P::ReadModel>> {
        self.read_model_store
            .get(id)
            .await
            .map_err(|e| CqrsError::storage(format!("Failed to get model: {e}")))
    }
}

/// Adapter that wraps a CqrsProjection to implement the base Projection trait.
struct CqrsProjectionAdapter<P, E>
where
    P: CqrsProjection<Event = E>,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone,
{
    projection: Arc<P>,
    read_model_store:
        Arc<dyn ReadModelStore<Model = P::ReadModel, Query = P::Query, Error = CqrsError>>,
    checkpoint_store: Arc<dyn CheckpointStore<Error = CqrsError>>,
    state: Arc<RwLock<Option<P::State>>>,
}

impl<P, E> std::fmt::Debug for CqrsProjectionAdapter<P, E>
where
    P: CqrsProjection<Event = E> + std::fmt::Debug,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CqrsProjectionAdapter")
            .field("projection", &self.projection)
            .field("read_model_store", &"<dyn ReadModelStore>")
            .field("checkpoint_store", &"<dyn CheckpointStore>")
            .field("state", &"<RwLock>")
            .finish()
    }
}

#[async_trait]
impl<P, E> Projection for CqrsProjectionAdapter<P, E>
where
    P: CqrsProjection<Event = E> + Send + Sync + 'static,
    P::State: Send + Sync + std::fmt::Debug + Clone + 'static,
    P::ReadModel: Send + Sync + 'static,
    P::Query: Send + Sync + 'static,
    E: Send + Sync + PartialEq + Eq + std::fmt::Debug + Clone + 'static,
{
    type State = P::State;
    type Event = E;

    fn config(&self) -> &ProjectionConfig {
        self.projection.config()
    }

    async fn get_state(&self) -> ProjectionResult<Self::State> {
        let state_guard = self.state.read().await;
        match &*state_guard {
            Some(state) => Ok(state.clone()),
            None => self.projection.initialize_state().await,
        }
    }

    async fn get_status(&self) -> ProjectionResult<ProjectionStatus> {
        self.projection.get_status().await
    }

    async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
        let checkpoint = self
            .checkpoint_store
            .load(&self.projection.config().name)
            .await
            .map_err(|e| ProjectionError::Internal(format!("Failed to load checkpoint: {e}")))?
            .unwrap_or_else(|| {
                debug!(
                    "No checkpoint found for projection: {}",
                    self.projection.config().name
                );
                ProjectionCheckpoint::initial()
            });
        Ok(checkpoint)
    }

    async fn save_checkpoint(&self, checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
        self.checkpoint_store
            .save(&self.projection.config().name, checkpoint)
            .await
            .map_err(|e| ProjectionError::Internal(format!("Failed to save checkpoint: {e}")))
    }

    #[instrument(skip_all, fields(event_id = ?event.id))]
    async fn apply_event(
        &self,
        state: &mut Self::State,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<()> {
        // First apply to the projection's internal state
        self.projection.apply_event(state, event).await?;

        // Then update read models if applicable
        if let Some(model_id) = self.projection.extract_model_id(event) {
            debug!("Updating read model: {}", model_id);

            // Load existing model
            let existing_model = self
                .read_model_store
                .get(&model_id)
                .await
                .map_err(|e| ProjectionError::Internal(format!("Failed to load model: {e}")))?;

            // Apply event to model
            let updated_model = self
                .projection
                .apply_to_model(existing_model, event)
                .await?;

            // Save or delete model
            match updated_model {
                Some(model) => {
                    self.read_model_store
                        .upsert(&model_id, model)
                        .await
                        .map_err(|e| {
                            ProjectionError::Internal(format!("Failed to save model: {e}"))
                        })?;
                }
                None => {
                    self.read_model_store.delete(&model_id).await.map_err(|e| {
                        ProjectionError::Internal(format!("Failed to delete model: {e}"))
                    })?;
                }
            }
        }

        Ok(())
    }

    async fn apply_events(
        &self,
        state: &mut Self::State,
        events: &[Event<Self::Event>],
    ) -> ProjectionResult<()> {
        if events.is_empty() {
            return Ok(());
        }

        // Call before_batch hook
        self.projection
            .before_batch(events, self.read_model_store.as_ref())
            .await?;

        // Group events by model ID for batch processing
        let mut events_by_model: std::collections::HashMap<Option<String>, Vec<&Event<E>>> =
            std::collections::HashMap::new();

        for event in events {
            let model_id = self.projection.extract_model_id(event);
            events_by_model.entry(model_id).or_default().push(event);
        }

        // Process events for each model
        for (model_id, model_events) in events_by_model {
            if let Some(model_id) = model_id {
                // Load existing model once
                let mut current_model =
                    self.read_model_store.get(&model_id).await.map_err(|e| {
                        ProjectionError::Internal(format!("Failed to load model: {e}"))
                    })?;

                // Apply all events to the model
                for event in model_events {
                    // Apply to projection state
                    self.projection.apply_event(state, event).await?;

                    // Apply to read model
                    current_model = self.projection.apply_to_model(current_model, event).await?;
                }

                // Save final model state
                match current_model {
                    Some(model) => {
                        self.read_model_store
                            .upsert(&model_id, model)
                            .await
                            .map_err(|e| {
                                ProjectionError::Internal(format!("Failed to save model: {e}"))
                            })?;
                    }
                    None => {
                        self.read_model_store.delete(&model_id).await.map_err(|e| {
                            ProjectionError::Internal(format!("Failed to delete model: {e}"))
                        })?;
                    }
                }
            } else {
                // Events that don't affect read models - just update projection state
                for event in model_events {
                    self.projection.apply_event(state, event).await?;
                }
            }
        }

        // Call after_batch hook
        self.projection
            .after_batch(events, self.read_model_store.as_ref())
            .await?;

        Ok(())
    }

    async fn initialize_state(&self) -> ProjectionResult<Self::State> {
        let state = self.projection.initialize_state().await?;
        *self.state.write().await = Some(state.clone());
        Ok(state)
    }

    async fn on_start(&self) -> ProjectionResult<()> {
        self.projection.on_start().await
    }

    async fn on_stop(&self) -> ProjectionResult<()> {
        self.projection.on_stop().await
    }

    async fn on_pause(&self) -> ProjectionResult<()> {
        self.projection.on_pause().await
    }

    async fn on_resume(&self) -> ProjectionResult<()> {
        self.projection.on_resume().await
    }

    async fn on_error(&self, error: &ProjectionError) -> ProjectionResult<()> {
        error!("CQRS projection error: {}", error);
        self.projection.on_error(error).await
    }

    fn should_process_event(&self, event: &Event<Self::Event>) -> bool {
        self.projection.should_process_event(event)
    }

    fn interested_streams(&self) -> Vec<crate::types::StreamId> {
        self.projection.interested_streams()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cqrs::{InMemoryCheckpointStore, InMemoryReadModelStore, QueryBuilder},
        metadata::EventMetadata,
        types::{EventId, StreamId, Timestamp},
    };
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestModel {
        id: String,
        value: i32,
        updated_at: Timestamp,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum TestEvent {
        Created { id: String, value: i32 },
        Updated { id: String, value: i32 },
        Deleted { id: String },
    }

    #[derive(Debug)]
    struct TestProjection;

    #[async_trait]
    impl Projection for TestProjection {
        type State = ();
        type Event = TestEvent;

        fn config(&self) -> &ProjectionConfig {
            Box::leak(Box::new(ProjectionConfig::new("test_projection")))
        }

        async fn get_state(&self) -> ProjectionResult<Self::State> {
            Ok(())
        }

        async fn get_status(&self) -> ProjectionResult<ProjectionStatus> {
            Ok(ProjectionStatus::Running)
        }

        async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
            Ok(ProjectionCheckpoint::initial())
        }

        async fn save_checkpoint(&self, _checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
            Ok(())
        }

        async fn apply_event(
            &self,
            _state: &mut Self::State,
            _event: &Event<Self::Event>,
        ) -> ProjectionResult<()> {
            Ok(())
        }

        async fn initialize_state(&self) -> ProjectionResult<Self::State> {
            Ok(())
        }
    }

    #[async_trait]
    impl CqrsProjection for TestProjection {
        type ReadModel = TestModel;
        type Query = QueryBuilder<TestModel>;

        fn extract_model_id(&self, event: &Event<Self::Event>) -> Option<String> {
            match &event.payload {
                TestEvent::Created { id, .. }
                | TestEvent::Updated { id, .. }
                | TestEvent::Deleted { id } => Some(id.clone()),
            }
        }

        async fn apply_to_model(
            &self,
            model: Option<Self::ReadModel>,
            event: &Event<Self::Event>,
        ) -> ProjectionResult<Option<Self::ReadModel>> {
            match &event.payload {
                TestEvent::Created { id, value } => Ok(Some(TestModel {
                    id: id.clone(),
                    value: *value,
                    updated_at: event.created_at,
                })),
                TestEvent::Updated { id: _, value } => {
                    let mut model = model.ok_or_else(|| {
                        ProjectionError::Internal("Model not found for update".to_string())
                    })?;
                    model.value = *value;
                    model.updated_at = event.created_at;
                    Ok(Some(model))
                }
                TestEvent::Deleted { .. } => Ok(None),
            }
        }
    }

    #[tokio::test]
    async fn cqrs_projection_apply_events() {
        let projection = TestProjection;
        let read_store = Arc::new(InMemoryReadModelStore::new());
        let checkpoint_store = Arc::new(InMemoryCheckpointStore::new());

        let adapter = CqrsProjectionAdapter {
            projection: Arc::new(projection),
            read_model_store: read_store.clone(),
            checkpoint_store,
            state: Arc::new(RwLock::new(Some(()))),
        };

        // Create event
        let event = Event {
            id: EventId::new(),
            stream_id: StreamId::try_new("test-stream").unwrap(),
            payload: TestEvent::Created {
                id: "model1".to_string(),
                value: 42,
            },
            metadata: EventMetadata::new(),
            created_at: Timestamp::now(),
        };

        let mut state = ();
        adapter.apply_event(&mut state, &event).await.unwrap();

        // Verify model was created
        let model = read_store.get("model1").await.unwrap().unwrap();
        assert_eq!(model.id, "model1");
        assert_eq!(model.value, 42);

        // Update event
        let update_event = Event {
            id: EventId::new(),
            stream_id: StreamId::try_new("test-stream").unwrap(),
            payload: TestEvent::Updated {
                id: "model1".to_string(),
                value: 100,
            },
            metadata: EventMetadata::new(),
            created_at: Timestamp::now(),
        };

        adapter
            .apply_event(&mut state, &update_event)
            .await
            .unwrap();

        // Verify model was updated
        let model = read_store.get("model1").await.unwrap().unwrap();
        assert_eq!(model.value, 100);

        // Delete event
        let delete_event = Event {
            id: EventId::new(),
            stream_id: StreamId::try_new("test-stream").unwrap(),
            payload: TestEvent::Deleted {
                id: "model1".to_string(),
            },
            metadata: EventMetadata::new(),
            created_at: Timestamp::now(),
        };

        adapter
            .apply_event(&mut state, &delete_event)
            .await
            .unwrap();

        // Verify model was deleted
        assert!(read_store.get("model1").await.unwrap().is_none());
    }
}
