//! Event subscription system for processing events from event streams.
//!
//! This module provides the core abstractions for subscribing to and processing
//! events from the event store. Subscriptions can be used to build projections,
//! process managers, or any other event-driven components.

use crate::{
    cqrs::{CheckpointStore, CqrsError, InMemoryCheckpointStore},
    errors::{EventStoreError, ProjectionError},
    event_store::{EventStore, ReadOptions, StoredEvent},
    projection::ProjectionCheckpoint,
    types::{EventId, StreamId},
};
use async_trait::async_trait;
use nutype::nutype;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinHandle, time};

/// Options for configuring how a subscription processes events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubscriptionOptions {
    /// Subscribe to all events from all streams starting from the beginning.
    CatchUpFromBeginning,
    /// Subscribe to all events from all streams starting from a specific position.
    CatchUpFromPosition(SubscriptionPosition),
    /// Subscribe to new events only (live subscription).
    LiveOnly,
    /// Subscribe to specific streams only, starting from the beginning.
    SpecificStreamsFromBeginning(SpecificStreamsMode),
    /// Subscribe to specific streams only, starting from a position.
    SpecificStreamsFromPosition(SpecificStreamsMode, SubscriptionPosition),
    /// Subscribe to all streams from a specific event position.
    AllStreams {
        /// Starting position for the subscription.
        from_position: Option<EventId>,
    },
    /// Subscribe to specific streams from a position.
    SpecificStreams {
        /// The streams to subscribe to.
        streams: Vec<StreamId>,
        /// Starting position for the subscription.
        from_position: Option<EventId>,
    },
}

/// Mode for subscribing to specific streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecificStreamsMode {
    /// Subscribe to events from specific named streams.
    Named,
    /// Subscribe to events from streams matching a pattern.
    Pattern,
}

/// Represents a position in the global event stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SubscriptionPosition {
    /// The last processed event ID.
    pub last_event_id: EventId,
    /// Per-stream checkpoints for tracking progress.
    pub stream_checkpoints: BTreeMap<StreamId, Checkpoint>,
}

impl SubscriptionPosition {
    /// Creates a new subscription position.
    pub const fn new(last_event_id: EventId) -> Self {
        Self {
            last_event_id,
            stream_checkpoints: BTreeMap::new(),
        }
    }

    /// Updates the checkpoint for a specific stream.
    pub fn update_checkpoint(&mut self, stream_id: StreamId, checkpoint: Checkpoint) {
        self.stream_checkpoints.insert(stream_id, checkpoint);
    }

    /// Gets the checkpoint for a specific stream.
    pub fn get_checkpoint(&self, stream_id: &StreamId) -> Option<&Checkpoint> {
        self.stream_checkpoints.get(stream_id)
    }
}

/// Checkpoint tracking for a specific stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The event ID of the last processed event in this stream.
    pub event_id: EventId,
    /// The version number of the last processed event.
    pub version: u64,
}

impl Checkpoint {
    /// Creates a new checkpoint.
    pub const fn new(event_id: EventId, version: u64) -> Self {
        Self { event_id, version }
    }
}

/// Name for a subscription, used for checkpoint storage and identification.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        AsRef,
        Deref,
        Serialize,
        Deserialize
    )
)]
pub struct SubscriptionName(String);

/// Result type for subscription operations.
pub type SubscriptionResult<T> = Result<T, SubscriptionError>;

