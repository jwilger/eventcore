//! Custom assertions for testing event sourcing domain invariants.
//!
//! This module provides assertion functions that verify common properties
//! and invariants in event sourcing systems.

use crate::event::StoredEvent;
use crate::event_store::StoredEvent as StoreStoredEvent;
use crate::types::{EventVersion, StreamId};
use std::collections::HashMap;
use std::fmt::Debug;

/// Asserts that a collection of events is properly ordered by their event IDs.
///
/// Since `EventId` uses `UUIDv7`, events should be chronologically ordered.
///
/// # Panics
/// Panics if events are not in chronological order.
///
/// # Example
/// ```rust,ignore
/// use eventcore::testing::assertions::assert_events_ordered;
///
/// let events = create_test_events();
/// assert_events_ordered(&events);
/// ```
pub fn assert_events_ordered<E>(events: &[StoredEvent<E>])
where
    E: PartialEq + Eq + Debug,
{
    for window in events.windows(2) {
        assert!(
            window[0] <= window[1],
            "Events are not ordered: {:?} should come before {:?}",
            window[0].id(),
            window[1].id()
        );
    }
}

/// Asserts that store events are properly ordered.
pub fn assert_store_events_ordered<E>(events: &[StoreStoredEvent<E>])
where
    E: Debug,
{
    for window in events.windows(2) {
        assert!(
            window[0].event_id <= window[1].event_id,
            "Events are not ordered: {:?} should come before {:?}",
            window[0].event_id,
            window[1].event_id
        );
    }
}

/// Asserts that event versions within a stream progress monotonically.
///
/// # Panics
/// Panics if versions don't increase by 1 for each event in the same stream.
///
/// # Example
/// ```rust,ignore
/// use eventcore::testing::assertions::assert_stream_version_progression;
///
/// let events = read_stream_events("stream-1");
/// assert_stream_version_progression(&events);
/// ```
pub fn assert_stream_version_progression<E>(events: &[StoredEvent<E>])
where
    E: PartialEq + Eq + Debug,
{
    let mut stream_versions: HashMap<StreamId, EventVersion> = HashMap::new();

    for event in events {
        let stream_id = event.stream_id();
        let version = event.version;

        if let Some(&last_version) = stream_versions.get(stream_id) {
            let expected_version = last_version.next();
            assert_eq!(
                version, expected_version,
                "Version gap detected in stream {stream_id:?}: expected {expected_version:?}, got {version:?}"
            );
        } else {
            // First event in stream, could start at any version
            // Just record it
        }

        stream_versions.insert(stream_id.clone(), version);
    }
}

/// Asserts that store event versions progress correctly.
pub fn assert_store_stream_version_progression<E>(events: &[StoreStoredEvent<E>])
where
    E: Debug,
{
    let mut stream_versions: HashMap<StreamId, EventVersion> = HashMap::new();

    for event in events {
        let stream_id = &event.stream_id;
        let version = event.event_version;

        if let Some(&last_version) = stream_versions.get(stream_id) {
            let expected_version = last_version.next();
            assert_eq!(
                version, expected_version,
                "Version gap detected in stream {stream_id:?}: expected {expected_version:?}, got {version:?}"
            );
        }

        stream_versions.insert(stream_id.clone(), version);
    }
}

/// Asserts that all events have unique IDs.
///
/// # Panics
/// Panics if any duplicate event IDs are found.
pub fn assert_unique_event_ids<E>(events: &[StoredEvent<E>])
where
    E: PartialEq + Eq + Debug,
{
    let mut seen_ids = std::collections::HashSet::new();

    for event in events {
        let event_id = event.id();
        assert!(
            seen_ids.insert(*event_id),
            "Duplicate event ID found: {event_id:?}"
        );
    }
}

/// Asserts that store events have unique IDs.
pub fn assert_unique_store_event_ids<E>(events: &[StoreStoredEvent<E>])
where
    E: Debug,
{
    let mut seen_ids = std::collections::HashSet::new();

    for event in events {
        assert!(
            seen_ids.insert(event.event_id),
            "Duplicate event ID found: {:?}",
            event.event_id
        );
    }
}

