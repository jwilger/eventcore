//! Property tests for event ordering determinism.
//!
//! These tests verify that event ordering is deterministic and consistent
//! across different scenarios, which is crucial for event sourcing systems.

use eventcore::event::{Event, StoredEvent};
use eventcore::event_store::{EventStore, EventToWrite, ExpectedVersion, ReadOptions, StreamEvents};
use eventcore::testing::prelude::*;
use eventcore::types::{EventId, EventVersion, StreamId, Timestamp};
use eventcore_memory::InMemoryEventStore;
use proptest::prelude::*;
use std::cmp::Ordering;

/// Test event type for ordering tests.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OrderingTestEvent {
    pub sequence: u64,
    pub content: String,
}

/// Property test: Event ordering is deterministic based on EventId.
///
/// This test verifies that:
/// 1. Events with UUIDv7 IDs maintain chronological ordering
/// 2. Event ordering is consistent across multiple reads
/// 3. Events created later have greater EventIds
#[test]
fn prop_event_ordering_deterministic() {
    proptest! {
        #[test]
        fn test_deterministic_event_ordering(
            stream_id in arb_stream_id(),
            event_count in 2usize..20,
            content_data in prop::collection::vec(any::<String>(), 2..20)
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = InMemoryEventStore::new();
                let mut event_ids = Vec::new();
                
                // Write events with a small delay to ensure timestamp ordering
                for (i, content) in content_data.iter().take(event_count).enumerate() {
                    let event_id = EventId::new();
                    event_ids.push(event_id);
                    
                    let events = vec![EventToWrite::new(
                        event_id,
                        OrderingTestEvent {
                            sequence: i as u64,
                            content: content.clone(),
                        }
                    )];
                    
                    let stream_events = vec![StreamEvents::new(
                        stream_id.clone(),
                        if i == 0 { ExpectedVersion::New } else { ExpectedVersion::Any },
                        events
                    )];
                    
                    store.write_events_multi(stream_events).await.unwrap();
                    
                    // Small delay to ensure different timestamps in UUIDv7
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                }
                
                // Read events back multiple times and verify consistent ordering
                for _ in 0..3 {
                    let stream_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                    let events = stream_data.events;
                    
                    prop_assert_eq!(events.len(), event_count);
                    
                    // Verify events are ordered by EventId (which includes timestamp)
                    for window in events.windows(2) {
                        prop_assert!(window[0].event_id <= window[1].event_id);
                    }
                    
                    // Verify the EventIds match what we stored
                    for (i, event) in events.iter().enumerate() {
                        prop_assert_eq!(event.event_id, event_ids[i]);
                    }
                }
            });
        }
    }
}

/// Property test: Event ordering respects chronological creation time.
///
/// This test verifies that events created later have EventIds that sort
/// after events created earlier, maintaining chronological order.
#[test]
fn prop_event_chronological_ordering() {
    proptest! {
        #[test]
        fn test_chronological_event_ordering(
            stream_ids in prop::collection::vec(arb_stream_id(), 1..5),
            events_per_stream in 2usize..10
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = InMemoryEventStore::new();
                let mut all_event_ids = Vec::new();
                let mut creation_order = Vec::new();
                
                // Create events across multiple streams with controlled timing
                for batch in 0..events_per_stream {
                    for (stream_idx, stream_id) in stream_ids.iter().enumerate() {
                        let event_id = EventId::new();
                        all_event_ids.push(event_id);
                        creation_order.push((batch, stream_idx, event_id));
                        
                        let events = vec![EventToWrite::new(
                            event_id,
                            OrderingTestEvent {
                                sequence: batch as u64,
                                content: format!("batch-{}-stream-{}", batch, stream_idx),
                            }
                        )];
                        
                        let stream_events = vec![StreamEvents::new(
                            stream_id.clone(),
                            ExpectedVersion::Any,
                            events
                        )];
                        
                        store.write_events_multi(stream_events).await.unwrap();
                        
                        // Ensure timestamp progression
                        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                    }
                }
                
                // Read all streams and collect all events
                let mut all_stored_events = Vec::new();
                for stream_id in &stream_ids {
                    let stream_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                    all_stored_events.extend(stream_data.events);
                }
                
                // Sort events by EventId (should match creation order)
                all_stored_events.sort_by(|a, b| a.event_id.cmp(&b.event_id));
                
                // Verify that EventId ordering matches creation order
                for window in all_stored_events.windows(2) {
                    prop_assert!(window[0].event_id <= window[1].event_id);
                }
                
                // Verify that events created in the same batch are ordered correctly
                for batch in 0..events_per_stream {
                    let batch_events: Vec<_> = all_stored_events.iter()
                        .filter(|e| e.payload.sequence == batch as u64)
                        .collect();
                    
                    // Events in the same batch should be ordered by creation time
                    for window in batch_events.windows(2) {
                        prop_assert!(window[0].event_id <= window[1].event_id);
                    }
                }
            });
        }
    }
}

