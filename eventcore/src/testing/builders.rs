//! Builder patterns for creating test data.
//!
//! This module provides fluent builders for creating events, commands, and other
//! domain objects in tests. Builders handle the complexity of creating valid
//! objects while providing a convenient API.

use crate::event::{Event, StoredEvent};
use crate::event_store::{EventToWrite, StoredEvent as StoreStoredEvent};
use crate::metadata::{EventMetadata, EventMetadataBuilder};
use crate::types::{EventId, EventVersion, StreamId, Timestamp};

/// Builder for creating `Event<E>` instances for testing.
///
/// # Example
/// ```rust,ignore
/// use eventcore::testing::builders::EventBuilder;
///
/// let event = EventBuilder::new()
///     .stream_id("account-123")
///     .payload(AccountEvent::Deposited { amount: 100 })
///     .with_user("user-456")
///     .build();
/// ```
pub struct EventBuilder<E> {
    stream_id: Option<StreamId>,
    payload: Option<E>,
    metadata: EventMetadataBuilder,
    event_id: Option<EventId>,
    timestamp: Option<Timestamp>,
}

impl<E> EventBuilder<E>
where
    E: PartialEq + Eq,
{
    /// Creates a new event builder.
    pub fn new() -> Self {
        Self {
            stream_id: None,
            payload: None,
            metadata: EventMetadataBuilder::new(),
            event_id: None,
            timestamp: None,
        }
    }

    /// Sets the stream ID.
    ///
    /// The value will be validated according to `StreamId` rules.
    #[must_use]
    pub fn stream_id(mut self, stream_id: impl Into<String>) -> Self {
        self.stream_id = StreamId::try_new(stream_id.into()).ok();
        self
    }

    /// Sets the stream ID from a valid `StreamId`.
    #[must_use]
    pub fn with_stream_id(mut self, stream_id: StreamId) -> Self {
        self.stream_id = Some(stream_id);
        self
    }

    /// Sets the event payload.
    #[must_use]
    pub fn payload(mut self, payload: E) -> Self {
        self.payload = Some(payload);
        self
    }

    /// Sets a specific event ID.
    ///
    /// If not set, a new `EventId` will be generated.
    #[must_use]
    pub const fn with_event_id(mut self, event_id: EventId) -> Self {
        self.event_id = Some(event_id);
        self
    }

    /// Sets a specific timestamp.
    ///
    /// If not set, the current timestamp will be used.
    #[must_use]
    pub const fn with_timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Sets the user ID in metadata.
    #[must_use]
    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        if let Ok(user_id) = crate::metadata::UserId::try_new(user_id.into()) {
            self.metadata = self.metadata.user_id(user_id);
        }
        self
    }

    /// Sets the correlation ID in metadata.
    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: crate::metadata::CorrelationId) -> Self {
        self.metadata = self.metadata.correlation_id(correlation_id);
        self
    }

    /// Sets the causation from an event ID.
    #[must_use]
    pub fn caused_by(mut self, event_id: EventId) -> Self {
        self.metadata = self.metadata.caused_by(event_id);
        self
    }

    /// Adds custom metadata.
    #[must_use]
    pub fn with_custom_metadata<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<serde_json::Value>,
    {
        self.metadata = self.metadata.custom(key, value);
        self
    }

    /// Builds the event.
    ///
    /// # Panics
    /// Panics if required fields (`stream_id`, payload) are not set.
    pub fn build(self) -> Event<E> {
        let stream_id = self.stream_id.expect("stream_id is required");
        let payload = self.payload.expect("payload is required");
        let metadata = self.metadata.build();

        let mut event = Event::new(stream_id, payload, metadata);

        if let Some(event_id) = self.event_id {
            event.id = event_id;
        }

        if let Some(timestamp) = self.timestamp {
            event.created_at = timestamp;
        }

        event
    }
}

impl<E> Default for EventBuilder<E>
where
    E: PartialEq + Eq,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating `StoredEvent<E>` instances for testing.
///
/// # Example
/// ```rust,ignore
/// use eventcore::testing::builders::StoredEventBuilder;
///
/// let stored_event = StoredEventBuilder::new()
///     .stream_id("account-123")
///     .payload(AccountEvent::Deposited { amount: 100 })
///     .version(5)
///     .build();
/// ```
pub struct StoredEventBuilder<E> {
    event_builder: EventBuilder<E>,
    version: Option<EventVersion>,
    stored_at: Option<Timestamp>,
}

