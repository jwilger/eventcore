//! Event store abstraction for EventCore.
//!
//! This module provides the core `EventStore` trait and related types that define
//! how events are persisted and retrieved. The design supports multi-stream event
//! sourcing with atomic operations across multiple streams.
//!
//! # Architecture
//!
//! The event store follows the Adapter pattern (from Hexagonal Architecture):
//! - The `EventStore` trait is the port (interface)
//! - Implementations like PostgreSQL or in-memory stores are adapters
//! - Commands interact only with the trait, not specific implementations
//!
//! # Key Features
//!
//! - **Multi-stream reads**: Read from multiple streams in a single operation
//! - **Atomic writes**: Write to multiple streams atomically
//! - **Optimistic concurrency**: Version-based conflict detection
//! - **Global ordering**: Events use UUIDv7 for timestamp-based ordering
//! - **Backend independence**: Easy to swap storage implementations
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use eventcore::event_store::{EventStore, StreamEvents, EventToWrite, ExpectedVersion};
//! use eventcore::types::{StreamId, EventId};
//!
//! // Reading from multiple streams
//! let streams = vec![
//!     StreamId::try_new("account-123").unwrap(),
//!     StreamId::try_new("account-456").unwrap(),
//! ];
//! let data = store.read_streams(&streams, &ReadOptions::default()).await?;
//!
//! // Writing to multiple streams atomically
//! let writes = vec![
//!     StreamEvents::new(
//!         StreamId::try_new("account-123").unwrap(),
//!         ExpectedVersion::Exact(data.stream_version(&streams[0]).unwrap()),
//!         vec![EventToWrite::new(EventId::new(), AccountEvent::Debited { amount: 100 })],
//!     ),
//!     StreamEvents::new(
//!         StreamId::try_new("account-456").unwrap(),
//!         ExpectedVersion::Exact(data.stream_version(&streams[1]).unwrap()),
//!         vec![EventToWrite::new(EventId::new(), AccountEvent::Credited { amount: 100 })],
//!     ),
//! ];
//! let new_versions = store.write_events_multi(writes).await?;
//! ```

use crate::errors::EventStoreResult;
use crate::metadata::EventMetadata;
use crate::types::{EventId, EventVersion, StreamId, Timestamp};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Container for events read from one or more streams.
///
/// `StreamData` is returned by `EventStore::read_streams` and contains all events
/// from the requested streams in chronological order, along with version information
/// needed for optimistic concurrency control.
///
/// # Ordering Guarantees
///
/// Events are ordered by their `EventId` (UUIDv7), which provides:
/// - Global chronological ordering across all streams
/// - Deterministic ordering for event replay
/// - Efficient range queries by time
///
/// # Example
///
/// ```rust,ignore
/// let data = store.read_streams(&[stream1, stream2], &ReadOptions::default()).await?;
///
/// // Process all events in chronological order
/// for event in data.events() {
///     println!("Event {} from stream {}", event.event_id, event.stream_id);
/// }
///
/// // Check version for concurrency control
/// if let Some(version) = data.stream_version(&stream1) {
///     println!("Stream {} is at version {}", stream1, version);
/// }
/// ```
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

/// An event as stored in the event store with full metadata.
///
/// `StoredEvent` represents a persisted event with all the metadata needed for
/// event sourcing, including versioning, ordering, and causation tracking.
///
/// # Fields
///
/// - `event_id`: Globally unique identifier (UUIDv7 for time-based ordering)
/// - `stream_id`: The stream this event belongs to
/// - `event_version`: Position within the stream (0-based, monotonic)
/// - `timestamp`: When the event was stored
/// - `payload`: The actual event data (your domain event)
/// - `metadata`: Optional causation, correlation, and custom metadata
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Serialize, Deserialize)]
/// enum AccountEvent {
///     Opened { owner: String },
///     Deposited { amount: u64 },
/// }
///
/// let event: StoredEvent<AccountEvent> = StoredEvent::new(
///     EventId::new(),
///     StreamId::try_new("account-123").unwrap(),
///     EventVersion::initial(),
///     Timestamp::now(),
///     AccountEvent::Opened { owner: "Alice".to_string() },
///     Some(metadata),
/// );
/// ```
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

