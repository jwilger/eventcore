//! In-memory event store implementation for testing.
//!
//! This module provides the `InMemoryEventStore` - a lightweight, zero-dependency
//! storage backend for EventCore integration tests and development.

use std::collections::HashMap;

use eventcore_types::{
    Event, EventFilter, EventPage, EventReader, EventStore, EventStoreError, EventStreamReader,
    EventStreamSlice, Operation, StreamId, StreamPosition, StreamVersion, StreamWriteEntry,
    StreamWrites,
};
use uuid::Uuid;

/// In-memory event store implementation for testing.
///
/// InMemoryEventStore provides a lightweight, zero-dependency storage backend
/// for EventCore integration tests and development. It implements the EventStore
/// trait using standard library collections (HashMap, BTreeMap) with optimistic
/// concurrency control via version checking.
///
/// # Example
///
/// ```ignore
/// use eventcore_memory::InMemoryEventStore;
///
/// let store = InMemoryEventStore::new();
/// // Use store with execute() function
/// ```
///
/// # Thread Safety
///
/// InMemoryEventStore uses interior mutability for concurrent access.
type StreamData = (Vec<Box<dyn std::any::Any + Send>>, StreamVersion);

/// Entry in the global event log with indexed stream_id for efficient filtering.
///
/// This structure mirrors the Postgres schema where stream_id is a separate
/// indexed column and event_id (UUID7) serves as the global position.
/// By storing stream_id and event_id separately, we can filter by stream
/// prefix and position without parsing JSON, matching the performance
/// characteristics of the database implementation.
#[derive(Debug, Clone)]
struct GlobalLogEntry {
    /// Event identifier (UUID7), used as global position
    event_id: Uuid,
    /// Stream identifier, extracted at write time for efficient filtering
    stream_id: String,
    /// Event data as JSON value
    event_data: serde_json::Value,
}

/// Internal storage combining per-stream data with global event ordering.
struct StoreData {
    streams: HashMap<StreamId, StreamData>,
    /// Global log with indexed stream_id for efficient EventReader queries
    global_log: Vec<GlobalLogEntry>,
}

pub struct InMemoryEventStore {
    data: std::sync::Mutex<StoreData>,
}

