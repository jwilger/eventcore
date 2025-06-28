//! Event types for the event sourcing system.
//!
//! This module defines the core event structures used throughout the system.
//! Events are immutable records of state changes that have occurred in the system.

use crate::metadata::EventMetadata;
use crate::types::{EventId, EventVersion, StreamId, Timestamp};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// A domain event with its payload and metadata.
///
/// This represents an event that has occurred in the system but has not yet been persisted.
/// The generic type `E` represents the event payload type specific to each domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event<E>
where
    E: PartialEq + Eq,
{
    /// The unique identifier for this event
    pub id: EventId,
    /// The stream this event belongs to
    pub stream_id: StreamId,
    /// The event payload containing domain-specific data
    pub payload: E,
    /// Metadata about the event (causation, correlation, actor, etc.)
    pub metadata: EventMetadata,
    /// When the event was created
    pub created_at: Timestamp,
}

impl<E> Event<E>
where
    E: PartialEq + Eq,
{
    /// Creates a new event with the given payload and metadata.
    pub fn new(stream_id: StreamId, payload: E, metadata: EventMetadata) -> Self {
        Self {
            id: EventId::new(),
            stream_id,
            payload,
            metadata,
            created_at: Timestamp::now(),
        }
    }

    /// Creates a new event with the given payload and default metadata.
    pub fn with_payload(stream_id: StreamId, payload: E) -> Self {
        Self::new(stream_id, payload, EventMetadata::default())
    }
}

/// A persisted event with version information.
///
/// This represents an event that has been stored in the event store
/// and includes additional information like the version number within its stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEvent<E>
where
    E: PartialEq + Eq,
{
    /// The underlying event
    pub event: Event<E>,
    /// The version of this event within its stream
    pub version: EventVersion,
    /// When the event was stored (may differ from `created_at` due to processing delays)
    pub stored_at: Timestamp,
}

impl<E> StoredEvent<E>
where
    E: PartialEq + Eq,
{
    /// Creates a new stored event from an event and version information.
    pub fn new(event: Event<E>, version: EventVersion) -> Self {
        Self {
            event,
            version,
            stored_at: Timestamp::now(),
        }
    }

    /// Creates a stored event with a specific stored timestamp (for testing or replay).
    pub const fn with_timestamp(
        event: Event<E>,
        version: EventVersion,
        stored_at: Timestamp,
    ) -> Self {
        Self {
            event,
            version,
            stored_at,
        }
    }

    /// Returns the event ID.
    pub const fn id(&self) -> &EventId {
        &self.event.id
    }

    /// Returns the stream ID.
    pub const fn stream_id(&self) -> &StreamId {
        &self.event.stream_id
    }

    /// Returns the event payload.
    pub const fn payload(&self) -> &E {
        &self.event.payload
    }

    /// Returns the event metadata.
    pub const fn metadata(&self) -> &EventMetadata {
        &self.event.metadata
    }

    /// Returns when the event was created.
    pub const fn created_at(&self) -> &Timestamp {
        &self.event.created_at
    }
}

/// Ordering for stored events based on their `EventId` (`UUIDv7`).
///
/// Since `EventId` uses `UUIDv7`, which includes a timestamp component,
/// events can be globally ordered chronologically.
impl<E> Ord for StoredEvent<E>
where
    E: PartialEq + Eq,
{
    fn cmp(&self, other: &Self) -> Ordering {
        self.event.id.cmp(&other.event.id)
    }
}

