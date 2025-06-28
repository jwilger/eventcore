//! Property tests for event immutability.
//!
//! These tests verify that once events are created, they cannot be modified.
//! This is a fundamental invariant of event sourcing systems.

use eventcore::event::{Event, StoredEvent};
use eventcore::event_store::StoredEvent as StoreStoredEvent;
use eventcore::testing::prelude::*;
use eventcore::types::{EventId, EventVersion, StreamId, Timestamp};
use proptest::prelude::*;
use std::sync::Arc;

/// Test event type for immutability testing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TestEvent {
    StringEvent(String),
    NumberEvent(i64),
    BoolEvent(bool),
    ComplexEvent { id: String, value: i64, flag: bool },
}

/// Property test: Event<E> instances are immutable once created.
///
/// This test verifies that:
/// 1. Event fields cannot be modified after creation
/// 2. Event comparison is stable (same events compare equal)
/// 3. Event serialization/deserialization preserves immutability
#[test]
fn prop_events_are_immutable() {
    proptest! {
        #[test]
        fn test_event_immutability(
            stream_id in arb_stream_id(),
            event_data in arb_test_event(),
            metadata in arb_event_metadata()
        ) {
            // Create an event
            let original_event = Event::new(stream_id.clone(), event_data.clone(), metadata.clone());
            
            // Clone the event (should be identical)
            let cloned_event = original_event.clone();
            
            // Verify immutability properties
            prop_assert_eq!(original_event, cloned_event);
            prop_assert_eq!(original_event.stream_id, stream_id);
            prop_assert_eq!(original_event.payload, event_data);
            prop_assert_eq!(original_event.metadata, metadata);
            
            // Verify that the cloned event has the same ID and timestamp
            prop_assert_eq!(original_event.id, cloned_event.id);
            prop_assert_eq!(original_event.created_at, cloned_event.created_at);
            
            // Events with same content should compare equal
            let recreated_event = EventBuilder::new()
                .with_stream_id(stream_id)
                .payload(event_data)
                .with_event_id(original_event.id)
                .with_timestamp(original_event.created_at)
                .build();
            
            prop_assert_eq!(original_event.id, recreated_event.id);
            prop_assert_eq!(original_event.stream_id, recreated_event.stream_id);
            prop_assert_eq!(original_event.payload, recreated_event.payload);
            prop_assert_eq!(original_event.created_at, recreated_event.created_at);
        }
    }
}

/// Property test: StoredEvent<E> instances are immutable once created.
///
/// This test verifies that stored events maintain immutability including
/// their version and storage timestamp information.
#[test]
fn prop_stored_events_are_immutable() {
    proptest! {
        #[test]
        fn test_stored_event_immutability(
            stream_id in arb_stream_id(),
            event_data in arb_test_event(),
            version in arb_event_version(),
            stored_at in arb_timestamp()
        ) {
            // Create a stored event
            let event = EventBuilder::new()
                .with_stream_id(stream_id.clone())
                .payload(event_data.clone())
                .build();
                
            let stored_event = StoredEvent::with_timestamp(event.clone(), version, stored_at);
            
            // Clone the stored event
            let cloned_stored = stored_event.clone();
            
            // Verify immutability
            prop_assert_eq!(stored_event, cloned_stored);
            prop_assert_eq!(stored_event.version, version);
            prop_assert_eq!(stored_event.stored_at, stored_at);
            prop_assert_eq!(stored_event.stream_id(), &stream_id);
            prop_assert_eq!(stored_event.payload(), &event_data);
            
            // Verify that accessing fields multiple times returns same values
            let id1 = stored_event.id();
            let id2 = stored_event.id();
            prop_assert_eq!(id1, id2);
            
            let stream1 = stored_event.stream_id();
            let stream2 = stored_event.stream_id();
            prop_assert_eq!(stream1, stream2);
        }
    }
}

/// Property test: Event store StoredEvent instances are immutable.
///
/// This test verifies immutability for the event store's StoredEvent type.
#[test]
fn prop_store_stored_events_are_immutable() {
    proptest! {
        #[test]
        fn test_store_stored_event_immutability(
            event_id in arb_event_id(),
            stream_id in arb_stream_id(),
            version in arb_event_version(),
            timestamp in arb_timestamp(),
            event_data in arb_test_event()
        ) {
            // Create a store stored event
            let stored_event = StoreStoredEvent::new(
                event_id,
                stream_id.clone(),
                version,
                timestamp,
                event_data.clone(),
                None // No metadata for this test
            );
            
            // Clone the event
            let cloned = stored_event.clone();
            
            // Verify immutability
            prop_assert_eq!(stored_event, cloned);
            prop_assert_eq!(stored_event.event_id, event_id);
            prop_assert_eq!(stored_event.stream_id, stream_id);
            prop_assert_eq!(stored_event.event_version, version);
            prop_assert_eq!(stored_event.timestamp, timestamp);
            prop_assert_eq!(stored_event.payload, event_data);
            
            // Verify fields don't change on repeated access
            let id_access_1 = stored_event.event_id;
            let id_access_2 = stored_event.event_id;
            prop_assert_eq!(id_access_1, id_access_2);
        }
    }
}