impl<E> StoredEvent<E>
where
    E: PartialEq + Eq,
{
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

    /// Converts this StoredEvent to an Event for compatibility with projections.
    pub fn to_event(&self) -> crate::event::Event<E>
    where
        E: Clone,
    {
        crate::event::Event {
            id: self.event_id,
            stream_id: self.stream_id.clone(),
            payload: self.payload.clone(),
            metadata: self.metadata.clone().unwrap_or_default(),
            created_at: self.timestamp,
        }
    }
}

/// Configuration options for reading events from streams.
///
/// `ReadOptions` allows you to control how events are read from the store,
/// including limiting the number of events and specifying version ranges.
///
/// # Example
///
/// ```rust,ignore
/// use eventcore::event_store::ReadOptions;
/// use eventcore::types::EventVersion;
///
/// // Read all events (default)
/// let all_events = ReadOptions::default();
///
/// // Read only the last 100 events
/// let recent = ReadOptions::new().with_max_events(100);
///
/// // Read events from version 10 to 20
/// let range = ReadOptions::new()
///     .from_version(EventVersion::try_new(10).unwrap())
///     .to_version(EventVersion::try_new(20).unwrap());
/// ```
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
///
/// `ExpectedVersion` is used when writing events to ensure that streams haven't
/// been modified concurrently. This prevents lost updates in distributed systems.
///
/// # Variants
///
/// - `New`: The stream must not exist yet (for creating new streams)
/// - `Exact(version)`: The stream must be at exactly this version
/// - `Any`: No version check (use with caution - can cause lost updates)
///
/// # Example
///
/// ```rust,ignore
/// use eventcore::event_store::{ExpectedVersion, EventVersion};
///
/// // Creating a new stream
/// let new_stream = ExpectedVersion::New;
///
/// // Updating an existing stream with concurrency control
/// let current_version = EventVersion::try_new(42).unwrap();
/// let exact = ExpectedVersion::Exact(current_version);
///
/// // Dangerous: bypassing concurrency control
/// let any = ExpectedVersion::Any;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedVersion {
    /// The stream must not exist
    New,
    /// The stream must exist and have exactly this version
    Exact(EventVersion),
    /// Any version is acceptable (no concurrency control)
    Any,
}

/// A batch of events to write to a specific stream.
///
/// `StreamEvents` groups events that should be written to the same stream,
/// along with the expected version for concurrency control. This is used
/// with `EventStore::write_events_multi` for atomic multi-stream writes.
///
/// # Example
///
/// ```rust,ignore
/// use eventcore::event_store::{StreamEvents, EventToWrite, ExpectedVersion};
/// use eventcore::types::{StreamId, EventId};
///
/// let events = vec![
///     EventToWrite::new(EventId::new(), AccountEvent::Deposited { amount: 100 }),
///     EventToWrite::new(EventId::new(), AccountEvent::WithdrawalRequested { amount: 50 }),
/// ];
///
/// let stream_write = StreamEvents::new(
///     StreamId::try_new("account-123").unwrap(),
///     ExpectedVersion::Exact(current_version),
///     events,
/// );
/// ```
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
///
/// `EventToWrite` represents an event before it's persisted. It contains
/// the event payload and optional metadata, but not the version or timestamp
/// (which are assigned by the event store).
///
/// # Example
///
/// ```rust,ignore
/// use eventcore::event_store::EventToWrite;
/// use eventcore::types::EventId;
/// use eventcore::metadata::{EventMetadata, UserId};
///
/// // Simple event without metadata
/// let event = EventToWrite::new(
///     EventId::new(),
///     OrderEvent::Placed { items: vec!["ABC123"] }
/// );
///
/// // Event with metadata for tracking causation
/// let metadata = EventMetadata::new()
///     .with_user_id(Some(UserId::try_new("user-456").unwrap()))
///     .with_custom("source", serde_json::json!("web"));
///
/// let event_with_meta = EventToWrite::with_metadata(
///     EventId::new(),
///     OrderEvent::Shipped { tracking: "XYZ789".to_string() },
///     metadata,
/// );
/// ```
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

