//! Comprehensive property-based test suite for eventcore library.
//!
//! This integration test module runs all property-based tests to verify
//! fundamental invariants of the event sourcing system.

use eventcore::{
    EventId, EventStore, EventToWrite, EventVersion, ExpectedVersion, ReadOptions, StreamEvents,
    StreamId,
};
use eventcore_memory::InMemoryEventStore;
use proptest::prelude::*;
use proptest::strategy::ValueTree;

// Basic generators
fn arb_stream_id() -> impl Strategy<Value = StreamId> {
    "[a-zA-Z0-9][a-zA-Z0-9._-]{0,254}"
        .prop_filter_map("Invalid StreamId", |s| StreamId::try_new(s).ok())
}

/// Test that verifies the property test framework itself works correctly.
#[test]
fn test_property_framework_sanity() {
    // Test that our property test generators work
    let mut runner = proptest::test_runner::TestRunner::default();

    // Test stream ID generator
    let stream_id = arb_stream_id().new_tree(&mut runner).unwrap().current();
    assert!(!stream_id.as_ref().is_empty());
    assert!(stream_id.as_ref().len() <= 255);
}

proptest! {
    #[test]
    fn test_basic_scenario(
        stream_ids in prop::collection::vec(arb_stream_id(), 1..3),
        events_per_stream in 1usize..5
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = InMemoryEventStore::new();

            for stream_id in &stream_ids {
                for i in 0..events_per_stream {
                    let event = EventToWrite::new(
                        EventId::new(),
                        format!("event-{i}")
                    );

                    let stream_events = vec![StreamEvents::new(
                        stream_id.clone(),
                        ExpectedVersion::Any,
                        vec![event]
                    )];

                    let result = store.write_events_multi(stream_events).await;
                    prop_assert!(result.is_ok());
                }
            }

            // Read back to verify
            for stream_id in &stream_ids {
                let data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await;
                prop_assert!(data.is_ok());
                let stream_data = data.unwrap();
                prop_assert_eq!(stream_data.events.len(), events_per_stream);
            }

            Ok(())
        })?;
    }
}

/// Test event immutability property.
#[test]
fn test_event_immutability() {
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test-stream").unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let original_event = EventToWrite::new(EventId::new(), "test-event".to_string());

        let original_id = original_event.event_id;
        let original_payload = original_event.payload.clone();

        let stream_events = vec![StreamEvents::new(
            stream_id.clone(),
            ExpectedVersion::New,
            vec![original_event],
        )];

        store.write_events_multi(stream_events).await.unwrap();

        // Read back the event
        let stream_data = store
            .read_streams(&[stream_id], &ReadOptions::new())
            .await
            .unwrap();
        let stored_event = &stream_data.events[0];

        // Verify immutability - event should be identical
        assert_eq!(stored_event.event_id, original_id);
        assert_eq!(stored_event.payload, original_payload);

        // Clone should be identical
        let cloned_event = stored_event.clone();
        assert_eq!(stored_event, &cloned_event);
    });
}

/// Test version monotonicity property.
#[test]
fn test_version_monotonicity() {
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test-stream").unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut current_version = EventVersion::initial();

        // Write multiple events and verify version progression
        for i in 0..5 {
            let event = EventToWrite::new(EventId::new(), format!("event-{i}"));

            let expected_version = if i == 0 {
                ExpectedVersion::New
            } else {
                ExpectedVersion::Exact(current_version)
            };

            let stream_events = vec![StreamEvents::new(
                stream_id.clone(),
                expected_version,
                vec![event],
            )];

            let result = store.write_events_multi(stream_events).await.unwrap();
            let new_version = *result.get(&stream_id).unwrap();

            // Verify version increases monotonically
            assert!(new_version > current_version);
            current_version = new_version;
        }

        // Verify final state
        let stream_data = store
            .read_streams(&[stream_id], &ReadOptions::new())
            .await
            .unwrap();
        let events = stream_data.events;

        assert_eq!(events.len(), 5);

        // Verify no version gaps
        for (i, event) in events.iter().enumerate() {
            let expected_version = EventVersion::try_new(i as u64 + 1).unwrap();
            assert_eq!(event.event_version, expected_version);
        }
    });
}

/// Test event ordering property.
#[test]
fn test_event_ordering() {
    let store = InMemoryEventStore::new();
    let stream_id = StreamId::try_new("test-stream").unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut event_ids = Vec::new();

        // Write events with delays to ensure different timestamps
        for i in 0..5 {
            let event_id = EventId::new();
            event_ids.push(event_id);

            let event = EventToWrite::new(event_id, format!("event-{i}"));

            let stream_events = vec![StreamEvents::new(
                stream_id.clone(),
                ExpectedVersion::Any,
                vec![event],
            )];

            store.write_events_multi(stream_events).await.unwrap();

            // Small delay to ensure timestamp progression
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }

        // Read back and verify ordering
        let stream_data = store
            .read_streams(&[stream_id], &ReadOptions::new())
            .await
            .unwrap();
        let events = stream_data.events;

        // Verify events maintain order
        for window in events.windows(2) {
            assert!(window[0].event_id <= window[1].event_id);
            assert!(window[0].event_version < window[1].event_version);
        }

        // Verify events match original order
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.event_id, event_ids[i]);
        }
    });
}
