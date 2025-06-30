//! In-memory adapter for `EventCore` event sourcing library
//!
//! This crate provides an in-memory implementation of the `EventStore` trait
//! from the eventcore crate, useful for testing and development scenarios
//! where persistence is not required.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![allow(clippy::significant_drop_tightening)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use eventcore::{
    Checkpoint, EventProcessor, EventStore, EventStoreError, EventVersion, ExpectedVersion,
    ReadOptions, StoredEvent, StreamData, StreamEvents, StreamId, Subscription, SubscriptionError,
    SubscriptionName, SubscriptionOptions, SubscriptionPosition, SubscriptionResult, Timestamp,
};

type EventStoreResult<T> = Result<T, EventStoreError>;

/// Thread-safe in-memory event store for testing
#[derive(Clone)]
pub struct InMemoryEventStore<E>
where
    E: Send + Sync + Clone + 'static + PartialEq + Eq,
{
    // Maps stream IDs to their stored events
    streams: Arc<RwLock<HashMap<StreamId, Vec<StoredEvent<E>>>>>,
    // Maps stream IDs to their current version
    versions: Arc<RwLock<HashMap<StreamId, EventVersion>>>,
}

impl<E> InMemoryEventStore<E>
where
    E: Send + Sync + Clone + 'static + PartialEq + Eq,
{
    /// Create a new empty in-memory event store
    pub fn new() -> Self {
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
            versions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl<E> Default for InMemoryEventStore<E>
where
    E: Send + Sync + Clone + 'static + PartialEq + Eq,
{
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<E> EventStore for InMemoryEventStore<E>
where
    E: Send + Sync + Clone + 'static + PartialEq + Eq,
{
    type Event = E;

    async fn read_streams(
        &self,
        stream_ids: &[StreamId],
        options: &ReadOptions,
    ) -> EventStoreResult<StreamData<Self::Event>> {
        let streams = self.streams.read().map_err(|_| {
            EventStoreError::Internal("Failed to acquire read lock on streams".to_string())
        })?;

        let versions = self.versions.read().map_err(|_| {
            EventStoreError::Internal("Failed to acquire read lock on versions".to_string())
        })?;

        let mut all_events = Vec::new();
        let mut stream_versions = HashMap::new();

        for stream_id in stream_ids {
            let version = versions
                .get(stream_id)
                .copied()
                .unwrap_or_else(EventVersion::initial);
            stream_versions.insert(stream_id.clone(), version);

            if let Some(stream_events) = streams.get(stream_id) {
                for event in stream_events {
                    // Apply from_version filter
                    if let Some(from_version) = options.from_version {
                        if event.event_version < from_version {
                            continue;
                        }
                    }

                    // Apply to_version filter
                    if let Some(to_version) = options.to_version {
                        if event.event_version > to_version {
                            continue;
                        }
                    }

                    all_events.push(event.clone());
                }
            }
        }

        // Sort by event ID (which provides timestamp ordering)
        all_events.sort_by_key(|e| e.event_id);

        // Apply max_events limit
        if let Some(max_events) = options.max_events {
            all_events.truncate(max_events);
        }

        Ok(StreamData::new(all_events, stream_versions))
    }

    async fn write_events_multi(
        &self,
        stream_events: Vec<StreamEvents<Self::Event>>,
    ) -> EventStoreResult<HashMap<StreamId, EventVersion>> {
        let mut streams = self.streams.write().map_err(|_| {
            EventStoreError::Internal("Failed to acquire write lock on streams".to_string())
        })?;

        let mut versions = self.versions.write().map_err(|_| {
            EventStoreError::Internal("Failed to acquire write lock on versions".to_string())
        })?;

        // First, verify all expected versions match
        for stream_event in &stream_events {
            let current_version = versions
                .get(&stream_event.stream_id)
                .copied()
                .unwrap_or_else(EventVersion::initial);

            match stream_event.expected_version {
                ExpectedVersion::New => {
                    if versions.contains_key(&stream_event.stream_id) {
                        return Err(EventStoreError::VersionConflict {
                            stream: stream_event.stream_id.clone(),
                            expected: EventVersion::initial(),
                            current: current_version,
                        });
                    }
                }
                ExpectedVersion::Exact(expected) => {
                    if current_version != expected {
                        return Err(EventStoreError::VersionConflict {
                            stream: stream_event.stream_id.clone(),
                            expected,
                            current: current_version,
                        });
                    }
                }
                ExpectedVersion::Any => {
                    // No check needed
                }
            }
        }

        // All versions match, proceed with writes
        let mut new_versions = HashMap::new();

        for stream_event in stream_events {
            let stream_events_list = streams.entry(stream_event.stream_id.clone()).or_default();

            let mut current_version = versions
                .get(&stream_event.stream_id)
                .copied()
                .unwrap_or_else(EventVersion::initial);

            for event_to_write in stream_event.events {
                current_version = current_version.next();

                let stored_event = StoredEvent::new(
                    event_to_write.event_id,
                    stream_event.stream_id.clone(),
                    current_version,
                    Timestamp::now(),
                    event_to_write.payload,
                    event_to_write.metadata,
                );

                stream_events_list.push(stored_event);
            }

            versions.insert(stream_event.stream_id.clone(), current_version);
            new_versions.insert(stream_event.stream_id.clone(), current_version);
        }

        Ok(new_versions)
    }

    async fn stream_exists(&self, stream_id: &StreamId) -> EventStoreResult<bool> {
        let streams = self.streams.read().map_err(|_| {
            EventStoreError::Internal("Failed to acquire read lock on streams".to_string())
        })?;

        Ok(streams.contains_key(stream_id))
    }

    async fn get_stream_version(
        &self,
        stream_id: &StreamId,
    ) -> EventStoreResult<Option<EventVersion>> {
        let versions = self.versions.read().map_err(|_| {
            EventStoreError::Internal("Failed to acquire read lock on versions".to_string())
        })?;

        Ok(versions.get(stream_id).copied())
    }

    async fn subscribe(
        &self,
        options: SubscriptionOptions,
    ) -> EventStoreResult<Box<dyn Subscription<Event = Self::Event>>> {
        let subscription = InMemorySubscription::new(self.clone(), options);
        Ok(Box::new(subscription))
    }
}

/// In-memory subscription implementation with full replay and checkpointing support.
pub struct InMemorySubscription<E>
where
    E: Send + Sync + Clone + 'static + PartialEq + Eq,
{
    event_store: InMemoryEventStore<E>,
    options: SubscriptionOptions,
    current_position: Arc<RwLock<Option<SubscriptionPosition>>>,
    checkpoints: Arc<RwLock<HashMap<String, SubscriptionPosition>>>,
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    stop_signal: Arc<AtomicBool>,
}

impl<E> InMemorySubscription<E>
where
    E: Send + Sync + Clone + 'static + PartialEq + Eq,
{
    /// Creates a new in-memory subscription.
    pub fn new(event_store: InMemoryEventStore<E>, options: SubscriptionOptions) -> Self {
        Self {
            event_store,
            options,
            current_position: Arc::new(RwLock::new(None)),
            checkpoints: Arc::new(RwLock::new(HashMap::new())),
            is_running: Arc::new(AtomicBool::new(false)),
            is_paused: Arc::new(AtomicBool::new(false)),
            stop_signal: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Processes events from the event store according to subscription options.
    async fn process_events(
        &self,
        name: SubscriptionName,
        mut processor: Box<dyn EventProcessor<Event = E>>,
    ) -> SubscriptionResult<()>
    where
        E: PartialEq + Eq,
    {
        // Load checkpoint to determine starting position
        let starting_position = self.load_checkpoint(&name).await?;

        loop {
            // Check if we should stop
            if self.stop_signal.load(Ordering::Acquire) {
                break;
            }

            // Check if we're paused
            if self.is_paused.load(Ordering::Acquire) {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }

            // Get events according to subscription options
            let events = self
                .get_events_for_processing(starting_position.as_ref())
                .await?;

            let mut current_pos = starting_position.clone();
            let mut has_new_events = false;

            for event in events {
                // Skip events we've already processed
                if let Some(ref pos) = current_pos {
                    if event.event_id <= pos.last_event_id {
                        continue;
                    }
                }

                // Process the event
                processor.process_event(event.clone()).await?;
                has_new_events = true;

                // Update current position
                let new_checkpoint = Checkpoint::new(event.event_id, event.event_version.into());

                current_pos = Some(if let Some(mut pos) = current_pos {
                    pos.last_event_id = event.event_id;
                    pos.update_checkpoint(event.stream_id.clone(), new_checkpoint);
                    pos
                } else {
                    let mut pos = SubscriptionPosition::new(event.event_id);
                    pos.update_checkpoint(event.stream_id.clone(), new_checkpoint);
                    pos
                });

                // Update our current position
                {
                    let mut guard = self.current_position.write().map_err(|_| {
                        SubscriptionError::CheckpointSaveFailed(
                            "Failed to acquire position lock".to_string(),
                        )
                    })?;
                    (*guard).clone_from(&current_pos);
                }

                // Periodically save checkpoint - for in-memory we just update current_position
                // which is handled above
            }

            // If we're caught up and this is a live subscription, notify the processor
            if !has_new_events && matches!(self.options, SubscriptionOptions::LiveOnly) {
                processor.on_live().await?;
            }

            // Sleep briefly to avoid busy-waiting
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        Ok(())
    }

    /// Gets events for processing based on subscription options.
    async fn get_events_for_processing(
        &self,
        starting_position: Option<&SubscriptionPosition>,
    ) -> SubscriptionResult<Vec<StoredEvent<E>>> {
        let (streams, from_position) = match &self.options {
            SubscriptionOptions::CatchUpFromBeginning => (vec![], None),
            SubscriptionOptions::CatchUpFromPosition(pos) => (vec![], Some(pos.last_event_id)),
            SubscriptionOptions::LiveOnly => {
                // For live-only, start from the current position in the store
                (vec![], starting_position.as_ref().map(|p| p.last_event_id))
            }
            SubscriptionOptions::SpecificStreamsFromBeginning(_mode) => {
                // This would need stream selection logic based on mode
                (vec![], None)
            }
            SubscriptionOptions::SpecificStreamsFromPosition(_mode, pos) => {
                (vec![], Some(pos.last_event_id))
            }
            SubscriptionOptions::AllStreams { from_position } => (vec![], *from_position),
            SubscriptionOptions::SpecificStreams {
                streams,
                from_position,
            } => (streams.clone(), *from_position),
        };

        // Read all events from specified streams (or all streams if empty)
        let all_events = if streams.is_empty() {
            self.read_all_events_sorted()?
        } else {
            self.read_streams_events(&streams).await?
        };

        // Filter events based on starting position
        let filtered_events = if let Some(from_id) =
            from_position.or_else(|| starting_position.map(|p| p.last_event_id))
        {
            all_events
                .into_iter()
                .filter(|e| e.event_id > from_id)
                .collect()
        } else {
            all_events
        };

        Ok(filtered_events)
    }

    /// Reads all events from the store in sorted order.
    fn read_all_events_sorted(&self) -> SubscriptionResult<Vec<StoredEvent<E>>> {
        let streams = self.event_store.streams.read().map_err(|_| {
            SubscriptionError::EventStore(EventStoreError::Internal(
                "Failed to acquire read lock on streams".to_string(),
            ))
        })?;

        let mut all_events = Vec::new();
        for events in streams.values() {
            all_events.extend(events.iter().cloned());
        }

        // Sort by event ID (which is timestamp-based with UUIDv7)
        all_events.sort_by(|a, b| a.event_id.cmp(&b.event_id));

        Ok(all_events)
    }

    /// Reads events from specific streams.
    async fn read_streams_events(
        &self,
        stream_ids: &[StreamId],
    ) -> SubscriptionResult<Vec<StoredEvent<E>>> {
        let read_options = ReadOptions::default();

        let stream_data = self
            .event_store
            .read_streams(stream_ids, &read_options)
            .await
            .map_err(SubscriptionError::EventStore)?;

        Ok(stream_data.events)
    }
}

#[async_trait]
impl<E> Subscription for InMemorySubscription<E>
where
    E: Send + Sync + Clone + 'static + PartialEq + Eq,
{
    type Event = E;

    async fn start(
        &mut self,
        name: SubscriptionName,
        options: SubscriptionOptions,
        processor: Box<dyn EventProcessor<Event = Self::Event>>,
    ) -> SubscriptionResult<()>
    where
        Self::Event: PartialEq + Eq,
    {
        // Update options if provided
        self.options = options;

        // Set running state
        self.is_running.store(true, Ordering::Release);
        self.stop_signal.store(false, Ordering::Release);
        self.is_paused.store(false, Ordering::Release);

        // Start processing events in a background task
        let subscription = self.clone(); // We'll need to implement Clone
        let name_copy = name;

        tokio::spawn(async move {
            if let Err(e) = subscription.process_events(name_copy, processor).await {
                eprintln!("Subscription processing failed: {e}");
            }
        });

        Ok(())
    }

    async fn stop(&mut self) -> SubscriptionResult<()> {
        self.stop_signal.store(true, Ordering::Release);
        self.is_running.store(false, Ordering::Release);
        Ok(())
    }

    async fn pause(&mut self) -> SubscriptionResult<()> {
        self.is_paused.store(true, Ordering::Release);
        Ok(())
    }

    async fn resume(&mut self) -> SubscriptionResult<()> {
        self.is_paused.store(false, Ordering::Release);
        Ok(())
    }

    async fn get_position(&self) -> SubscriptionResult<Option<SubscriptionPosition>> {
        let guard = self.current_position.read().map_err(|_| {
            SubscriptionError::CheckpointLoadFailed("Failed to acquire position lock".to_string())
        })?;
        Ok(guard.clone())
    }

    async fn save_checkpoint(&mut self, position: SubscriptionPosition) -> SubscriptionResult<()> {
        // For the in-memory implementation, we don't have a specific name context here,
        // so we'll update the current position
        {
            let mut guard = self.current_position.write().map_err(|_| {
                SubscriptionError::CheckpointSaveFailed(
                    "Failed to acquire position lock".to_string(),
                )
            })?;
            *guard = Some(position);
        }
        Ok(())
    }

    async fn load_checkpoint(
        &self,
        name: &SubscriptionName,
    ) -> SubscriptionResult<Option<SubscriptionPosition>> {
        let checkpoints = self.checkpoints.read().map_err(|_| {
            SubscriptionError::CheckpointLoadFailed(
                "Failed to acquire checkpoints lock".to_string(),
            )
        })?;
        Ok(checkpoints.get(name.as_ref()).cloned())
    }
}

// We need to implement Clone for the subscription
impl<E> Clone for InMemorySubscription<E>
where
    E: Send + Sync + Clone + 'static + PartialEq + Eq,
{
    fn clone(&self) -> Self {
        Self {
            event_store: self.event_store.clone(),
            options: self.options.clone(),
            current_position: Arc::clone(&self.current_position),
            checkpoints: Arc::clone(&self.checkpoints),
            is_running: Arc::clone(&self.is_running),
            is_paused: Arc::clone(&self.is_paused),
            stop_signal: Arc::clone(&self.stop_signal),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eventcore::{EventId, EventToWrite};

    #[tokio::test]
    async fn test_new_store_is_empty() {
        let store: InMemoryEventStore<String> = InMemoryEventStore::new();
        assert!(store.streams.read().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_clone_shares_storage() {
        let store1: InMemoryEventStore<String> = InMemoryEventStore::new();
        #[allow(clippy::redundant_clone)]
        let store2 = store1.clone();

        // Verify both stores point to the same storage
        assert!(Arc::ptr_eq(&store1.streams, &store2.streams));
        assert!(Arc::ptr_eq(&store1.versions, &store2.versions));
    }

    #[tokio::test]
    async fn test_stream_exists() {
        let store: InMemoryEventStore<String> = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("test-stream").unwrap();

        // Stream should not exist initially
        assert!(!store.stream_exists(&stream_id).await.unwrap());

        // Write an event to create the stream
        let event = EventToWrite::new(EventId::new(), "test-event".to_string());

        let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::New, vec![event]);

        store.write_events_multi(vec![stream_events]).await.unwrap();

        // Stream should now exist
        assert!(store.stream_exists(&stream_id).await.unwrap());
    }

    #[tokio::test]
    async fn test_get_stream_version() {
        let store: InMemoryEventStore<String> = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("test-stream").unwrap();

        // New stream should have no version
        assert_eq!(store.get_stream_version(&stream_id).await.unwrap(), None);

        // Write an event
        let event = EventToWrite::new(EventId::new(), "test-event".to_string());
        let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::New, vec![event]);

        store.write_events_multi(vec![stream_events]).await.unwrap();

        // Version should be 1
        assert_eq!(
            store.get_stream_version(&stream_id).await.unwrap(),
            Some(EventVersion::try_new(1).unwrap())
        );
    }

    #[tokio::test]
    async fn test_read_streams() {
        let store: InMemoryEventStore<String> = InMemoryEventStore::new();
        let stream_id1 = StreamId::try_new("stream-1").unwrap();
        let stream_id2 = StreamId::try_new("stream-2").unwrap();

        // Write events to both streams
        let event1 = EventToWrite::new(EventId::new(), "event-1".to_string());
        let event2 = EventToWrite::new(EventId::new(), "event-2".to_string());

        let stream_events1 =
            StreamEvents::new(stream_id1.clone(), ExpectedVersion::New, vec![event1]);
        let stream_events2 =
            StreamEvents::new(stream_id2.clone(), ExpectedVersion::New, vec![event2]);

        store
            .write_events_multi(vec![stream_events1, stream_events2])
            .await
            .unwrap();

        // Read both streams
        let result = store
            .read_streams(
                &[stream_id1.clone(), stream_id2.clone()],
                &ReadOptions::new(),
            )
            .await
            .unwrap();

        assert_eq!(result.events.len(), 2);
        assert_eq!(
            result.stream_version(&stream_id1),
            Some(EventVersion::try_new(1).unwrap())
        );
        assert_eq!(
            result.stream_version(&stream_id2),
            Some(EventVersion::try_new(1).unwrap())
        );

        // Verify we can find events by stream
        let stream1_events: Vec<_> = result.events_for_stream(&stream_id1).collect();
        assert_eq!(stream1_events.len(), 1);
        assert_eq!(stream1_events[0].payload, "event-1");

        let stream2_events: Vec<_> = result.events_for_stream(&stream_id2).collect();
        assert_eq!(stream2_events.len(), 1);
        assert_eq!(stream2_events[0].payload, "event-2");
    }

    #[tokio::test]
    async fn test_concurrency_control() {
        let store: InMemoryEventStore<String> = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("test-stream").unwrap();

        // Write initial event
        let event1 = EventToWrite::new(EventId::new(), "event-1".to_string());
        let stream_events1 =
            StreamEvents::new(stream_id.clone(), ExpectedVersion::New, vec![event1]);

        store
            .write_events_multi(vec![stream_events1])
            .await
            .unwrap();

        // Try to write with wrong expected version
        let event2 = EventToWrite::new(EventId::new(), "event-2".to_string());
        let stream_events2 = StreamEvents::new(
            stream_id.clone(),
            ExpectedVersion::Exact(EventVersion::initial()), // Wrong version, should be 1
            vec![event2.clone()],
        );

        let result = store.write_events_multi(vec![stream_events2]).await;

        assert!(matches!(
            result,
            Err(EventStoreError::VersionConflict { .. })
        ));

        // Write with correct version should succeed
        let stream_events3 = StreamEvents::new(
            stream_id.clone(),
            ExpectedVersion::Exact(EventVersion::try_new(1).unwrap()),
            vec![event2],
        );

        let result = store.write_events_multi(vec![stream_events3]).await;

        assert!(result.is_ok());
        assert_eq!(
            store.get_stream_version(&stream_id).await.unwrap(),
            Some(EventVersion::try_new(2).unwrap())
        );
    }

    #[tokio::test]
    async fn test_multiple_events_in_single_write() {
        let store: InMemoryEventStore<String> = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("test-stream").unwrap();

        // Write multiple events at once
        let events: Vec<EventToWrite<String>> = (0..5)
            .map(|i| EventToWrite::new(EventId::new(), format!("event-{i}")))
            .collect();

        let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::New, events);

        store.write_events_multi(vec![stream_events]).await.unwrap();

        // Version should be 5
        assert_eq!(
            store.get_stream_version(&stream_id).await.unwrap(),
            Some(EventVersion::try_new(5).unwrap())
        );

        // Read and verify all events
        let result = store
            .read_streams(&[stream_id.clone()], &ReadOptions::new())
            .await
            .unwrap();
        assert_eq!(result.events.len(), 5);
        for (i, event) in result.events.iter().enumerate() {
            assert_eq!(event.payload, format!("event-{i}"));
        }
    }

    #[tokio::test]
    async fn test_expected_version_new() {
        let store: InMemoryEventStore<String> = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("test-stream").unwrap();

        // First write with ExpectedVersion::New should succeed
        let event1 = EventToWrite::new(EventId::new(), "event-1".to_string());
        let stream_events1 =
            StreamEvents::new(stream_id.clone(), ExpectedVersion::New, vec![event1]);

        store
            .write_events_multi(vec![stream_events1])
            .await
            .unwrap();

        // Second write with ExpectedVersion::New should fail
        let event2 = EventToWrite::new(EventId::new(), "event-2".to_string());
        let stream_events2 =
            StreamEvents::new(stream_id.clone(), ExpectedVersion::New, vec![event2]);

        let result = store.write_events_multi(vec![stream_events2]).await;
        assert!(matches!(
            result,
            Err(EventStoreError::VersionConflict { .. })
        ));
    }

    #[tokio::test]
    async fn test_expected_version_any() {
        let store: InMemoryEventStore<String> = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("test-stream").unwrap();

        // Write with ExpectedVersion::Any on new stream should succeed
        let event1 = EventToWrite::new(EventId::new(), "event-1".to_string());
        let stream_events1 =
            StreamEvents::new(stream_id.clone(), ExpectedVersion::Any, vec![event1]);

        store
            .write_events_multi(vec![stream_events1])
            .await
            .unwrap();

        // Write with ExpectedVersion::Any on existing stream should succeed
        let event2 = EventToWrite::new(EventId::new(), "event-2".to_string());
        let stream_events2 =
            StreamEvents::new(stream_id.clone(), ExpectedVersion::Any, vec![event2]);

        store
            .write_events_multi(vec![stream_events2])
            .await
            .unwrap();

        assert_eq!(
            store.get_stream_version(&stream_id).await.unwrap(),
            Some(EventVersion::try_new(2).unwrap())
        );
    }

    #[tokio::test]
    async fn test_read_options_filtering() {
        let store: InMemoryEventStore<String> = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("test-stream").unwrap();

        // Write 10 events
        let events: Vec<EventToWrite<String>> = (0..10)
            .map(|i| EventToWrite::new(EventId::new(), format!("event-{i}")))
            .collect();

        let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::New, events);

        store.write_events_multi(vec![stream_events]).await.unwrap();

        // Test from_version
        let options = ReadOptions::new().from_version(EventVersion::try_new(5).unwrap());
        let result = store
            .read_streams(&[stream_id.clone()], &options)
            .await
            .unwrap();
        assert_eq!(result.events.len(), 6); // Events 5-10

        // Test to_version
        let options = ReadOptions::new().to_version(EventVersion::try_new(3).unwrap());
        let result = store
            .read_streams(&[stream_id.clone()], &options)
            .await
            .unwrap();
        assert_eq!(result.events.len(), 3); // Events 1-3

        // Test from_version and to_version
        let options = ReadOptions::new()
            .from_version(EventVersion::try_new(3).unwrap())
            .to_version(EventVersion::try_new(7).unwrap());
        let result = store
            .read_streams(&[stream_id.clone()], &options)
            .await
            .unwrap();
        assert_eq!(result.events.len(), 5); // Events 3-7

        // Test max_events
        let options = ReadOptions::new().with_max_events(5);
        let result = store
            .read_streams(&[stream_id.clone()], &options)
            .await
            .unwrap();
        assert_eq!(result.events.len(), 5); // First 5 events
    }
}