/// Asserts that a command execution result contains expected events.
///
/// This is useful for verifying that commands produce the correct events.
///
/// # Example
/// ```rust,ignore
/// use eventcore::testing::assertions::assert_events_match;
///
/// let result = command.handle(state, input).await?;
/// assert_events_match(
///     &result,
///     vec![
///         ("account-123", AccountEvent::Deposited { amount: 100 }),
///         ("account-456", AccountEvent::Withdrawn { amount: 100 }),
///     ]
/// );
/// ```
pub fn assert_events_match<E>(actual: &[(StreamId, E)], expected: &[(&str, E)])
where
    E: PartialEq + Debug,
{
    assert_eq!(
        actual.len(),
        expected.len(),
        "Expected {} events, got {}",
        expected.len(),
        actual.len()
    );

    for (i, ((actual_stream, actual_event), (expected_stream, expected_event))) in
        actual.iter().zip(expected.iter()).enumerate()
    {
        assert_eq!(
            actual_stream.as_ref(),
            *expected_stream,
            "Event {} stream mismatch: expected {:?}, got {:?}",
            i,
            expected_stream,
            actual_stream.as_ref()
        );

        assert_eq!(
            actual_event, expected_event,
            "Event {i} payload mismatch on stream {expected_stream:?}"
        );
    }
}

/// Asserts that a stream contains a specific number of events.
pub fn assert_stream_event_count<E>(
    events: &[StoredEvent<E>],
    stream_id: &str,
    expected_count: usize,
) where
    E: PartialEq + Eq + Debug,
{
    let stream_id = StreamId::try_new(stream_id).expect("Invalid stream ID in assertion");
    let actual_count = events
        .iter()
        .filter(|e| e.stream_id() == &stream_id)
        .count();

    assert_eq!(
        actual_count, expected_count,
        "Stream {stream_id:?} has {actual_count} events, expected {expected_count}"
    );
}

/// Asserts that events are idempotent when applied multiple times.
///
/// This helper applies the same event multiple times and verifies the state
/// remains the same after the first application.
///
/// # Example
/// ```rust,ignore
/// use eventcore::testing::assertions::assert_event_idempotent;
///
/// assert_event_idempotent(
///     initial_state,
///     &event,
///     |state, event| {
///         // Apply event to state
///         command.apply(state, event);
///     }
/// );
/// ```
pub fn assert_event_idempotent<S, E, F>(initial_state: S, event: &E, mut apply_fn: F)
where
    S: Clone + PartialEq + Debug,
    E: Clone + Debug,
    F: FnMut(&mut S, &E),
{
    let mut state1 = initial_state.clone();
    let mut state2 = initial_state;

    // Apply once
    apply_fn(&mut state1, event);

    // Apply twice
    apply_fn(&mut state2, event);
    apply_fn(&mut state2, event);

    assert_eq!(
        state1, state2,
        "Event is not idempotent. State differs after applying event {event:?} multiple times"
    );
}

/// Asserts that a specific event exists in a collection.
pub fn assert_event_exists<E, P>(events: &[StoredEvent<E>], predicate: P)
where
    E: PartialEq + Eq + Debug,
    P: Fn(&StoredEvent<E>) -> bool,
{
    assert!(
        events.iter().any(predicate),
        "Expected event not found in collection"
    );
}

/// Asserts that no event matches a predicate.
pub fn assert_no_event_matches<E, P>(events: &[StoredEvent<E>], predicate: P)
where
    E: PartialEq + Eq + Debug,
    P: Fn(&StoredEvent<E>) -> bool,
{
    assert!(
        !events.iter().any(predicate),
        "Unexpected event found in collection"
    );
}

/// Asserts properties about event metadata.
pub fn assert_event_metadata<E, F>(event: &StoredEvent<E>, assertion: F)
where
    E: PartialEq + Eq + Debug,
    F: FnOnce(&crate::metadata::EventMetadata),
{
    assertion(event.metadata());
}

/// Asserts that all events in a collection have the same correlation ID.
pub fn assert_same_correlation_id<E>(events: &[StoredEvent<E>])
where
    E: PartialEq + Eq + Debug,
{
    if events.is_empty() {
        return;
    }

    let first_correlation_id = &events[0].metadata().correlation_id;

    for (i, event) in events.iter().enumerate().skip(1) {
        assert_eq!(
            &event.metadata().correlation_id,
            first_correlation_id,
            "Event {i} has different correlation ID"
        );
    }
}

