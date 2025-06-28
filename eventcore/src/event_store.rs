//! Event store abstraction for the `EventCore` event sourcing library.
//!
//! This module defines the core `EventStore` trait that serves as the port interface
//! for different event store implementations. The trait is designed to be backend-independent
//! and support multi-stream atomic operations.

use crate::errors::EventStoreResult;
use crate::types::{EventId, EventVersion, StreamId, Timestamp};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Data returned when reading from one or more streams.
///
/// This type contains all events from the requested streams along with
/// metadata needed for version tracking and ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamData<E> {
    /// The events from all requested streams, ordered by `EventId` (timestamp-based)
    pub events: Vec<StoredEvent<E>>,
    /// The current version of each stream that was read
    pub stream_versions: HashMap<StreamId, EventVersion>,
}

impl<E> StreamData<E> {
    /// Creates a new `StreamData` instance.
    pub const fn new(
        events: Vec<StoredEvent<E>>,
        stream_versions: HashMap<StreamId, EventVersion>,
    ) -> Self {
        Self {
            events,
            stream_versions,
        }
    }

    /// Returns whether any events were found.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns the number of events in the stream data.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Gets the current version of a specific stream.
    pub fn stream_version(&self, stream_id: &StreamId) -> Option<EventVersion> {
        self.stream_versions.get(stream_id).copied()
    }

    /// Returns an iterator over the events.
    pub fn events(&self) -> impl Iterator<Item = &StoredEvent<E>> + '_ {
        self.events.iter()
    }

    /// Returns an iterator over events from a specific stream.
    pub fn events_for_stream(
        &self,
        stream_id: &StreamId,
    ) -> impl Iterator<Item = &StoredEvent<E>> + '_ {
        let stream_id = stream_id.clone();
        self.events
            .iter()
            .filter(move |event| event.stream_id == stream_id)
    }
}

/// A stored event with full metadata.
///
/// This represents an event as it exists in the event store, including
/// all metadata required for ordering, causation tracking, and version control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEvent<E> {
    /// Unique identifier for this event
    pub event_id: EventId,
    /// The stream this event belongs to
    pub stream_id: StreamId,
    /// The version of this event within its stream
    pub event_version: EventVersion,
    /// When this event was stored
    pub timestamp: Timestamp,
    /// The event payload
    pub payload: E,
    /// Optional metadata for this event
    pub metadata: Option<EventMetadata>,
}

impl<E> StoredEvent<E> {
    /// Creates a new stored event.
    pub const fn new(
        event_id: EventId,
        stream_id: StreamId,
        event_version: EventVersion,
        timestamp: Timestamp,
        payload: E,
        metadata: Option<EventMetadata>,
    ) -> Self {
        Self {
            event_id,
            stream_id,
            event_version,
            timestamp,
            payload,
            metadata,
        }
    }
}

/// Metadata that can be attached to events for tracking and correlation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventMetadata {
    /// ID of the event that caused this event (for causation tracking)
    pub causation_id: Option<EventId>,
    /// ID used to correlate related events across multiple commands
    pub correlation_id: Option<String>,
    /// ID of the user or system that initiated this event
    pub user_id: Option<String>,
    /// Additional custom metadata
    pub custom: HashMap<String, String>,
}

impl EventMetadata {
    /// Creates new empty metadata.
    pub fn new() -> Self {
        Self {
            causation_id: None,
            correlation_id: None,
            user_id: None,
            custom: HashMap::new(),
        }
    }

    /// Sets the causation ID.
    #[must_use]
    pub const fn with_causation_id(mut self, causation_id: EventId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    /// Sets the correlation ID.
    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: String) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    /// Sets the user ID.
    #[must_use]
    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    /// Adds custom metadata.
    #[must_use]
    pub fn with_custom(mut self, key: String, value: String) -> Self {
        self.custom.insert(key, value);
        self
    }
}

impl Default for EventMetadata {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for reading streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOptions {
    /// Maximum number of events to read (None = no limit)
    pub max_events: Option<usize>,
    /// Start reading from this version (inclusive). None = from beginning
    pub from_version: Option<EventVersion>,
    /// Stop reading at this version (inclusive). None = to end
    pub to_version: Option<EventVersion>,
}

impl ReadOptions {
    /// Creates new read options with default values.
    pub const fn new() -> Self {
        Self {
            max_events: None,
            from_version: None,
            to_version: None,
        }
    }

    /// Sets the maximum number of events to read.
    #[must_use]
    pub const fn with_max_events(mut self, max_events: usize) -> Self {
        self.max_events = Some(max_events);
        self
    }

    /// Sets the starting version.
    #[must_use]
    pub const fn from_version(mut self, version: EventVersion) -> Self {
        self.from_version = Some(version);
        self
    }

