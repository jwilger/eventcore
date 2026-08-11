use super::*;
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

    fn event_type_name() -> &'static str {
        "TestEvent"
    }
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

#[test]
fn stream_prefix_rejects_glob_metacharacters() {
    for raw in ["account-*", "account-?", "account-[", "account-]"] {
        assert!(
            StreamPrefix::try_new(raw).is_err(),
            "StreamPrefix should reject glob metacharacter in {raw:?} (ADR-017)"
        );
    }
}

#[test]
fn stream_prefix_accepts_literal_prefix() {
    assert!(StreamPrefix::try_new("account-").is_ok());
}

#[test]
fn stream_pattern_rejects_invalid_glob() {
    // An unclosed character class is not a compilable glob pattern.
    assert!(
        StreamPattern::try_new("account-[").is_err(),
        "StreamPattern should reject an uncompilable glob pattern"
    );
}

#[test]
fn stream_pattern_star_matches_across_separator() {
    let pattern = StreamPattern::try_new("account-*").expect("valid glob");
    assert!(pattern.matches("account-1"));
    assert!(pattern.matches("account-1/sub"));
    assert!(!pattern.matches("order-1"));
}

#[test]
fn stream_pattern_question_mark_matches_single_char() {
    let pattern = StreamPattern::try_new("account-?").expect("valid glob");
    assert!(pattern.matches("account-1"));
    assert!(!pattern.matches("account-12"));
}

#[test]
fn stream_pattern_char_class_matches_digit() {
    let pattern = StreamPattern::try_new("account-[0-9]").expect("valid glob");
    assert!(pattern.matches("account-7"));
    assert!(!pattern.matches("account-a"));
}

#[test]
fn into_entries_returns_appended_events() {
    let stream_id = StreamId::try_new("into-entries-test").expect("valid stream id");
    let event = TestEvent {
        stream_id: stream_id.clone(),
        data: "test-data".to_string(),
    };

    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .and_then(|w| w.append(event))
        .expect("append should succeed");

    let entries = writes.into_entries();

    assert_eq!(entries.len(), 1);
}

#[test]
fn stream_version_increment_adds_one() {
    let v0 = StreamVersion::new(5);

    let v1 = v0.increment();

    assert_eq!(v1, StreamVersion::new(6));
}

#[tokio::test]
async fn collect_events_yields_all_events_in_order() {
    let stream_id = StreamId::try_new("collect-order-test").expect("valid stream id");
    let events = vec![
        TestEvent {
            stream_id: stream_id.clone(),
            data: "first".to_string(),
        },
        TestEvent {
            stream_id: stream_id.clone(),
            data: "second".to_string(),
        },
        TestEvent {
            stream_id: stream_id.clone(),
            data: "third".to_string(),
        },
    ];

    let stream = EventStream::new(futures::stream::iter(
        events.clone().into_iter().map(Ok::<_, EventStoreError>),
    ));

    let collected = collect_events(stream)
        .await
        .expect("collect should succeed");

    assert_eq!(collected, events);
}

#[tokio::test]
async fn collect_events_returns_empty_for_empty_stream() {
    let stream = EventStream::new(futures::stream::iter(Vec::<
        Result<TestEvent, EventStoreError>,
    >::new()));

    let collected = collect_events(stream)
        .await
        .expect("collect should succeed");

    assert!(collected.is_empty());
}

#[tokio::test]
async fn collect_events_propagates_first_error_item() {
    let stream_id = StreamId::try_new("collect-error-test").expect("valid stream id");
    let items: Vec<Result<TestEvent, EventStoreError>> = vec![
        Ok(TestEvent {
            stream_id: stream_id.clone(),
            data: "first".to_string(),
        }),
        Err(EventStoreError::DeserializationFailed {
            stream_id: stream_id.clone(),
            detail: "bad event".to_string(),
        }),
    ];

    let stream = EventStream::new(futures::stream::iter(items));

    let error = collect_events(stream)
        .await
        .expect_err("collect should surface the error item");

    assert!(matches!(
        error,
        EventStoreError::DeserializationFailed { .. }
    ));
}