/// Asserts that events form a valid causation chain.
pub fn assert_causation_chain<E>(events: &[StoredEvent<E>])
where
    E: PartialEq + Eq + Debug,
{
    for window in events.windows(2) {
        let first_event_id = window[0].id();
        let second_causation_id = window[1].metadata().causation_id.as_ref();

        if let Some(causation_id) = second_causation_id {
            // If causation is set, it should reference the previous event
            assert_eq!(
                causation_id.as_ref(),
                first_event_id.as_ref(),
                "Broken causation chain: event {:?} should be caused by {:?}",
                window[1].id(),
                first_event_id
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::CorrelationId;
    use crate::testing::builders::{create_event_sequence, StoredEventBuilder};

    #[test]
    fn test_assert_events_ordered_passes() {
        let events = create_event_sequence("test-stream", vec!["event1", "event2", "event3"]);
        assert_events_ordered(&events); // Should not panic
    }

    #[test]
    fn test_assert_stream_version_progression_passes() {
        let events = create_event_sequence("test-stream", vec!["event1", "event2", "event3"]);
        assert_stream_version_progression(&events); // Should not panic
    }

    #[test]
    fn test_assert_unique_event_ids_passes() {
        let events = create_event_sequence("test-stream", vec!["event1", "event2", "event3"]);
        assert_unique_event_ids(&events); // Should not panic
    }

    #[test]
    fn test_assert_events_match_passes() {
        let stream1 = StreamId::try_new("stream1").unwrap();
        let stream2 = StreamId::try_new("stream2").unwrap();

        let actual = vec![(stream1, "event1"), (stream2, "event2")];

        assert_events_match(&actual, &[("stream1", "event1"), ("stream2", "event2")]);
    }

    #[test]
    #[should_panic(expected = "Expected 2 events, got 1")]
    fn test_assert_events_match_fails_on_count() {
        let stream1 = StreamId::try_new("stream1").unwrap();
        let actual = vec![(stream1, "event1")];

        assert_events_match(&actual, &[("stream1", "event1"), ("stream2", "event2")]);
    }

    #[test]
    fn test_assert_event_idempotent_passes() {
        #[derive(Clone, PartialEq, Debug)]
        struct State {
            count: i32,
        }

        let initial = State { count: 0 };
        let event = "increment";

        assert_event_idempotent(initial, &event, |state, _| {
            // Idempotent operation: set to specific value
            state.count = 1;
        });
    }

    #[test]
    #[should_panic(expected = "Event is not idempotent")]
    fn test_assert_event_idempotent_fails() {
        #[derive(Clone, PartialEq, Debug)]
        struct State {
            count: i32,
        }

        let initial = State { count: 0 };
        let event = "increment";

        assert_event_idempotent(initial, &event, |state, _| {
            // Non-idempotent operation: increment
            state.count += 1;
        });
    }

    #[test]
    fn test_assert_stream_event_count() {
        let mut events = create_event_sequence("stream1", vec!["e1", "e2"]);
        events.extend(create_event_sequence("stream2", vec!["e3"]));

        assert_stream_event_count(&events, "stream1", 2);
        assert_stream_event_count(&events, "stream2", 1);
    }

    #[test]
    fn test_assert_event_exists() {
        let events = create_event_sequence("test", vec!["event1", "event2", "event3"]);

        assert_event_exists(&events, |e| e.payload() == &"event2");
    }

    #[test]
    #[should_panic(expected = "Expected event not found")]
    fn test_assert_event_exists_fails() {
        let events = create_event_sequence("test", vec!["event1", "event2"]);

        assert_event_exists(&events, |e| e.payload() == &"event3");
    }

    #[test]
    fn test_assert_same_correlation_id() {
        let correlation_id = CorrelationId::new();

        let events: Vec<_> = (0..3)
            .map(|i| {
                StoredEventBuilder::new()
                    .stream_id("test")
                    .payload(format!("event{i}"))
                    .version(i + 1)
                    .with_correlation_id(correlation_id)
                    .build()
            })
            .collect();

        assert_same_correlation_id(&events); // Should not panic
    }
}