/// Errors that can occur during subscription processing.
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    /// Event store error occurred.
    #[error("Event store error: {0}")]
    EventStore(#[from] EventStoreError),

    /// Projection error occurred.
    #[error("Projection error: {0}")]
    Projection(#[from] ProjectionError),

    /// Subscription was cancelled.
    #[error("Subscription cancelled")]
    Cancelled,

    /// Failed to save checkpoint.
    #[error("Failed to save checkpoint: {0}")]
    CheckpointSaveFailed(String),

    /// Failed to load checkpoint.
    #[error("Failed to load checkpoint: {0}")]
    CheckpointLoadFailed(String),
}

/// Trait for processing events from a subscription.
#[async_trait]
pub trait EventProcessor: Send + Sync {
    /// The type of events this processor handles.
    type Event: Send + Sync;

    /// Processes a single event.
    async fn process_event(&mut self, event: StoredEvent<Self::Event>) -> SubscriptionResult<()>
    where
        Self::Event: PartialEq + Eq + Clone;

    /// Called when the subscription catches up to the live position.
    async fn on_live(&mut self) -> SubscriptionResult<()> {
        Ok(())
    }
}

/// Trait for managing event subscriptions.
#[async_trait]
pub trait Subscription: Send + Sync {
    /// The type of events this subscription handles.
    type Event: Send + Sync;

    /// Starts the subscription with the given processor.
    async fn start(
        &mut self,
        name: SubscriptionName,
        options: SubscriptionOptions,
        processor: Box<dyn EventProcessor<Event = Self::Event>>,
    ) -> SubscriptionResult<()>
    where
        Self::Event: PartialEq + Eq + Clone;

    /// Stops the subscription.
    async fn stop(&mut self) -> SubscriptionResult<()>;

    /// Pauses the subscription.
    async fn pause(&mut self) -> SubscriptionResult<()>;

    /// Resumes the subscription.
    async fn resume(&mut self) -> SubscriptionResult<()>;

    /// Gets the current position of the subscription.
    async fn get_position(&self) -> SubscriptionResult<Option<SubscriptionPosition>>;

    /// Saves a checkpoint for the subscription.
    async fn save_checkpoint(&mut self, position: SubscriptionPosition) -> SubscriptionResult<()>;

    /// Loads the last saved checkpoint for the subscription.
    async fn load_checkpoint(
        &self,
        name: &SubscriptionName,
    ) -> SubscriptionResult<Option<SubscriptionPosition>>;
}

/// Internal state of a subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubscriptionState {
    Stopped,
    Running,
    Paused,
}

/// Shared state for a subscription.
struct SubscriptionSharedState {
    state: AtomicBool, // true = running, false = stopped/paused
    current_position: Mutex<Option<SubscriptionPosition>>,
}

impl SubscriptionSharedState {
    const fn new() -> Self {
        Self {
            state: AtomicBool::new(false),
            current_position: Mutex::new(None),
        }
    }

    fn is_running(&self) -> bool {
        self.state.load(Ordering::Acquire)
    }

    fn set_running(&self, running: bool) {
        self.state.store(running, Ordering::Release);
    }

    fn update_position(&self, position: SubscriptionPosition) {
        if let Ok(mut current) = self.current_position.lock() {
            *current = Some(position);
        }
    }

    fn get_position(&self) -> Option<SubscriptionPosition> {
        self.current_position.lock().ok().and_then(|p| p.clone())
    }
}

/// Full implementation for subscription functionality.
pub struct SubscriptionImpl<E> {
    event_store: Arc<dyn EventStore<Event = E>>,
    checkpoint_store: Arc<dyn CheckpointStore<Error = CqrsError>>,
    shared_state: Arc<SubscriptionSharedState>,
    task_handle: Mutex<Option<JoinHandle<()>>>,
    shutdown_signal: Mutex<Option<oneshot::Sender<()>>>,
    poll_interval: Duration,
    _phantom: std::marker::PhantomData<E>,
}