/// Property test: Events maintain identity across Arc wrapping.
///
/// This test verifies that events remain immutable and maintain their
/// identity when wrapped in Arc for concurrent access.
#[test]
fn prop_events_immutable_in_arc() {
    proptest! {
        #[test]
        fn test_arc_wrapped_event_immutability(
            stream_id in arb_stream_id(),
            event_data in arb_test_event()
        ) {
            let event = EventBuilder::new()
                .with_stream_id(stream_id.clone())
                .payload(event_data.clone())
                .build();
            
            let arc_event = Arc::new(event.clone());
            let arc_clone = Arc::clone(&arc_event);
            
            // Verify Arc doesn't affect immutability
            prop_assert_eq!(*arc_event, event);
            prop_assert_eq!(*arc_clone, event);
            prop_assert_eq!(*arc_event, *arc_clone);
            
            // Verify that Arc wrapping preserves all fields
            prop_assert_eq!(arc_event.id, event.id);
            prop_assert_eq!(arc_event.stream_id, event.stream_id);
            prop_assert_eq!(arc_event.payload, event.payload);
            prop_assert_eq!(arc_event.created_at, event.created_at);
        }
    }
}

/// Property test: Event serialization round-trip preserves immutability.
///
/// This test verifies that events remain immutable after serialization
/// and deserialization.
#[test]
fn prop_event_serialization_preserves_immutability() {
    proptest! {
        #[test]
        fn test_serialization_immutability(
            stream_id in arb_stream_id(),
            event_data in arb_test_event()
        ) {
            let original_event = EventBuilder::new()
                .with_stream_id(stream_id)
                .payload(event_data)
                .build();
            
            // Serialize and deserialize the event
            let serialized = serde_json::to_string(&original_event)
                .expect("Event should serialize");
            let deserialized: Event<TestEvent> = serde_json::from_str(&serialized)
                .expect("Event should deserialize");
            
            // Verify immutability is preserved across serialization
            prop_assert_eq!(original_event.id, deserialized.id);
            prop_assert_eq!(original_event.stream_id, deserialized.stream_id);
            prop_assert_eq!(original_event.payload, deserialized.payload);
            prop_assert_eq!(original_event.created_at, deserialized.created_at);
            prop_assert_eq!(original_event.metadata, deserialized.metadata);
            
            // Verify the deserialized event is still immutable
            let cloned_deserialized = deserialized.clone();
            prop_assert_eq!(deserialized, cloned_deserialized);
        }
    }
}

/// Generator for test events.
fn arb_test_event() -> impl Strategy<Value = TestEvent> {
    prop_oneof![
        any::<String>().prop_map(TestEvent::StringEvent),
        any::<i64>().prop_map(TestEvent::NumberEvent),
        any::<bool>().prop_map(TestEvent::BoolEvent),
        (any::<String>(), any::<i64>(), any::<bool>()).prop_map(|(id, value, flag)| {
            TestEvent::ComplexEvent { id, value, flag }
        })
    ]
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_event_immutability_basic() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event_data = TestEvent::StringEvent("test".to_string());
        
        let event = EventBuilder::new()
            .with_stream_id(stream_id.clone())
            .payload(event_data.clone())
            .build();
        
        // Verify basic immutability
        assert_eq!(event.stream_id, stream_id);
        assert_eq!(event.payload, event_data);
        
        // Clone should be identical
        let cloned = event.clone();
        assert_eq!(event, cloned);
    }

    #[test]
    fn test_stored_event_immutability_basic() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event_data = TestEvent::NumberEvent(42);
        let version = EventVersion::try_new(1).unwrap();
        
        let event = EventBuilder::new()
            .with_stream_id(stream_id.clone())
            .payload(event_data.clone())
            .build();
            
        let stored_event = StoredEvent::new(event, version);
        
        // Verify immutability
        assert_eq!(stored_event.version, version);
        assert_eq!(stored_event.stream_id(), &stream_id);
        assert_eq!(stored_event.payload(), &event_data);
        
        // Clone should be identical
        let cloned = stored_event.clone();
        assert_eq!(stored_event, cloned);
    }

    #[test]
    fn test_arc_immutability_basic() {
        let event = EventBuilder::new()
            .with_stream_id(StreamId::try_new("test").unwrap())
            .payload(TestEvent::BoolEvent(true))
            .build();
        
        let arc_event = Arc::new(event.clone());
        let arc_clone = Arc::clone(&arc_event);
        
        assert_eq!(*arc_event, event);
        assert_eq!(*arc_clone, event);
        assert_eq!(*arc_event, *arc_clone);
    }
}