impl<E> StoredEventBuilder<E>
where
    E: PartialEq + Eq,
{
    /// Creates a new stored event builder.
    pub fn new() -> Self {
        Self {
            event_builder: EventBuilder::new(),
            version: None,
            stored_at: None,
        }
    }

    /// Sets the stream ID.
    #[must_use]
    pub fn stream_id(mut self, stream_id: impl Into<String>) -> Self {
        self.event_builder = self.event_builder.stream_id(stream_id);
        self
    }

    /// Sets the stream ID from a valid `StreamId`.
    #[must_use]
    pub fn with_stream_id(mut self, stream_id: StreamId) -> Self {
        self.event_builder = self.event_builder.with_stream_id(stream_id);
        self
    }

    /// Sets the event payload.
    #[must_use]
    pub fn payload(mut self, payload: E) -> Self {
        self.event_builder = self.event_builder.payload(payload);
        self
    }

    /// Sets the event version.
    #[must_use]
    pub fn version(mut self, version: u64) -> Self {
        self.version = EventVersion::try_new(version).ok();
        self
    }

    /// Sets the event version from a valid `EventVersion`.
    #[must_use]
    pub const fn with_version(mut self, version: EventVersion) -> Self {
        self.version = Some(version);
        self
    }

    /// Sets when the event was stored.
    #[must_use]
    pub const fn stored_at(mut self, timestamp: Timestamp) -> Self {
        self.stored_at = Some(timestamp);
        self
    }

    /// Sets a specific event ID.
    #[must_use]
    pub fn with_event_id(mut self, event_id: EventId) -> Self {
        self.event_builder = self.event_builder.with_event_id(event_id);
        self
    }

    /// Sets the user ID in metadata.
    #[must_use]
    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.event_builder = self.event_builder.with_user(user_id);
        self
    }

    /// Sets the correlation ID in metadata.
    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: crate::metadata::CorrelationId) -> Self {
        self.event_builder = self.event_builder.with_correlation_id(correlation_id);
        self
    }

    /// Builds the stored event.
    ///
    /// # Panics
    /// Panics if required fields are not set.
    pub fn build(self) -> StoredEvent<E> {
        let event = self.event_builder.build();
        let version = self.version.unwrap_or_else(EventVersion::initial);

        if let Some(stored_at) = self.stored_at {
            StoredEvent::with_timestamp(event, version, stored_at)
        } else {
            StoredEvent::new(event, version)
        }
    }
}

impl<E> Default for StoredEventBuilder<E>
where
    E: PartialEq + Eq,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating store-specific `StoredEvent<E>` instances.
///
/// This builder creates events as they appear in the `event_store` module.
pub struct StoreEventBuilder<E> {
    event_id: Option<EventId>,
    stream_id: Option<StreamId>,
    event_version: Option<EventVersion>,
    timestamp: Option<Timestamp>,
    payload: Option<E>,
    metadata: Option<EventMetadata>,
}

impl<E> StoreEventBuilder<E>
where
    E: PartialEq + Eq,
{
    /// Creates a new store event builder.
    pub const fn new() -> Self {
        Self {
            event_id: None,
            stream_id: None,
            event_version: None,
            timestamp: None,
            payload: None,
            metadata: None,
        }
    }

    /// Sets the event ID.
    #[must_use]
    pub const fn with_event_id(mut self, event_id: EventId) -> Self {
        self.event_id = Some(event_id);
        self
    }

    /// Sets the stream ID.
    #[must_use]
    pub fn stream_id(mut self, stream_id: impl Into<String>) -> Self {
        self.stream_id = StreamId::try_new(stream_id.into()).ok();
        self
    }

    /// Sets the stream ID from a valid `StreamId`.
    #[must_use]
    pub fn with_stream_id(mut self, stream_id: StreamId) -> Self {
        self.stream_id = Some(stream_id);
        self
    }

    /// Sets the event version.
    #[must_use]
    pub fn version(mut self, version: u64) -> Self {
        self.event_version = EventVersion::try_new(version).ok();
        self
    }

    /// Sets the event version from a valid `EventVersion`.
    #[must_use]
    pub const fn with_version(mut self, version: EventVersion) -> Self {
        self.event_version = Some(version);
        self
    }

    /// Sets the timestamp.
    #[must_use]
    pub const fn with_timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Sets the payload.
    #[must_use]
    pub fn payload(mut self, payload: E) -> Self {
        self.payload = Some(payload);
        self
    }

    /// Sets the metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: EventMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Builds the store event.
    ///
    /// # Panics
    /// Panics if required fields are not set.
    pub fn build(self) -> StoreStoredEvent<E> {
        StoreStoredEvent::new(
            self.event_id.unwrap_or_default(),
            self.stream_id.expect("stream_id is required"),
            self.event_version.unwrap_or_else(EventVersion::initial),
            self.timestamp.unwrap_or_else(Timestamp::now),
            self.payload.expect("payload is required"),
            self.metadata,
        )
    }
}

impl<E> Default for StoreEventBuilder<E>
where
    E: PartialEq + Eq,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating `EventToWrite<E>` instances.
pub struct EventToWriteBuilder<E> {
    event_id: Option<EventId>,
    payload: Option<E>,
    metadata: Option<EventMetadata>,
}

impl<E> EventToWriteBuilder<E> {
    /// Creates a new builder.
    pub const fn new() -> Self {
        Self {
            event_id: None,
            payload: None,
            metadata: None,
        }
    }