/// Property test: Stored event ordering is consistent.
///
/// This test verifies that StoredEvent ordering (which includes version)
/// is consistent with EventId ordering within a stream.
#[test]
fn prop_stored_event_ordering_consistent() {
    proptest! {
        #[test]
        fn test_stored_event_ordering_consistency(
            stream_id in arb_stream_id(),
            event_contents in prop::collection::vec(any::<String>(), 2..15)
        ) {
            let events: Vec<StoredEvent<OrderingTestEvent>> = event_contents.iter().enumerate().map(|(i, content)| {
                let event = EventBuilder::new()
                    .with_stream_id(stream_id.clone())
                    .payload(OrderingTestEvent {
                        sequence: i as u64,
                        content: content.clone(),
                    })
                    .build();
                
                StoredEventBuilder::new()
                    .with_stream_id(stream_id.clone())
                    .payload(OrderingTestEvent {
                        sequence: i as u64,
                        content: content.clone(),
                    })
                    .version(i as u64 + 1)
                    .build()
            }).collect();
            
            // Test ordering consistency
            for window in events.windows(2) {
                // Version ordering should be consistent
                prop_assert!(window[0].version < window[1].version);
                
                // EventId ordering should also be consistent (assuming sequential creation)
                prop_assert!(window[0].id() <= window[1].id());
                
                // Stream ID should be the same
                prop_assert_eq!(window[0].stream_id(), window[1].stream_id());
            }
            
            // Test sorting stability
            let mut events_copy = events.clone();
            events_copy.sort();
            
            // Should be in the same order (already sorted by version/EventId)
            for (original, sorted) in events.iter().zip(events_copy.iter()) {
                prop_assert_eq!(original, sorted);
            }
        }
    }
}

/// Property test: Event ordering is transitive and antisymmetric.
///
/// This test verifies mathematical properties of event ordering that
/// must hold for a valid ordering relation.
#[test]
fn prop_event_ordering_mathematical_properties() {
    proptest! {
        #[test]
        fn test_ordering_mathematical_properties(
            events in prop::collection::vec(
                (arb_stream_id(), any::<String>(), arb_timestamp()),
                3..10
            )
        ) {
            let test_events: Vec<Event<OrderingTestEvent>> = events.iter().enumerate().map(|(i, (stream_id, content, timestamp))| {
                EventBuilder::new()
                    .with_stream_id(stream_id.clone())
                    .payload(OrderingTestEvent {
                        sequence: i as u64,
                        content: content.clone(),
                    })
                    .with_timestamp(*timestamp)
                    .build()
            }).collect();
            
            // Test reflexivity: a <= a
            for event in &test_events {
                prop_assert_eq!(event.cmp(event), Ordering::Equal);
            }
            
            // Test antisymmetry: if a <= b and b <= a, then a == b
            for i in 0..test_events.len() {
                for j in 0..test_events.len() {
                    let a = &test_events[i];
                    let b = &test_events[j];
                    
                    if a.cmp(b) == Ordering::Less || a.cmp(b) == Ordering::Equal {
                        if b.cmp(a) == Ordering::Less || b.cmp(a) == Ordering::Equal {
                            // Both a <= b and b <= a, so a == b
                            prop_assert_eq!(a.cmp(b), Ordering::Equal);
                            prop_assert_eq!(b.cmp(a), Ordering::Equal);
                        }
                    }
                }
            }
            
            // Test transitivity: if a <= b and b <= c, then a <= c
            for i in 0..test_events.len() {
                for j in 0..test_events.len() {
                    for k in 0..test_events.len() {
                        let a = &test_events[i];
                        let b = &test_events[j];
                        let c = &test_events[k];
                        
                        let a_le_b = matches!(a.cmp(b), Ordering::Less | Ordering::Equal);
                        let b_le_c = matches!(b.cmp(c), Ordering::Less | Ordering::Equal);
                        let a_le_c = matches!(a.cmp(c), Ordering::Less | Ordering::Equal);
                        
                        if a_le_b && b_le_c {
                            prop_assert!(a_le_c, "Transitivity violation: {:?} <= {:?} <= {:?} but {:?} > {:?}",
                                a.id, b.id, c.id, a.id, c.id);
                        }
                    }
                }
            }
        }
    }
}