impl<E> PartialOrd for StoredEvent<E>
where
    E: PartialEq + Eq,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_event_creation() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let payload = "test payload";
        let metadata = EventMetadata::default();

        let event = Event::new(stream_id.clone(), payload, metadata.clone());

        assert_eq!(event.stream_id, stream_id);
        assert_eq!(event.payload, payload);
        assert_eq!(event.metadata, metadata);
    }

    #[test]
    fn test_event_with_payload() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let payload = "test payload";

        let event = Event::with_payload(stream_id.clone(), payload);

        assert_eq!(event.stream_id, stream_id);
        assert_eq!(event.payload, payload);
        // EventMetadata::default() creates a new timestamp and correlation ID each time
        // So we just verify the structure is as expected
        assert!(event.metadata.causation_id.is_none());
        assert!(event.metadata.user_id.is_none());
        assert!(event.metadata.custom.is_empty());
    }

    #[test]
    fn test_stored_event_creation() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event = Event::with_payload(stream_id, "test");
        let version = EventVersion::try_new(1).unwrap();

        let stored = StoredEvent::new(event.clone(), version);

        assert_eq!(stored.event, event);
        assert_eq!(stored.version, version);
    }

    #[test]
    fn test_stored_event_accessors() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event = Event::with_payload(stream_id.clone(), "test");
        let version = EventVersion::try_new(1).unwrap();

        let stored = StoredEvent::new(event.clone(), version);

        assert_eq!(stored.id(), &event.id);
        assert_eq!(stored.stream_id(), &stream_id);
        assert_eq!(stored.payload(), &"test");
        assert_eq!(stored.metadata(), &event.metadata);
        assert_eq!(stored.created_at(), &event.created_at);
    }

    #[test]
    fn test_event_ordering_by_id() {
        // Create events with small delays to ensure different timestamps
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event1 = Event::with_payload(stream_id.clone(), "first");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let event2 = Event::with_payload(stream_id.clone(), "second");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let event3 = Event::with_payload(stream_id, "third");

        let stored1 = StoredEvent::new(event1, EventVersion::try_new(1).unwrap());
        let stored2 = StoredEvent::new(event2, EventVersion::try_new(2).unwrap());
        let stored3 = StoredEvent::new(event3, EventVersion::try_new(3).unwrap());

        // Events should be ordered by their EventId (UUIDv7 timestamp)
        assert!(stored1 < stored2);
        assert!(stored2 < stored3);
        assert!(stored1 < stored3);
    }

    // Property tests
    proptest! {
        #[test]
        fn prop_event_ordering_is_transitive(
            payload1 in ".*",
            payload2 in ".*",
            payload3 in ".*"
        ) {
            let stream_id = StreamId::try_new("test-stream").unwrap();

            let event1 = Event::with_payload(stream_id.clone(), payload1);
            std::thread::sleep(std::time::Duration::from_millis(2));
            let event2 = Event::with_payload(stream_id.clone(), payload2);
            std::thread::sleep(std::time::Duration::from_millis(2));
            let event3 = Event::with_payload(stream_id, payload3);

            let stored1 = StoredEvent::new(event1, EventVersion::try_new(1).unwrap());
            let stored2 = StoredEvent::new(event2, EventVersion::try_new(2).unwrap());
            let stored3 = StoredEvent::new(event3, EventVersion::try_new(3).unwrap());

            // Transitivity: if a < b and b < c, then a < c
            if stored1 < stored2 && stored2 < stored3 {
                prop_assert!(stored1 < stored3);
            }
        }

        #[test]
        fn prop_event_ordering_is_antisymmetric(
            payload1 in ".*",
            payload2 in ".*"
        ) {
            let stream_id = StreamId::try_new("test-stream").unwrap();

            let event1 = Event::with_payload(stream_id.clone(), payload1);
            std::thread::sleep(std::time::Duration::from_millis(2));
            let event2 = Event::with_payload(stream_id, payload2);

            let stored1 = StoredEvent::new(event1, EventVersion::try_new(1).unwrap());
            let stored2 = StoredEvent::new(event2, EventVersion::try_new(2).unwrap());

            // Antisymmetry: if a <= b and b <= a, then a == b
            if stored1 <= stored2 && stored2 <= stored1 {
                prop_assert_eq!(&stored1.id(), &stored2.id());
            }
        }

        #[test]
        fn prop_event_ordering_is_reflexive(
            payload in ".*"
        ) {
            let stream_id = StreamId::try_new("test-stream").unwrap();
            let event = Event::with_payload(stream_id, payload);
            let stored = StoredEvent::new(event, EventVersion::try_new(1).unwrap());

            // Reflexivity: a == a for all a
            prop_assert_eq!(&stored, &stored);
            prop_assert!(stored <= stored);
            prop_assert!(stored >= stored);
        }

        #[test]
        fn prop_events_maintain_chronological_order(
            payloads in prop::collection::vec(".*", 3..10)
        ) {
            let stream_id = StreamId::try_new("test-stream").unwrap();

            // Create events with delays to ensure different timestamps
            let mut events = Vec::new();
            for (i, payload) in payloads.iter().enumerate() {
                let event = Event::with_payload(stream_id.clone(), payload.clone());
                let stored = StoredEvent::new(event, EventVersion::try_new(i as u64 + 1).unwrap());
                events.push(stored);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }

            // Verify that events maintain chronological order
            for i in 1..events.len() {
                prop_assert!(events[i-1] < events[i],
                    "Event {} should be less than event {}", i-1, i);
            }
        }
    }
}