impl<E> SubscriptionImpl<E>
where
    E: Send + Sync + 'static,
{
    /// Creates a new subscription implementation.
    pub fn new(event_store: Arc<dyn EventStore<Event = E>>) -> Self {
        Self {
            event_store,
            checkpoint_store: Arc::new(InMemoryCheckpointStore::new()),
            shared_state: Arc::new(SubscriptionSharedState::new()),
            task_handle: Mutex::new(None),
            shutdown_signal: Mutex::new(None),
            poll_interval: Duration::from_millis(100),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Creates a new subscription implementation with custom checkpoint store.
    pub fn with_checkpoint_store(
        event_store: Arc<dyn EventStore<Event = E>>,
        checkpoint_store: Arc<dyn CheckpointStore<Error = CqrsError>>,
    ) -> Self {
        Self {
            event_store,
            checkpoint_store,
            shared_state: Arc::new(SubscriptionSharedState::new()),
            task_handle: Mutex::new(None),
            shutdown_signal: Mutex::new(None),
            poll_interval: Duration::from_millis(100),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Sets the polling interval for the subscription.
    #[must_use]
    pub const fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }
}

#[async_trait]
impl<E> Subscription for SubscriptionImpl<E>
where
    E: Send + Sync + 'static,
{
    type Event = E;

    #[allow(clippy::too_many_lines)]
    async fn start(
        &mut self,
        name: SubscriptionName,
        options: SubscriptionOptions,
        mut processor: Box<dyn EventProcessor<Event = Self::Event>>,
    ) -> SubscriptionResult<()>
    where
        Self::Event: PartialEq + Eq + Clone,
    {
        // Check if already running
        if self.shared_state.is_running() {
            return Err(SubscriptionError::CheckpointSaveFailed(
                "Subscription is already running".to_string(),
            ));
        }

        // Load checkpoint to determine starting position
        let starting_position = self.load_checkpoint(&name).await?;

        // Set up shutdown channel
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        if let Ok(mut signal) = self.shutdown_signal.lock() {
            *signal = Some(shutdown_tx);
        }

        // Clone references for the background task
        let event_store = Arc::clone(&self.event_store);
        let checkpoint_store = Arc::clone(&self.checkpoint_store);
        let shared_state = Arc::clone(&self.shared_state);
        let name_clone = name.clone();
        let poll_interval = self.poll_interval;

        // Start background processing task
        let task_handle = tokio::spawn(async move {
            let mut current_position = starting_position;
            let mut has_caught_up = false;

            shared_state.set_running(true);

            loop {
                // Check for shutdown signal
                if shutdown_rx.try_recv() == Ok(()) {
                    break;
                }

                // Check if paused
                if !shared_state.is_running() {
                    time::sleep(Duration::from_millis(10)).await;
                    continue;
                }

                // Determine read options based on subscription options and current position
                let read_options = match (&options, &current_position) {
                    (SubscriptionOptions::CatchUpFromBeginning, None) => ReadOptions::default(),
                    (SubscriptionOptions::CatchUpFromPosition(pos), None) => {
                        // Start from the specified position
                        current_position = Some(pos.clone());
                        ReadOptions::default().with_max_events(100)
                    }
                    (SubscriptionOptions::LiveOnly, None) => {
                        // For live-only, we skip to the end first
                        has_caught_up = true;
                        ReadOptions::default().with_max_events(100)
                    }
                    (_, Some(_pos)) => {
                        // Continue from last processed position
                        ReadOptions::default().with_max_events(100)
                    }
                    _ => ReadOptions::default().with_max_events(100),
                };

                // Determine which streams to read based on options
                let streams_to_read = match &options {
                    SubscriptionOptions::SpecificStreams { streams, .. } => streams.clone(),
                    SubscriptionOptions::SpecificStreamsFromBeginning(
                        SpecificStreamsMode::Named,
                    ) => {
                        // For now, read all streams - would need stream discovery in real implementation
                        vec![]
                    }
                    _ => {
                        // Read all streams - would need stream discovery in real implementation
                        vec![]
                    }
                };

                // For simplicity in this implementation, we'll simulate event polling
                // In a real implementation, this would query the event store for new events
                if streams_to_read.is_empty() {
                    // No specific streams to read, so we wait
                    time::sleep(poll_interval).await;
                    continue;
                }

                // Read events from the event store
                match event_store
                    .read_streams(&streams_to_read, &read_options)
                    .await
                {
                    Ok(stream_data) => {
                        if stream_data.events.is_empty() {
                            // No new events, mark as caught up if we weren't already
                            if !has_caught_up {
                                has_caught_up = true;
                                if let Err(e) = processor.on_live().await {
                                    eprintln!("Error in on_live callback: {e}");
                                }
                            }
                            time::sleep(poll_interval).await;
                            continue;
                        }

                        // Process each event
                        for event in stream_data.events() {
                            // Skip events we've already processed
                            if let Some(pos) = &current_position {
                                if event.event_id <= pos.last_event_id {
                                    continue;
                                }
                            }

                            // Process the event
                            if let Err(e) = processor.process_event(event.clone()).await {
                                eprintln!("Error processing event {}: {e}", event.event_id);
                                continue;
                            }

                            // Update position
                            let mut new_position = current_position
                                .clone()
                                .unwrap_or_else(|| SubscriptionPosition::new(event.event_id));
                            new_position.last_event_id = event.event_id;
                            new_position.update_checkpoint(
                                event.stream_id.clone(),
                                Checkpoint::new(event.event_id, event.event_version.into()),
                            );

                            current_position = Some(new_position.clone());
                            shared_state.update_position(new_position.clone());

                            // Periodically save checkpoint
                            if let Err(e) = Self::save_checkpoint_internal(
                                &*checkpoint_store,
                                &name_clone,
                                new_position,
                            )
                            .await
                            {
                                eprintln!("Error saving checkpoint: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading events: {e}");
                        time::sleep(poll_interval).await;
                    }
                }

                time::sleep(poll_interval).await;
            }

            // Final checkpoint save before shutdown
            if let Some(position) = shared_state.get_position() {
                if let Err(e) =
                    Self::save_checkpoint_internal(&*checkpoint_store, &name_clone, position).await
                {
                    eprintln!("Error saving final checkpoint: {e}");
                }
            }

            shared_state.set_running(false);
        });

        // Store the task handle
        if let Ok(mut handle) = self.task_handle.lock() {
            *handle = Some(task_handle);
        }

        Ok(())
    }

    async fn stop(&mut self) -> SubscriptionResult<()> {
        // Signal shutdown
        if let Ok(mut signal) = self.shutdown_signal.lock() {
            if let Some(tx) = signal.take() {
                let _ = tx.send(());
            }
        }

        // Extract the task handle first to avoid holding the mutex across await
        let task_handle = self
            .task_handle
            .lock()
            .map_or_else(|_| None, |mut handle| handle.take());

        // Wait for the task to complete
        if let Some(task) = task_handle {
            let _ = task.await;
        }

        self.shared_state.set_running(false);
        Ok(())
    }

    async fn pause(&mut self) -> SubscriptionResult<()> {
        self.shared_state.set_running(false);
        Ok(())
    }

    async fn resume(&mut self) -> SubscriptionResult<()> {
        self.shared_state.set_running(true);
        Ok(())
    }

    async fn get_position(&self) -> SubscriptionResult<Option<SubscriptionPosition>> {
        Ok(self.shared_state.get_position())
    }

    async fn save_checkpoint(&mut self, position: SubscriptionPosition) -> SubscriptionResult<()> {
        self.shared_state.update_position(position);

        // Note: This method doesn't have a subscription name, so we can't persist to checkpoint store
        // This would typically be called internally with a known subscription name
        Ok(())
    }

    async fn load_checkpoint(
        &self,
        name: &SubscriptionName,
    ) -> SubscriptionResult<Option<SubscriptionPosition>> {
        // Load from checkpoint store
        match self.checkpoint_store.load(name.as_ref()).await {
            Ok(Some(checkpoint)) => {
                // Convert ProjectionCheckpoint to SubscriptionPosition
                let position = if let Some(last_event_id) = checkpoint.last_event_id {
                    let mut position = SubscriptionPosition::new(last_event_id);
                    for (stream_id, event_id) in checkpoint.stream_positions {
                        position.update_checkpoint(
                            stream_id,
                            Checkpoint::new(event_id, 0), // Version not tracked in ProjectionCheckpoint
                        );
                    }
                    Some(position)
                } else {
                    None
                };
                Ok(position)
            }
            Ok(None) => Ok(None),
            Err(e) => Err(SubscriptionError::CheckpointLoadFailed(e.to_string())),
        }
    }
}

impl<E> SubscriptionImpl<E>
where
    E: Send + Sync + 'static,
{
    /// Internal helper to save checkpoints with a subscription name.
    async fn save_checkpoint_internal(
        checkpoint_store: &dyn CheckpointStore<Error = CqrsError>,
        name: &SubscriptionName,
        position: SubscriptionPosition,
    ) -> SubscriptionResult<()> {
        // Convert SubscriptionPosition to ProjectionCheckpoint
        let checkpoint = ProjectionCheckpoint {
            last_event_id: Some(position.last_event_id),
            checkpoint_time: crate::types::Timestamp::now(),
            stream_positions: position
                .stream_checkpoints
                .into_iter()
                .map(|(stream_id, checkpoint)| (stream_id, checkpoint.event_id))
                .collect(),
        };

        checkpoint_store
            .save(name.as_ref(), checkpoint)
            .await
            .map_err(|e| SubscriptionError::CheckpointSaveFailed(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use uuid::Uuid;

    // Property test generators
    prop_compose! {
        fn arb_event_id()(uuid in any::<u128>()) -> EventId {
            let uuid = Uuid::from_u128(uuid);
            // Create a UUIDv7 with sortrand version
            let mut bytes = *uuid.as_bytes();
            // Set version bits for UUIDv7 (0111 in the version field)
            bytes[6] = (bytes[6] & 0x0F) | 0x70;
            // Set variant bits
            bytes[8] = (bytes[8] & 0x3F) | 0x80;
            EventId::try_new(Uuid::from_bytes(bytes)).unwrap()
        }
    }

    prop_compose! {
        fn arb_stream_id()(s in "[a-zA-Z0-9_-]{1,50}") -> StreamId {
            StreamId::try_new(s).unwrap()
        }
    }

    prop_compose! {
        fn arb_checkpoint()(
            event_id in arb_event_id(),
            version in 0u64..1_000_000
        ) -> Checkpoint {
            Checkpoint::new(event_id, version)
        }
    }

    prop_compose! {
        fn arb_subscription_position()(
            last_event_id in arb_event_id(),
            checkpoints in prop::collection::btree_map(
                arb_stream_id(),
                arb_checkpoint(),
                0..10
            )
        ) -> SubscriptionPosition {
            let mut pos = SubscriptionPosition::new(last_event_id);
            for (stream_id, checkpoint) in checkpoints {
                pos.update_checkpoint(stream_id, checkpoint);
            }
            pos
        }
    }

    proptest! {
        #[test]
        fn subscription_position_ordering_respects_event_id(
            pos1 in arb_subscription_position(),
            pos2 in arb_subscription_position()
        ) {
            // Subscription positions should be ordered by their last_event_id
            let ordering = pos1.cmp(&pos2);
            let event_ordering = pos1.last_event_id.cmp(&pos2.last_event_id);
            prop_assert_eq!(ordering, event_ordering);
        }

        #[test]
        fn checkpoint_ordering_respects_event_id_then_version(
            checkpoint1 in arb_checkpoint(),
            checkpoint2 in arb_checkpoint()
        ) {
            // Checkpoints should be ordered first by event_id, then by version
            let ordering = checkpoint1.cmp(&checkpoint2);
            let expected = match checkpoint1.event_id.cmp(&checkpoint2.event_id) {
                std::cmp::Ordering::Equal => checkpoint1.version.cmp(&checkpoint2.version),
                other => other,
            };
            prop_assert_eq!(ordering, expected);
        }

        #[test]
        fn subscription_position_update_checkpoint_updates_correctly(
            mut position in arb_subscription_position(),
            stream_id in arb_stream_id(),
            checkpoint in arb_checkpoint()
        ) {
            position.update_checkpoint(stream_id.clone(), checkpoint);
            prop_assert_eq!(position.get_checkpoint(&stream_id), Some(&checkpoint));
        }

        #[test]
        fn subscription_position_get_checkpoint_returns_none_for_missing_stream(
            position in arb_subscription_position(),
            missing_stream in arb_stream_id()
        ) {
            // Ensure the missing stream is not in the position
            prop_assume!(!position.stream_checkpoints.contains_key(&missing_stream));
            prop_assert_eq!(position.get_checkpoint(&missing_stream), None);
        }
    }

    #[test]
    fn test_subscription_name_validation() {
        // Valid names
        assert!(SubscriptionName::try_new("valid_name").is_ok());
        assert!(SubscriptionName::try_new("projection-1").is_ok());
        assert!(SubscriptionName::try_new("MyProjection").is_ok());

        // Invalid names
        assert!(SubscriptionName::try_new("").is_err()); // Empty
        assert!(SubscriptionName::try_new("   ").is_err()); // Only whitespace
        assert!(SubscriptionName::try_new("a".repeat(256)).is_err()); // Too long
    }

    #[test]
    fn test_subscription_options() {
        // Test serialization/deserialization roundtrip
        let options = vec![
            SubscriptionOptions::CatchUpFromBeginning,
            SubscriptionOptions::LiveOnly,
            SubscriptionOptions::SpecificStreamsFromBeginning(SpecificStreamsMode::Named),
            SubscriptionOptions::SpecificStreamsFromBeginning(SpecificStreamsMode::Pattern),
        ];

        for opt in options {
            let serialized = serde_json::to_string(&opt).unwrap();
            let deserialized: SubscriptionOptions = serde_json::from_str(&serialized).unwrap();
            assert_eq!(opt, deserialized);
        }
    }
}