    /// Sets the ending version.
    #[must_use]
    pub const fn to_version(mut self, version: EventVersion) -> Self {
        self.to_version = Some(version);
        self
    }
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Expected version for optimistic concurrency control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedVersion {
    /// The stream must not exist
    New,
    /// The stream must exist and have exactly this version
    Exact(EventVersion),
    /// Any version is acceptable (no concurrency control)
    Any,
}

/// Events to write to a specific stream.
#[derive(Debug, Clone)]
pub struct StreamEvents<E> {
    /// The target stream
    pub stream_id: StreamId,
    /// The expected version for optimistic concurrency control
    pub expected_version: ExpectedVersion,
    /// The events to write
    pub events: Vec<EventToWrite<E>>,
}

impl<E> StreamEvents<E> {
    /// Creates a new `StreamEvents` instance.
    pub const fn new(
        stream_id: StreamId,
        expected_version: ExpectedVersion,
        events: Vec<EventToWrite<E>>,
    ) -> Self {
        Self {
            stream_id,
            expected_version,
            events,
        }
    }
}

/// An event to be written to the event store.
#[derive(Debug, Clone)]
pub struct EventToWrite<E> {
    /// Unique identifier for this event (must be `UUIDv7`)
    pub event_id: EventId,
    /// The event payload
    pub payload: E,
    /// Optional metadata for this event
    pub metadata: Option<EventMetadata>,
}

impl<E> EventToWrite<E> {
    /// Creates a new event to write.
    pub const fn new(event_id: EventId, payload: E) -> Self {
        Self {
            event_id,
            payload,
            metadata: None,
        }
    }

    /// Creates a new event with metadata.
    pub const fn with_metadata(event_id: EventId, payload: E, metadata: EventMetadata) -> Self {
        Self {
            event_id,
            payload,
            metadata: Some(metadata),
        }
    }
}

/// The core event store trait that all implementations must satisfy.
///
/// This trait is designed to be backend-independent and support the
/// aggregate-per-command pattern with multi-stream atomic operations.
#[async_trait]
pub trait EventStore: Send + Sync {
    /// The event type this store handles.
    type Event: Send + Sync;

    /// Reads events from multiple streams.
    ///
    /// This method reads events from all specified streams and returns them
    /// in a single response, ordered by event ID (which provides timestamp ordering).
    /// This enables commands to read from multiple streams atomically.
    ///
    /// # Arguments
    /// * `stream_ids` - The streams to read from
    /// * `options` - Configuration for the read operation
    ///
    /// # Returns
    /// A `StreamData` containing all events from the requested streams,
    /// ordered by timestamp, along with current stream versions.
    ///
    /// # Errors
    /// Returns `EventStoreError::StreamNotFound` if any requested stream doesn't exist
    /// and the store requires streams to exist before reading.
    async fn read_streams(
        &self,
        stream_ids: &[StreamId],
        options: &ReadOptions,
    ) -> EventStoreResult<StreamData<Self::Event>>;

    /// Writes events to multiple streams atomically.
    ///
    /// This is the core operation that enables the aggregate-per-command pattern.
    /// All writes either succeed completely or fail completely, ensuring atomicity
    /// across multiple streams.
    ///
    /// # Arguments
    /// * `stream_events` - Events to write to each stream with expected versions
    ///
    /// # Returns
    /// A map of stream ID to the new version after writing.
    ///
    /// # Errors
    /// * `EventStoreError::VersionConflict` - If any expected version doesn't match
    /// * `EventStoreError::DuplicateEventId` - If any event ID already exists
    /// * `EventStoreError::ConnectionFailed` - If the store is unavailable
    async fn write_events_multi(
        &self,
        stream_events: Vec<StreamEvents<Self::Event>>,
    ) -> EventStoreResult<HashMap<StreamId, EventVersion>>;

    /// Checks if a stream exists.
    ///
    /// # Arguments
    /// * `stream_id` - The stream to check
    ///
    /// # Returns
    /// `true` if the stream exists, `false` otherwise.
    async fn stream_exists(&self, stream_id: &StreamId) -> EventStoreResult<bool>;

    /// Gets the current version of a stream.
    ///
    /// # Arguments
    /// * `stream_id` - The stream to check
    ///
    /// # Returns
    /// The current version of the stream, or `None` if the stream doesn't exist.
    async fn get_stream_version(
        &self,
        stream_id: &StreamId,
    ) -> EventStoreResult<Option<EventVersion>>;

    /// Creates a subscription to events.
    ///
    /// This method creates a subscription that can be used to receive events
    /// as they are written to the store. The subscription behavior is controlled
    /// by the provided options.
    ///
    /// # Arguments
    /// * `options` - Configuration for the subscription
    ///
    /// # Returns
    /// A subscription instance that can be used to receive events.
    async fn subscribe(
        &self,
        options: crate::subscription::SubscriptionOptions,
    ) -> EventStoreResult<Box<dyn crate::subscription::Subscription<Event = Self::Event>>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_data_creation_and_access() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event_id = EventId::new();
        let version = EventVersion::initial();
        let timestamp = Timestamp::now();

