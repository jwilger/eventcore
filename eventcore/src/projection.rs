//! Projection system for the `EventCore` event sourcing library.
//!
//! This module defines the core projection traits and types that enable
//! building read models from event streams. Projections maintain their own
//! state and checkpoint management for resumability.

use crate::errors::{ProjectionError, ProjectionResult};
use crate::event::Event;
use crate::types::{EventId, StreamId, Timestamp};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;

/// A checkpoint representing the position in the event stream where a projection
/// has processed events up to.
///
/// Checkpoints enable projections to resume processing from where they left off
/// after restarts or failures, ensuring exactly-once processing semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionCheckpoint {
    /// The ID of the last processed event
    pub last_event_id: Option<EventId>,
    /// The timestamp when this checkpoint was created
    pub checkpoint_time: Timestamp,
    /// Stream-specific positions for multi-stream projections
    pub stream_positions: HashMap<StreamId, EventId>,
}

impl Ord for ProjectionCheckpoint {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare by last_event_id first, then checkpoint_time
        // Ignore stream_positions for ordering since HashMap doesn't have ordering
        match (self.last_event_id, other.last_event_id) {
            (Some(a), Some(b)) => a.cmp(&b),
            (None, None) => self.checkpoint_time.cmp(&other.checkpoint_time),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
        }
    }
}

impl PartialOrd for ProjectionCheckpoint {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl ProjectionCheckpoint {
    /// Creates a new checkpoint at the beginning (no events processed yet).
    pub fn initial() -> Self {
        Self {
            last_event_id: None,
            checkpoint_time: Timestamp::now(),
            stream_positions: HashMap::new(),
        }
    }

    /// Creates a checkpoint for the given event.
    pub fn from_event_id(event_id: EventId) -> Self {
        Self {
            last_event_id: Some(event_id),
            checkpoint_time: Timestamp::now(),
            stream_positions: HashMap::new(),
        }
    }

    /// Updates the checkpoint with a new event ID.
    #[must_use]
    pub fn with_event_id(mut self, event_id: EventId) -> Self {
        self.last_event_id = Some(event_id);
        self.checkpoint_time = Timestamp::now();
        self
    }

    /// Updates the position for a specific stream.
    #[must_use]
    pub fn with_stream_position(mut self, stream_id: StreamId, event_id: EventId) -> Self {
        self.stream_positions.insert(stream_id, event_id);
        self
    }

    /// Gets the position for a specific stream, if any.
    pub fn get_stream_position(&self, stream_id: &StreamId) -> Option<&EventId> {
        self.stream_positions.get(stream_id)
    }
}

/// The current status of a projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectionStatus {
    /// The projection is not running.
    Stopped,
    /// The projection is actively processing events.
    Running,
    /// The projection is paused and can be resumed.
    Paused,
    /// The projection has encountered an error and requires intervention.
    Faulted,
    /// The projection is being rebuilt from the beginning.
    Rebuilding,
}

impl ProjectionStatus {
    /// Returns true if the projection is actively processing events.
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Rebuilding)
    }

    /// Returns true if the projection can be started.
    pub const fn can_start(self) -> bool {
        matches!(self, Self::Stopped | Self::Paused)
    }

    /// Returns true if the projection can be paused.
    pub const fn can_pause(self) -> bool {
        matches!(self, Self::Running | Self::Rebuilding)
    }

    /// Returns true if the projection can be stopped.
    pub const fn can_stop(self) -> bool {
        !matches!(self, Self::Stopped)
    }
}

/// Configuration for projection behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionConfig {
    /// The name of the projection (must be unique).
    pub name: String,
    /// How often to save checkpoints (in number of events processed).
    pub checkpoint_frequency: u64,
    /// Maximum number of events to process in a single batch.
    pub batch_size: usize,
    /// Whether to start from the beginning when no checkpoint exists.
    pub start_from_beginning: bool,
    /// Streams to subscribe to (empty means all streams).
    pub streams: Vec<StreamId>,
}

impl ProjectionConfig {
    /// Creates a new projection configuration.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            checkpoint_frequency: 100,
            batch_size: 1000,
            start_from_beginning: true,
            streams: Vec::new(),
        }
    }

    /// Sets the checkpoint frequency.
    #[must_use]
    pub const fn with_checkpoint_frequency(mut self, frequency: u64) -> Self {
        self.checkpoint_frequency = frequency;
        self
    }

    /// Sets the batch size.
    #[must_use]
    pub const fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    /// Sets whether to start from the beginning.
    #[must_use]
    pub const fn with_start_from_beginning(mut self, start: bool) -> Self {
        self.start_from_beginning = start;
        self
    }

    /// Adds a stream to subscribe to.
    #[must_use]
    pub fn with_stream(mut self, stream_id: StreamId) -> Self {
        self.streams.push(stream_id);
        self
    }

    /// Sets the streams to subscribe to.
    #[must_use]
    pub fn with_streams(mut self, streams: Vec<StreamId>) -> Self {
        self.streams = streams;
        self
    }
}

