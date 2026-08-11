use super::*;
use eventcore_types::collect_events;
use eventcore_types::{BatchSize, EventFilter, EventPage};
use serde::{Deserialize, Serialize};
use serde_json::json;

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

    fn event_type_name() -> &'static str {
        "TestEvent"
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

    let stream = store
        .read_stream::<TestEvent>(stream_id)
        .await
        .expect("read to succeed");
    let events = collect_events(stream).await.expect("collect to succeed");

    let observed = (events.is_empty(), events.len());

    assert_eq!(observed, (false, 1usize));
}

#[tokio::test]
async fn event_stream_reader_is_empty_reflects_stream_population() {
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("is-empty-observation".to_string()).expect("valid stream id");

    let initial_stream = store
        .read_stream::<TestEvent>(stream_id.clone())
        .await
        .expect("initial read to succeed");
    let initial_events = collect_events(initial_stream)
        .await
        .expect("collect to succeed");

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

    let populated_stream = store
        .read_stream::<TestEvent>(stream_id)
        .await
        .expect("populated read to succeed");
    let populated_events = collect_events(populated_stream)
        .await
        .expect("collect to succeed");

    let observed = (
        initial_events.is_empty(),
        initial_events.len(),
        populated_events.is_empty(),
        populated_events.len(),
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

    let stream = store
        .read_stream::<TestEvent>(stream_id)
        .await
        .expect("read to succeed");
    let events = collect_events(stream).await.expect("collect to succeed");

    let collected: Vec<String> = events.iter().map(|event| event.data.clone()).collect();

    let observed = (events.is_empty(), collected);

    assert_eq!(
        observed,
        (false, vec!["first".to_string(), "second".to_string()])
    );
}

#[test]
fn stream_writes_accepts_duplicate_stream_with_same_expected_version() {
    let stream_id =
        StreamId::try_new("duplicate-stream-same-version".to_string()).expect("valid stream id");

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
    let stream_id = StreamId::try_new("registered-stream".to_string()).expect("valid stream id");

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
    let stream_id = StreamId::try_new("unregistered-stream".to_string()).expect("valid stream id");

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

    let _ = store
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

#[tokio::test]
async fn in_memory_event_store_implements_checkpoint_store() {
    // Given: An InMemoryEventStore
    let store = InMemoryEventStore::new();

    // When: We save a checkpoint
    let position = StreamPosition::new(Uuid::now_v7());
    CheckpointStore::save(&store, "test-projector", position)
        .await
        .expect("save should succeed");

    // Then: We can load it back
    let loaded = CheckpointStore::load(&store, "test-projector")
        .await
        .expect("load should succeed");
    assert_eq!(loaded, Some(position));
}

#[tokio::test]
async fn in_memory_event_store_implements_projector_coordinator() {
    // Given: An InMemoryEventStore
    let store = InMemoryEventStore::new();

    // When: We try to acquire leadership
    let guard = ProjectorCoordinator::try_acquire(&store, "test-projector").await;

    // Then: It should succeed
    assert!(guard.is_ok(), "should acquire leadership");
}

#[tokio::test]
async fn command_state_snapshot_save_does_not_replace_a_newer_projection() {
    let store = InMemoryEventStore::new();
    let snapshot_id = CommandStateSnapshotId::try_new("accounts::snapshot".to_string())
        .expect("valid snapshot id");
    let stream_id = StreamId::try_new("accounts::primary".to_string()).expect("valid stream id");

    let newer = CommandStateSnapshot::new(
        json!({ "balance": 200 }),
        HashMap::from([(stream_id.clone(), StreamVersion::new(2))]),
    );
    let stale = CommandStateSnapshot::new(
        json!({ "balance": 100 }),
        HashMap::from([(stream_id.clone(), StreamVersion::new(1))]),
    );

    store
        .save_command_state_snapshot(snapshot_id.clone(), newer)
        .await
        .expect("newer projection persists");
    store
        .save_command_state_snapshot(snapshot_id.clone(), stale)
        .await
        .expect("stale persistence request is handled");

    let snapshot = store
        .load_command_state_snapshot(snapshot_id)
        .await
        .expect("projection loads")
        .expect("projection remains present");

    assert_eq!(snapshot.state, json!({ "balance": 200 }));
    assert_eq!(
        snapshot.stream_versions.get(&stream_id),
        Some(&StreamVersion::new(2))
    );
}