        let event = StoredEvent::new(
            event_id,
            stream_id.clone(),
            version,
            timestamp,
            "test_payload",
            None,
        );

        let mut stream_versions = HashMap::new();
        stream_versions.insert(stream_id.clone(), version);

        let stream_data = StreamData::new(vec![event.clone()], stream_versions);

        assert!(!stream_data.is_empty());
        assert_eq!(stream_data.len(), 1);
        assert_eq!(stream_data.stream_version(&stream_id), Some(version));

        let events: Vec<_> = stream_data.events().collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], &event);

        let stream_events: Vec<_> = stream_data.events_for_stream(&stream_id).collect();
        assert_eq!(stream_events.len(), 1);
        assert_eq!(stream_events[0], &event);
    }

    #[test]
    fn stored_event_creation() {
        let event_id = EventId::new();
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let version = EventVersion::initial();
        let timestamp = Timestamp::now();
        let payload = "test_payload";

        let event = StoredEvent::new(
            event_id,
            stream_id.clone(),
            version,
            timestamp,
            payload,
            None,
        );

        assert_eq!(event.event_id, event_id);
        assert_eq!(event.stream_id, stream_id);
        assert_eq!(event.event_version, version);
        assert_eq!(event.timestamp, timestamp);
        assert_eq!(event.payload, payload);
        assert_eq!(event.metadata, None);
    }

    #[test]
    fn event_metadata_builder() {
        let causation_id = EventId::new();
        let correlation_id = "corr-123".to_string();
        let user_id = "user-456".to_string();

        let metadata = EventMetadata::new()
            .with_causation_id(causation_id)
            .with_correlation_id(correlation_id.clone())
            .with_user_id(user_id.clone())
            .with_custom("key1".to_string(), "value1".to_string())
            .with_custom("key2".to_string(), "value2".to_string());

        assert_eq!(metadata.causation_id, Some(causation_id));
        assert_eq!(metadata.correlation_id, Some(correlation_id));
        assert_eq!(metadata.user_id, Some(user_id));
        assert_eq!(metadata.custom.get("key1"), Some(&"value1".to_string()));
        assert_eq!(metadata.custom.get("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn read_options_builder() {
        let options = ReadOptions::new()
            .with_max_events(100)
            .from_version(EventVersion::try_new(5).unwrap())
            .to_version(EventVersion::try_new(10).unwrap());

        assert_eq!(options.max_events, Some(100));
        assert_eq!(
            options.from_version,
            Some(EventVersion::try_new(5).unwrap())
        );
        assert_eq!(options.to_version, Some(EventVersion::try_new(10).unwrap()));
    }

    #[test]
    fn expected_version_variants() {
        let new_version = ExpectedVersion::New;
        let exact_version = ExpectedVersion::Exact(EventVersion::try_new(5).unwrap());
        let any_version = ExpectedVersion::Any;

        assert_eq!(new_version, ExpectedVersion::New);
        assert_eq!(
            exact_version,
            ExpectedVersion::Exact(EventVersion::try_new(5).unwrap())
        );
        assert_eq!(any_version, ExpectedVersion::Any);
    }

    #[test]
    fn stream_events_creation() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let expected_version = ExpectedVersion::Exact(EventVersion::try_new(3).unwrap());

        let event_to_write = EventToWrite::new(EventId::new(), "payload");
        let events = vec![event_to_write];

        let stream_events = StreamEvents::new(stream_id.clone(), expected_version, events);

        assert_eq!(stream_events.stream_id, stream_id);
        assert_eq!(stream_events.expected_version, expected_version);
        assert_eq!(stream_events.events.len(), 1);
    }

    #[test]
    fn event_to_write_creation() {
        let event_id = EventId::new();
        let payload = "test_payload";

        // Without metadata
        let event1 = EventToWrite::new(event_id, payload);
        assert_eq!(event1.event_id, event_id);
        assert_eq!(event1.payload, payload);
        assert_eq!(event1.metadata, None);

        // With metadata
        let metadata = EventMetadata::new().with_user_id("user-123".to_string());
        let event2 = EventToWrite::with_metadata(event_id, payload, metadata.clone());
        assert_eq!(event2.event_id, event_id);
        assert_eq!(event2.payload, payload);
        assert_eq!(event2.metadata, Some(metadata));
    }

    #[test]
    fn event_metadata_serialization() {
        let metadata = EventMetadata::new()
            .with_causation_id(EventId::new())
            .with_correlation_id("test-correlation".to_string())
            .with_user_id("test-user".to_string())
            .with_custom("key".to_string(), "value".to_string());

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: EventMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(metadata, deserialized);
    }

    #[test]
    fn stored_event_serialization() {
        let event = StoredEvent::new(
            EventId::new(),
            StreamId::try_new("test").unwrap(),
            EventVersion::initial(),
            Timestamp::now(),
            "test_payload",
            Some(EventMetadata::new()),
        );

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: StoredEvent<&str> = serde_json::from_str(&json).unwrap();

        assert_eq!(event, deserialized);
    }
}