/// Core trait for all projections.
///
/// Projections transform events into read models, maintaining their own state
/// and managing checkpoints for resumability. Each projection processes events
/// in order and maintains exactly-once processing semantics.
#[async_trait]
pub trait Projection: Send + Sync + Debug {
    /// The type of the projection's state.
    type State: Send + Sync + Debug + Clone;

    /// The type of events this projection processes.
    type Event: Send + Sync + Debug + PartialEq + Eq;

    /// Returns the configuration for this projection.
    fn config(&self) -> &ProjectionConfig;

    /// Returns the current state of the projection.
    async fn get_state(&self) -> ProjectionResult<Self::State>;

    /// Returns the current status of the projection.
    async fn get_status(&self) -> ProjectionResult<ProjectionStatus>;

    /// Loads the checkpoint from persistent storage.
    async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint>;

    /// Saves the checkpoint to persistent storage.
    async fn save_checkpoint(&self, checkpoint: ProjectionCheckpoint) -> ProjectionResult<()>;

    /// Processes a single event, updating the projection state.
    ///
    /// This method should be idempotent - processing the same event multiple
    /// times should produce the same result.
    async fn apply_event(
        &self,
        state: &mut Self::State,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<()>;

    /// Processes a batch of events efficiently.
    ///
    /// The default implementation processes events one by one, but projections
    /// can override this for batch optimizations.
    async fn apply_events(
        &self,
        state: &mut Self::State,
        events: &[Event<Self::Event>],
    ) -> ProjectionResult<()> {
        for event in events {
            self.apply_event(state, event).await?;
        }
        Ok(())
    }

    /// Initializes the projection state.
    ///
    /// This is called when starting a projection for the first time or
    /// when rebuilding from scratch.
    async fn initialize_state(&self) -> ProjectionResult<Self::State>;

    /// Called when the projection starts processing.
    async fn on_start(&self) -> ProjectionResult<()> {
        Ok(())
    }

    /// Called when the projection stops processing.
    async fn on_stop(&self) -> ProjectionResult<()> {
        Ok(())
    }

    /// Called when the projection is paused.
    async fn on_pause(&self) -> ProjectionResult<()> {
        Ok(())
    }

    /// Called when the projection is resumed.
    async fn on_resume(&self) -> ProjectionResult<()> {
        Ok(())
    }

    /// Called when an error occurs during processing.
    ///
    /// Projections can override this to implement custom error handling,
    /// such as logging, alerting, or recovery strategies.
    async fn on_error(&self, error: &ProjectionError) -> ProjectionResult<()> {
        tracing::error!("Projection error: {}", error);
        Ok(())
    }

    /// Determines if this projection should process the given event.
    ///
    /// This is called before `apply_event` and allows projections to filter
    /// events they don't care about. The default implementation accepts all events.
    fn should_process_event(&self, event: &Event<Self::Event>) -> bool {
        let _ = event; // Suppress unused parameter warning
        true
    }

    /// Returns the streams this projection is interested in.
    ///
    /// If this returns an empty vector, the projection will receive events
    /// from all streams.
    fn interested_streams(&self) -> Vec<StreamId> {
        self.config().streams.clone()
    }
}

/// A simple in-memory projection for testing and development.
///
/// This projection maintains its state and checkpoint in memory only,
/// making it suitable for testing but not for production use.
#[derive(Debug)]
pub struct InMemoryProjection<S, E> {
    config: ProjectionConfig,
    state: tokio::sync::RwLock<Option<S>>,
    checkpoint: tokio::sync::RwLock<ProjectionCheckpoint>,
    status: tokio::sync::RwLock<ProjectionStatus>,
    _phantom: std::marker::PhantomData<E>,
}

impl<S, E> InMemoryProjection<S, E>
where
    S: Send + Sync + Debug + Clone + Default,
    E: Send + Sync + Debug + PartialEq + Eq,
{
    /// Creates a new in-memory projection with the given configuration.
    pub fn new(config: ProjectionConfig) -> Self {
        Self {
            config,
            state: tokio::sync::RwLock::new(None),
            checkpoint: tokio::sync::RwLock::new(ProjectionCheckpoint::initial()),
            status: tokio::sync::RwLock::new(ProjectionStatus::Stopped),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Sets the status of the projection.
    pub async fn set_status(&self, status: ProjectionStatus) -> ProjectionResult<()> {
        {
            let mut current_status = self.status.write().await;
            *current_status = status;
        }
        Ok(())
    }
}

#[async_trait]
impl<S, E> Projection for InMemoryProjection<S, E>
where
    S: Send + Sync + Debug + Clone + Default + 'static,
    E: Send + Sync + Debug + PartialEq + Eq + 'static,
{
    type State = S;
    type Event = E;

    fn config(&self) -> &ProjectionConfig {
        &self.config
    }

    async fn get_state(&self) -> ProjectionResult<Self::State> {
        let state = self.state.read().await;
        Ok(state.clone().unwrap_or_default())
    }

    async fn get_status(&self) -> ProjectionResult<ProjectionStatus> {
        let status = self.status.read().await;
        Ok(*status)
    }

    async fn load_checkpoint(&self) -> ProjectionResult<ProjectionCheckpoint> {
        let checkpoint = self.checkpoint.read().await;
        Ok(checkpoint.clone())
    }

    async fn save_checkpoint(&self, checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
        {
            let mut current_checkpoint = self.checkpoint.write().await;
            *current_checkpoint = checkpoint;
        }
        Ok(())
    }

    async fn apply_event(
        &self,
        _state: &mut Self::State,
        _event: &Event<Self::Event>,
    ) -> ProjectionResult<()> {
        // Default implementation does nothing - projections should override this
        Ok(())
    }

    async fn initialize_state(&self) -> ProjectionResult<Self::State> {
        let state = S::default();
        {
            let mut current_state = self.state.write().await;
            *current_state = Some(state.clone());
        }
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EventId;

    #[test]
    fn projection_checkpoint_initial_has_no_events() {
        let checkpoint = ProjectionCheckpoint::initial();
        assert!(checkpoint.last_event_id.is_none());
        assert!(checkpoint.stream_positions.is_empty());
    }

    #[test]
    fn projection_checkpoint_from_event_id_sets_last_event() {
        let event_id = EventId::new();
        let checkpoint = ProjectionCheckpoint::from_event_id(event_id);
        assert_eq!(checkpoint.last_event_id, Some(event_id));
    }

    #[test]
    fn projection_checkpoint_with_event_id_updates_last_event() {
        let event_id1 = EventId::new();
        let event_id2 = EventId::new();

        let checkpoint = ProjectionCheckpoint::from_event_id(event_id1).with_event_id(event_id2);

        assert_eq!(checkpoint.last_event_id, Some(event_id2));
    }

    #[test]
    fn projection_checkpoint_with_stream_position_updates_streams() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event_id = EventId::new();

        let checkpoint =
            ProjectionCheckpoint::initial().with_stream_position(stream_id.clone(), event_id);

        assert_eq!(checkpoint.get_stream_position(&stream_id), Some(&event_id));
    }

    #[test]
    fn projection_status_is_active_checks() {
        assert!(ProjectionStatus::Running.is_active());
        assert!(ProjectionStatus::Rebuilding.is_active());
        assert!(!ProjectionStatus::Stopped.is_active());
        assert!(!ProjectionStatus::Paused.is_active());
        assert!(!ProjectionStatus::Faulted.is_active());
    }

    #[test]
    fn projection_status_can_start_checks() {
        assert!(ProjectionStatus::Stopped.can_start());
        assert!(ProjectionStatus::Paused.can_start());
        assert!(!ProjectionStatus::Running.can_start());
        assert!(!ProjectionStatus::Rebuilding.can_start());
        assert!(!ProjectionStatus::Faulted.can_start());
    }

    #[test]
    fn projection_status_can_pause_checks() {
        assert!(ProjectionStatus::Running.can_pause());
        assert!(ProjectionStatus::Rebuilding.can_pause());
        assert!(!ProjectionStatus::Stopped.can_pause());
        assert!(!ProjectionStatus::Paused.can_pause());
        assert!(!ProjectionStatus::Faulted.can_pause());
    }

    #[test]
    fn projection_status_can_stop_checks() {
        assert!(!ProjectionStatus::Stopped.can_stop());
        assert!(ProjectionStatus::Running.can_stop());
        assert!(ProjectionStatus::Paused.can_stop());
        assert!(ProjectionStatus::Faulted.can_stop());
        assert!(ProjectionStatus::Rebuilding.can_stop());
    }

    #[test]
    fn projection_config_builder_pattern() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let config = ProjectionConfig::new("test-projection")
            .with_checkpoint_frequency(50)
            .with_batch_size(500)
            .with_start_from_beginning(false)
            .with_stream(stream_id.clone());

        assert_eq!(config.name, "test-projection");
        assert_eq!(config.checkpoint_frequency, 50);
        assert_eq!(config.batch_size, 500);
        assert!(!config.start_from_beginning);
        assert_eq!(config.streams, vec![stream_id]);
    }

    #[test]
    fn projection_config_with_streams_replaces_existing() {
        let stream1 = StreamId::try_new("stream1").unwrap();
        let stream2 = StreamId::try_new("stream2").unwrap();
        let stream3 = StreamId::try_new("stream3").unwrap();

        let config = ProjectionConfig::new("test")
            .with_stream(stream1)
            .with_streams(vec![stream2.clone(), stream3.clone()]);

        assert_eq!(config.streams, vec![stream2, stream3]);
    }

    #[tokio::test]
    async fn in_memory_projection_initial_state() {
        let config = ProjectionConfig::new("test");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

        assert_eq!(projection.get_state().await.unwrap(), String::default());
        assert_eq!(
            projection.get_status().await.unwrap(),
            ProjectionStatus::Stopped
        );

        let checkpoint = projection.load_checkpoint().await.unwrap();
        assert!(checkpoint.last_event_id.is_none());
    }

    #[tokio::test]
    async fn in_memory_projection_save_and_load_checkpoint() {
        let config = ProjectionConfig::new("test");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

        let event_id = EventId::new();
        let checkpoint = ProjectionCheckpoint::from_event_id(event_id);

        projection
            .save_checkpoint(checkpoint.clone())
            .await
            .unwrap();
        let loaded = projection.load_checkpoint().await.unwrap();

        assert_eq!(loaded.last_event_id, Some(event_id));
    }

    #[tokio::test]
    async fn in_memory_projection_set_status() {
        let config = ProjectionConfig::new("test");
        let projection: InMemoryProjection<String, String> = InMemoryProjection::new(config);

        projection
            .set_status(ProjectionStatus::Running)
            .await
            .unwrap();
        assert_eq!(
            projection.get_status().await.unwrap(),
            ProjectionStatus::Running
        );
    }

    #[tokio::test]
    async fn in_memory_projection_initialize_state() {
        let config = ProjectionConfig::new("test");
        let projection: InMemoryProjection<Vec<i32>, String> = InMemoryProjection::new(config);

        let state = projection.initialize_state().await.unwrap();
        assert!(state.is_empty());

        let current_state = projection.get_state().await.unwrap();
        assert_eq!(current_state, state);
    }

    // Property tests for checkpoint ordering
    mod property_tests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn checkpoint_ordering_is_consistent(
                event_id1 in any::<[u8; 16]>(),
                event_id2 in any::<[u8; 16]>()
            ) {
                // Create valid v7 UUIDs
                let mut bytes1 = event_id1;
                bytes1[6] = (bytes1[6] & 0x0F) | 0x70;
                bytes1[8] = (bytes1[8] & 0x3F) | 0x80;

                let mut bytes2 = event_id2;
                bytes2[6] = (bytes2[6] & 0x0F) | 0x70;
                bytes2[8] = (bytes2[8] & 0x3F) | 0x80;

                let id1 = EventId::try_new(uuid::Uuid::from_bytes(bytes1)).unwrap();
                let id2 = EventId::try_new(uuid::Uuid::from_bytes(bytes2)).unwrap();

                let checkpoint1 = ProjectionCheckpoint::from_event_id(id1);
                let checkpoint2 = ProjectionCheckpoint::from_event_id(id2);

                // Checkpoint ordering should follow event ID ordering
                prop_assert_eq!(checkpoint1 < checkpoint2, id1 < id2);
                prop_assert_eq!(checkpoint1 == checkpoint2, id1 == id2);
                prop_assert_eq!(checkpoint1 > checkpoint2, id1 > id2);
            }

            #[test]
            fn projection_config_name_preserved(name in "[a-zA-Z0-9_-]{1,100}") {
                let config = ProjectionConfig::new(name.clone());
                prop_assert_eq!(config.name, name);
            }

            #[test]
            fn projection_config_checkpoint_frequency_preserved(freq in 1u64..=10000u64) {
                let config = ProjectionConfig::new("test")
                    .with_checkpoint_frequency(freq);
                prop_assert_eq!(config.checkpoint_frequency, freq);
            }

            #[test]
            fn projection_config_batch_size_preserved(size in 1usize..=10000usize) {
                let config = ProjectionConfig::new("test")
                    .with_batch_size(size);
                prop_assert_eq!(config.batch_size, size);
            }
        }
    }
}