impl InMemoryEventStore {
    /// Create a new in-memory event store.
    ///
    /// Returns an empty event store ready for command execution.
    /// All streams start at version 0 (no events).
    pub fn new() -> Self {
        Self {
            data: std::sync::Mutex::new(StoreData {
                streams: HashMap::new(),
                global_log: Vec::new(),
            }),
        }
    }
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStore for InMemoryEventStore {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStreamReader<E>, EventStoreError> {
        let data = self
            .data
            .lock()
            .map_err(|_| EventStoreError::StoreFailure {
                operation: Operation::ReadStream,
            })?;
        let events = data
            .streams
            .get(&stream_id)
            .map(|(boxed_events, _version)| {
                boxed_events
                    .iter()
                    .filter_map(|boxed| boxed.downcast_ref::<E>())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        Ok(EventStreamReader::new(events))
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        let mut data = self
            .data
            .lock()
            .map_err(|_| EventStoreError::StoreFailure {
                operation: Operation::AppendEvents,
            })?;
        let expected_versions = writes.expected_versions().clone();

        // Check all version constraints before writing any events
        for (stream_id, expected_version) in &expected_versions {
            let current_version = data
                .streams
                .get(stream_id)
                .map(|(_events, version)| *version)
                .unwrap_or_else(|| StreamVersion::new(0));

            if current_version != *expected_version {
                return Err(EventStoreError::VersionConflict);
            }
        }

        // All versions match - proceed with writes
        for entry in writes.into_entries() {
            let StreamWriteEntry {
                stream_id,
                event,
                event_type: _,
                event_data,
            } = entry;

            // Generate UUID7 for this event (monotonic, timestamp-ordered)
            let event_id = Uuid::now_v7();

            // Store in global log for EventReader with indexed stream_id and event_id
            data.global_log.push(GlobalLogEntry {
                event_id,
                stream_id: stream_id.as_ref().to_string(),
                event_data,
            });

            let (events, version) = data
                .streams
                .entry(stream_id)
                .or_insert_with(|| (Vec::new(), StreamVersion::new(0)));
            events.push(event);
            *version = version.increment();
        }

        Ok(EventStreamSlice)
    }
}

impl EventReader for InMemoryEventStore {
    type Error = EventStoreError;

    async fn read_events<E: Event>(
        &self,
        filter: EventFilter,
        page: EventPage,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        let data = self
            .data
            .lock()
            .map_err(|_| EventStoreError::StoreFailure {
                operation: Operation::ReadStream,
            })?;

        let after_event_id = page.after_position().map(|p| p.into_inner());

        let events: Vec<(E, StreamPosition)> = data
            .global_log
            .iter()
            .filter(|entry| {
                // Filter by event_id (UUID7 comparison)
                match after_event_id {
                    None => true,
                    Some(after_id) => entry.event_id > after_id,
                }
            })
            .filter(|entry| {
                // Filter by indexed stream_id WITHOUT parsing JSON (matches Postgres behavior)
                match filter.stream_prefix() {
                    None => true,
                    Some(prefix) => entry.stream_id.starts_with(prefix.as_ref()),
                }
            })
            .take(page.limit().into_inner())
            .filter_map(|entry| {
                serde_json::from_value::<E>(entry.event_data.clone())
                    .ok()
                    .map(|e| (e, StreamPosition::new(entry.event_id)))
            })
            .collect();

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eventcore_types::{BatchSize, EventFilter, EventPage};
    use serde::{Deserialize, Serialize};

    /// Test-specific domain event type for unit testing storage operations.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestEvent {
        stream_id: StreamId,
        data: String,
    }

    impl Event for TestEvent {
        fn stream_id(&self) -> &StreamId {
            &self.stream_id
        }
    }

    /// Unit test: Verify InMemoryEventStore can append and retrieve a single event
    ///
    /// This test verifies the fundamental event storage capability:
    /// - Append an event to a stream
    /// - Read the stream back
    /// - Verify the event is retrievable with correct data
    ///
    /// This is a unit test drilling down from the failing integration test
    /// test_deposit_command_event_data_is_retrievable. We're testing the
    /// storage layer in isolation before testing the full command execution flow.
    #[tokio::test]
    async fn test_append_and_read_single_event() {
        // Given: An in-memory event store
        let store = InMemoryEventStore::new();

        // And: A stream ID
        let stream_id = StreamId::try_new("test-stream-123".to_string()).expect("valid stream id");

        // And: A domain event to store
        let event = TestEvent {
            stream_id: stream_id.clone(),
            data: "test event data".to_string(),
        };

        // And: A collection of writes containing the event (expected version 0 for empty stream)
        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(event.clone()))
            .expect("append should succeed");

        // When: We append the event to the store
        let _ = store
            .append_events(writes)
            .await
            .expect("append to succeed");

        let reader = store
            .read_stream::<TestEvent>(stream_id)
            .await
            .expect("read to succeed");

        let observed = (
            reader.is_empty(),
            reader.len(),
            reader.iter().next().is_none(),
        );

        assert_eq!(observed, (false, 1usize, false));
    }

    #[tokio::test]
    async fn event_stream_reader_is_empty_reflects_stream_population() {
        let store = InMemoryEventStore::new();
        let stream_id =
            StreamId::try_new("is-empty-observation".to_string()).expect("valid stream id");

        let initial_reader = store
            .read_stream::<TestEvent>(stream_id.clone())
            .await
            .expect("initial read to succeed");

        let event = TestEvent {
            stream_id: stream_id.clone(),
            data: "populated event".to_string(),
        };

        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(event))
            .expect("append should succeed");

        let _ = store
            .append_events(writes)
            .await
            .expect("append to succeed");

        let populated_reader = store
            .read_stream::<TestEvent>(stream_id)
            .await
            .expect("populated read to succeed");

        let observed = (
            initial_reader.is_empty(),
            initial_reader.len(),
            populated_reader.is_empty(),
            populated_reader.len(),
        );

        assert_eq!(observed, (true, 0usize, false, 1usize));
    }

    #[tokio::test]
    async fn read_stream_iterates_through_events_in_order() {
        let store = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("ordered-stream".to_string()).expect("valid stream id");

        let first_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "first".to_string(),
        };

        let second_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "second".to_string(),
        };

        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(first_event))
            .and_then(|writes| writes.append(second_event))
            .expect("append chain should succeed");

        let _ = store
            .append_events(writes)
            .await
            .expect("append to succeed");

        let reader = store
            .read_stream::<TestEvent>(stream_id)
            .await
            .expect("read to succeed");

        let collected: Vec<String> = reader.iter().map(|event| event.data.clone()).collect();

        let observed = (reader.is_empty(), collected);

        assert_eq!(
            observed,
            (false, vec!["first".to_string(), "second".to_string()])
        );
    }

    #[test]
    fn stream_writes_accepts_duplicate_stream_with_same_expected_version() {
        let stream_id = StreamId::try_new("duplicate-stream-same-version".to_string())
            .expect("valid stream id");

        let first_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "first-event".to_string(),
        };

        let second_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "second-event".to_string(),
        };

        let writes_result = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(first_event))
            .and_then(|writes| writes.append(second_event));

        assert!(writes_result.is_ok());
    }

    #[test]
    fn stream_writes_rejects_duplicate_stream_with_conflicting_expected_versions() {
        let stream_id =
            StreamId::try_new("duplicate-stream-conflict".to_string()).expect("valid stream id");

        let first_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "first-event-conflict".to_string(),
        };

        let second_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "second-event-conflict".to_string(),
        };

        let conflict = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(first_event))
            .and_then(|writes| writes.register_stream(stream_id.clone(), StreamVersion::new(1)))
            .and_then(|writes| writes.append(second_event));

        let message = conflict.unwrap_err().to_string();

        assert_eq!(
            message,
            "conflicting expected versions for stream duplicate-stream-conflict: first=0, second=1"
        );
    }

    #[tokio::test]
    async fn stream_writes_registers_stream_before_appending_multiple_events() {
        let store = InMemoryEventStore::new();
        let stream_id =
            StreamId::try_new("registered-stream".to_string()).expect("valid stream id");

        let first_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "first-registered-event".to_string(),
        };

        let second_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "second-registered-event".to_string(),
        };

        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(first_event))
            .and_then(|writes| writes.append(second_event))
            .expect("registered stream should accept events");

        let result = store.append_events(writes).await;

        assert!(
            result.is_ok(),
            "append should succeed when stream registered before events"
        );
    }

    #[test]
    fn stream_writes_rejects_appends_for_unregistered_streams() {
        let stream_id =
            StreamId::try_new("unregistered-stream".to_string()).expect("valid stream id");

        let event = TestEvent {
            stream_id: stream_id.clone(),
            data: "unregistered-event".to_string(),
        };

        let error = StreamWrites::new()
            .append(event)
            .expect_err("append without prior registration should fail");

        assert!(matches!(
            error,
            EventStoreError::UndeclaredStream { stream_id: ref actual } if *actual == stream_id
        ));
    }

    #[test]
    fn expected_versions_returns_registered_streams_and_versions() {
        let stream_a = StreamId::try_new("stream-a").expect("valid stream id");
        let stream_b = StreamId::try_new("stream-b").expect("valid stream id");

        let writes = StreamWrites::new()
            .register_stream(stream_a.clone(), StreamVersion::new(0))
            .and_then(|w| w.register_stream(stream_b.clone(), StreamVersion::new(5)))
            .expect("registration should succeed");

        let versions = writes.expected_versions();

        assert_eq!(versions.len(), 2);
        assert_eq!(versions.get(&stream_a), Some(&StreamVersion::new(0)));
        assert_eq!(versions.get(&stream_b), Some(&StreamVersion::new(5)));
    }

    #[test]
    fn stream_id_rejects_asterisk_metacharacter() {
        let result = StreamId::try_new("account-*");
        assert!(
            result.is_err(),
            "StreamId should reject asterisk glob metacharacter"
        );
    }

    #[test]
    fn stream_id_rejects_question_mark_metacharacter() {
        let result = StreamId::try_new("account-?");
        assert!(
            result.is_err(),
            "StreamId should reject question mark glob metacharacter"
        );
    }

    #[test]
    fn stream_id_rejects_open_bracket_metacharacter() {
        let result = StreamId::try_new("account-[");
        assert!(
            result.is_err(),
            "StreamId should reject open bracket glob metacharacter"
        );
    }

    #[test]
    fn stream_id_rejects_close_bracket_metacharacter() {
        let result = StreamId::try_new("account-]");
        assert!(
            result.is_err(),
            "StreamId should reject close bracket glob metacharacter"
        );
    }

    #[tokio::test]
    async fn event_reader_after_position_excludes_event_at_position() {
        // Given: An event store with 3 events
        let store = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("reader-test").expect("valid stream id");

        let event1 = TestEvent {
            stream_id: stream_id.clone(),
            data: "first".to_string(),
        };
        let event2 = TestEvent {
            stream_id: stream_id.clone(),
            data: "second".to_string(),
        };
        let event3 = TestEvent {
            stream_id: stream_id.clone(),
            data: "third".to_string(),
        };

        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|w| w.append(event1))
            .and_then(|w| w.append(event2))
            .and_then(|w| w.append(event3))
            .expect("append should succeed");

        store
            .append_events(writes)
            .await
            .expect("append to succeed");

        // First, read all events to get their positions
        let all_events = store
            .read_events::<TestEvent>(EventFilter::all(), EventPage::first(BatchSize::new(100)))
            .await
            .expect("read all events to succeed");

        assert_eq!(all_events.len(), 3, "Should have 3 events total");
        let (first_event, first_position) = &all_events[0];

        // When: We read events after the first event's position
        let page = EventPage::after(*first_position, BatchSize::new(100));
        let filter = EventFilter::all();
        let events = store
            .read_events::<TestEvent>(filter, page)
            .await
            .expect("read to succeed");

        // Then: We should get 2 events (event2 and event3), not including event1
        assert_eq!(events.len(), 2, "Should get 2 events after first position");
        assert_eq!(
            events[0].0.data, "second",
            "First returned event should be 'second'"
        );
        assert_eq!(
            events[1].0.data, "third",
            "Second returned event should be 'third'"
        );

        // And: The first event should NOT be in the results
        for (event, _pos) in &events {
            assert_ne!(
                event.data, first_event.data,
                "First event should be excluded"
            );
        }

        // And: All returned positions should be greater than first_position
        for (_event, pos) in &events {
            assert!(
                *pos > *first_position,
                "Returned position {} should be > first position {}",
                pos,
                first_position
            );
        }
    }
}