/// The core trait for event store implementations.
///
/// `EventStore` defines the contract that all event store backends must implement.
/// It's designed to support multi-stream event sourcing with atomic operations
/// across multiple streams, while remaining backend-agnostic.
///
/// # Implementation Requirements
///
/// Implementations must ensure:
/// - **Atomicity**: All events in a write batch succeed or fail together
/// - **Consistency**: Version checks prevent concurrent modifications
/// - **Durability**: Successfully written events are permanently stored
/// - **Ordering**: Events maintain chronological order via UUIDv7
///
/// # Backend Examples
///
/// - **PostgreSQL**: Full ACID compliance with transactions
/// - **EventStoreDB**: Purpose-built event store with projections
/// - **In-Memory**: For testing, with HashMap-based storage
///
/// # Example Implementation
///
/// ```rust,ignore
/// use async_trait::async_trait;
/// use eventcore::event_store::{EventStore, EventStoreResult, StreamData};
///
/// struct MyEventStore {
///     // Backend-specific fields
/// }
///
/// #[async_trait]
/// impl EventStore for MyEventStore {
///     type Event = MyDomainEvent;
///
///     async fn read_streams(
///         &self,
///         stream_ids: &[StreamId],
///         options: &ReadOptions,
///     ) -> EventStoreResult<StreamData<Self::Event>> {
///         // Implementation details
///         todo!()
///     }
///
///     // ... other methods
/// }
/// ```
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
    /// This is the core operation that enables multi-stream event sourcing.
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
        use crate::metadata::{CausationId, CorrelationId, UserId};

        let event_id = EventId::new();
        let causation_id = CausationId::from(event_id);
        let correlation_id = CorrelationId::new();
        let user_id = UserId::try_new("user-456").unwrap();

        let metadata = EventMetadata::new()
            .with_causation_id(causation_id)
            .with_correlation_id(correlation_id)
            .with_user_id(Some(user_id.clone()))
            .with_custom("key1", serde_json::json!("value1"))
            .with_custom("key2", serde_json::json!("value2"));

        assert_eq!(metadata.causation_id, Some(causation_id));
        assert_eq!(metadata.correlation_id, correlation_id);
        assert_eq!(metadata.user_id, Some(user_id));
        assert_eq!(
            metadata.custom.get("key1"),
            Some(&serde_json::json!("value1"))
        );
        assert_eq!(
            metadata.custom.get("key2"),
            Some(&serde_json::json!("value2"))
        );
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
        use crate::metadata::UserId;

        let event_id = EventId::new();
        let payload = "test_payload";

        // Without metadata
        let event1 = EventToWrite::new(event_id, payload);
        assert_eq!(event1.event_id, event_id);
        assert_eq!(event1.payload, payload);
        assert_eq!(event1.metadata, None);

        // With metadata
        let metadata =
            EventMetadata::new().with_user_id(Some(UserId::try_new("user-123").unwrap()));
        let event2 = EventToWrite::with_metadata(event_id, payload, metadata.clone());
        assert_eq!(event2.event_id, event_id);
        assert_eq!(event2.payload, payload);
        assert_eq!(event2.metadata, Some(metadata));
    }

    #[test]
    fn event_metadata_serialization() {
        use crate::metadata::{CausationId, CorrelationId, UserId};

        let event_id = EventId::new();
        let metadata = EventMetadata::new()
            .with_causation_id(CausationId::from(event_id))
            .with_correlation_id(CorrelationId::new())
            .with_user_id(Some(UserId::try_new("test-user").unwrap()))
            .with_custom("key", serde_json::json!("value"));

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