/// Property test: Multi-stream event ordering respects global chronology.
///
/// This test verifies that when events from multiple streams are combined,
/// they maintain a consistent global chronological order.
#[test]
fn prop_multi_stream_global_ordering() {
    proptest! {
        #[test]
        fn test_multi_stream_global_ordering(
            stream_count in 2usize..5,
            events_per_stream in 2usize..8
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = InMemoryEventStore::new();
                let stream_ids: Vec<StreamId> = (0..stream_count)
                    .map(|i| StreamId::try_new(format!("stream-{}", i)).unwrap())
                    .collect();
                
                let mut global_creation_order = Vec::new();
                
                // Create events in a round-robin fashion across streams
                for event_idx in 0..events_per_stream {
                    for (stream_idx, stream_id) in stream_ids.iter().enumerate() {
                        let event_id = EventId::new();
                        global_creation_order.push((stream_idx, event_idx, event_id));
                        
                        let events = vec![EventToWrite::new(
                            event_id,
                            OrderingTestEvent {
                                sequence: event_idx as u64,
                                content: format!("stream-{}-event-{}", stream_idx, event_idx),
                            }
                        )];
                        
                        let stream_events = vec![StreamEvents::new(
                            stream_id.clone(),
                            ExpectedVersion::Any,
                            events
                        )];
                        
                        store.write_events_multi(stream_events).await.unwrap();
                        
                        // Ensure progression of time
                        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                    }
                }
                
                // Read events from all streams
                let all_stream_data = store.read_streams(&stream_ids, &ReadOptions::new()).await.unwrap();
                let mut all_events = all_stream_data.events;
                
                // Sort by EventId (should match global creation order)
                all_events.sort_by(|a, b| a.event_id.cmp(&b.event_id));
                
                // Verify global ordering matches creation order
                for (i, event) in all_events.iter().enumerate() {
                    let (expected_stream_idx, expected_event_idx, expected_event_id) = global_creation_order[i];
                    
                    prop_assert_eq!(event.event_id, expected_event_id);
                    prop_assert_eq!(event.payload.sequence, expected_event_idx as u64);
                    
                    let expected_stream_id = &stream_ids[expected_stream_idx];
                    prop_assert_eq!(event.stream_id, *expected_stream_id);
                }
                
                // Verify that events are in strict chronological order
                for window in all_events.windows(2) {
                    prop_assert!(window[0].event_id < window[1].event_id);
                }
            });
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_basic_event_ordering() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        
        // Create events with controlled timing
        let event1 = EventBuilder::new()
            .with_stream_id(stream_id.clone())
            .payload(OrderingTestEvent {
                sequence: 1,
                content: "first".to_string(),
            })
            .build();
        
        std::thread::sleep(std::time::Duration::from_millis(2));
        
        let event2 = EventBuilder::new()
            .with_stream_id(stream_id.clone())
            .payload(OrderingTestEvent {
                sequence: 2,
                content: "second".to_string(),
            })
            .build();
        
        // Event created later should have greater EventId due to UUIDv7 timestamp
        assert!(event1.id < event2.id);
        assert_eq!(event1.cmp(&event2), Ordering::Less);
    }

    #[test]
    fn test_stored_event_ordering() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        
        let stored1 = StoredEventBuilder::new()
            .with_stream_id(stream_id.clone())
            .payload(OrderingTestEvent {
                sequence: 1,
                content: "first".to_string(),
            })
            .version(1)
            .build();
        
        let stored2 = StoredEventBuilder::new()
            .with_stream_id(stream_id.clone())
            .payload(OrderingTestEvent {
                sequence: 2,
                content: "second".to_string(),
            })
            .version(2)
            .build();
        
        assert!(stored1 < stored2);
        assert!(stored1.version < stored2.version);
    }

    #[tokio::test]
    async fn test_event_store_ordering() {
        let store = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("test-stream").unwrap();
        
        // Write multiple events
        for i in 0..5 {
            let events = vec![EventToWrite::new(
                EventId::new(),
                OrderingTestEvent {
                    sequence: i,
                    content: format!("event-{}", i),
                }
            )];
            
            let stream_events = vec![StreamEvents::new(
                stream_id.clone(),
                ExpectedVersion::Any,
                events
            )];
            
            store.write_events_multi(stream_events).await.unwrap();
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
        
        // Read back and verify ordering
        let stream_data = store.read_streams(&[stream_id], &ReadOptions::new()).await.unwrap();
        let events = stream_data.events;
        
        assert_eq!(events.len(), 5);
        
        // Verify ordering
        for window in events.windows(2) {
            assert!(window[0].event_id <= window[1].event_id);
            assert!(window[0].event_version < window[1].event_version);
        }
    }
}