    /// Sets the event ID.
    #[must_use]
    pub const fn with_event_id(mut self, event_id: EventId) -> Self {
        self.event_id = Some(event_id);
        self
    }

    /// Sets the payload.
    #[must_use]
    pub fn payload(mut self, payload: E) -> Self {
        self.payload = Some(payload);
        self
    }

    /// Sets the metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: EventMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Builds the event to write.
    ///
    /// # Panics
    /// Panics if payload is not set.
    pub fn build(self) -> EventToWrite<E> {
        let event_id = self.event_id.unwrap_or_default();
        let payload = self.payload.expect("payload is required");

        if let Some(metadata) = self.metadata {
            EventToWrite::with_metadata(event_id, payload, metadata)
        } else {
            EventToWrite::new(event_id, payload)
        }
    }
}

impl<E> Default for EventToWriteBuilder<E> {
    fn default() -> Self {
        Self::new()
    }
}

/// Creates a sequence of events with incrementing versions.
///
/// # Example
/// ```rust,ignore
/// use eventcore::testing::builders::create_event_sequence;
///
/// let events = create_event_sequence("stream-1", vec!["event1", "event2", "event3"]);
/// assert_eq!(events.len(), 3);
/// assert_eq!(events[0].version, EventVersion::try_new(1).unwrap());
/// ```
pub fn create_event_sequence<E>(
    stream_id: impl Into<String>,
    payloads: Vec<E>,
) -> Vec<StoredEvent<E>>
where
    E: PartialEq + Eq,
{
    let stream_id = StreamId::try_new(stream_id.into()).expect("Invalid stream ID");
    let mut events = Vec::new();

    for (i, payload) in payloads.into_iter().enumerate() {
        let event = StoredEventBuilder::new()
            .with_stream_id(stream_id.clone())
            .payload(payload)
            .version(i as u64 + 1)
            .build();
        events.push(event);
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::CorrelationId;

    #[test]
    fn test_event_builder_basic() {
        let event = EventBuilder::new()
            .stream_id("test-stream")
            .payload("test payload")
            .build();

        assert_eq!(event.stream_id.as_ref(), "test-stream");
        assert_eq!(event.payload, "test payload");
    }

    #[test]
    fn test_event_builder_with_metadata() {
        let correlation_id = CorrelationId::new();
        let causing_event = EventId::new();

        let event = EventBuilder::new()
            .stream_id("test-stream")
            .payload("test payload")
            .with_user("user-123")
            .with_correlation_id(correlation_id)
            .caused_by(causing_event)
            .with_custom_metadata("key", "value")
            .build();

        assert_eq!(event.metadata.correlation_id, correlation_id);
        assert_eq!(
            event
                .metadata
                .user_id
                .as_ref()
                .map(std::convert::AsRef::as_ref),
            Some("user-123")
        );
        assert!(event.metadata.causation_id.is_some());
    }

    #[test]
    fn test_stored_event_builder() {
        let stored_event = StoredEventBuilder::new()
            .stream_id("test-stream")
            .payload("test payload")
            .version(5)
            .build();

        assert_eq!(stored_event.stream_id().as_ref(), "test-stream");
        assert_eq!(stored_event.payload(), &"test payload");
        assert_eq!(stored_event.version, EventVersion::try_new(5).unwrap());
    }

    #[test]
    fn test_store_event_builder() {
        let store_event = StoreEventBuilder::new()
            .stream_id("test-stream")
            .payload("test payload")
            .version(3)
            .build();

        assert_eq!(store_event.stream_id.as_ref(), "test-stream");
        assert_eq!(store_event.payload, "test payload");
        assert_eq!(store_event.event_version, EventVersion::try_new(3).unwrap());
    }

    #[test]
    fn test_event_to_write_builder() {
        let event_to_write = EventToWriteBuilder::new().payload("test payload").build();

        assert_eq!(event_to_write.payload, "test payload");
        assert!(event_to_write.metadata.is_none());
    }

    #[test]
    fn test_create_event_sequence() {
        let events = create_event_sequence("test-stream", vec!["event1", "event2", "event3"]);

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].version, EventVersion::try_new(1).unwrap());
        assert_eq!(events[1].version, EventVersion::try_new(2).unwrap());
        assert_eq!(events[2].version, EventVersion::try_new(3).unwrap());

        assert_eq!(events[0].payload(), &"event1");
        assert_eq!(events[1].payload(), &"event2");
        assert_eq!(events[2].payload(), &"event3");
    }

    #[test]
    #[should_panic(expected = "stream_id is required")]
    fn test_event_builder_panics_without_stream_id() {
        EventBuilder::<String>::new()
            .payload("test".to_string())
            .build();
    }

    #[test]
    #[should_panic(expected = "payload is required")]
    fn test_event_builder_panics_without_payload() {
        let _: Event<String> = EventBuilder::new().stream_id("test").build();
    }
}
