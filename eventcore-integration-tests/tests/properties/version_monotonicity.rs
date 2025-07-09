//! Property tests for stream version monotonicity.
//!
//! These tests verify that stream versions always increase monotonically,
//! which is essential for event ordering and optimistic concurrency control.

use eventcore::event_store::{
    EventStore, EventToWrite, ExpectedVersion, ReadOptions, StreamData, StreamEvents,
};
use eventcore::testing::prelude::*;
use eventcore::types::{EventId, EventVersion, StreamId};
use eventcore_memory::InMemoryEventStore;
use proptest::prelude::*;
use std::collections::HashMap;

/// Test event type for version testing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VersionTestEvent {
    pub sequence: u64,
    pub data: String,
}

/// Property test: Stream versions increase monotonically.
///
/// This test verifies that:
/// 1. New events get incrementing version numbers
/// 2. Version numbers never decrease
/// 3. Version numbers have no gaps when events are written sequentially
#[test]
fn prop_stream_versions_monotonic() {
    proptest! {
        #[test]
        fn test_version_monotonicity(
            stream_id in arb_stream_id(),
            event_count in 1usize..20,
            event_data in prop::collection::vec(any::<String>(), 1..20)
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = InMemoryEventStore::new();
                let mut expected_version = EventVersion::initial();
                
                // Write events one by one and verify version progression
                for (i, data) in event_data.iter().take(event_count).enumerate() {
                    let events = vec![EventToWrite::new(
                        EventId::new(),
                        VersionTestEvent {
                            sequence: i as u64,
                            data: data.clone(),
                        }
                    )];
                    
                    let stream_events = vec![StreamEvents::new(
                        stream_id.clone(),
                        if i == 0 { ExpectedVersion::New } else { ExpectedVersion::Exact(expected_version) },
                        events
                    )];
                    
                    let result = store.write_events_multi(stream_events).await.unwrap();
                    let new_version = result.get(&stream_id).unwrap();
                    
                    // Version should be exactly one more than the previous
                    let expected_next = EventVersion::try_new(i as u64 + 1).unwrap();
                    prop_assert_eq!(*new_version, expected_next);
                    
                    expected_version = *new_version;
                }
                
                // Read back all events and verify version sequence
                let stream_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                let events = stream_data.events;
                
                prop_assert_eq!(events.len(), event_count);
                
                // Verify each event has the correct version
                for (i, event) in events.iter().enumerate() {
                    let expected_version = EventVersion::try_new(i as u64 + 1).unwrap();
                    prop_assert_eq!(event.event_version, expected_version);
                }
                
                // Verify versions are strictly increasing
                for window in events.windows(2) {
                    prop_assert!(window[0].event_version < window[1].event_version);
                }
            });
        }
    }
}

/// Property test: Concurrent writes maintain version monotonicity.
///
/// This test verifies that even when multiple writes attempt to happen
/// concurrently, version monotonicity is preserved through optimistic
/// concurrency control.
#[test]
fn prop_concurrent_version_monotonicity() {
    proptest! {
        #[test]
        fn test_concurrent_version_consistency(
            stream_id in arb_stream_id(),
            initial_events in prop::collection::vec(any::<String>(), 1..5),
            concurrent_attempts in prop::collection::vec(any::<String>(), 2..10)
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = InMemoryEventStore::new();
                
                // Write initial events to establish a baseline
                let initial_event_writes: Vec<_> = initial_events.iter().enumerate().map(|(i, data)| {
                    EventToWrite::new(
                        EventId::new(),
                        VersionTestEvent {
                            sequence: i as u64,
                            data: data.clone(),
                        }
                    )
                }).collect();
                
                let initial_stream_events = vec![StreamEvents::new(
                    stream_id.clone(),
                    ExpectedVersion::New,
                    initial_event_writes
                )];
                
                let initial_result = store.write_events_multi(initial_stream_events).await.unwrap();
                let initial_version = *initial_result.get(&stream_id).unwrap();
                
                // Attempt concurrent writes (some should fail due to version conflicts)
                let mut handles = Vec::new();
                for (i, data) in concurrent_attempts.iter().enumerate() {
                    let store_clone = store.clone();
                    let stream_id_clone = stream_id.clone();
                    let data_clone = data.clone();
                    
                    let handle = tokio::spawn(async move {
                        let events = vec![EventToWrite::new(
                            EventId::new(),
                            VersionTestEvent {
                                sequence: 1000 + i as u64, // Use different sequence to track which succeeded
                                data: data_clone,
                            }
                        )];
                        
                        let stream_events = vec![StreamEvents::new(
                            stream_id_clone,
                            ExpectedVersion::Exact(initial_version),
                            events
                        )];
                        
                        store_clone.write_events_multi(stream_events).await
                    });
                    
                    handles.push(handle);
                }
                
                // Collect results - only one should succeed due to optimistic concurrency control
                let mut results = Vec::new();
                for handle in handles {
                    results.push(handle.await.unwrap());
                }
                
                let successful_writes: Vec<_> = results.iter().filter(|r| r.is_ok()).collect();
                
                // At most one concurrent write should succeed
                prop_assert!(successful_writes.len() <= 1);
                
                // Read final state and verify monotonicity
                let final_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                let final_events = final_data.events;
                
                // Verify monotonic version progression
                for window in final_events.windows(2) {
                    prop_assert!(window[0].event_version < window[1].event_version);
                }
                
                // Verify no version gaps
                for (i, event) in final_events.iter().enumerate() {
                    let expected_version = EventVersion::try_new(i as u64 + 1).unwrap();
                    prop_assert_eq!(event.event_version, expected_version);
                }
            });
        }
    }
}

/// Property test: Multi-stream writes maintain version monotonicity per stream.
///
/// This test verifies that when writing to multiple streams simultaneously,
/// each stream maintains its own monotonic version sequence.
#[test]
fn prop_multi_stream_version_monotonicity() {
    proptest! {
        #[test]
        fn test_multi_stream_version_consistency(
            stream_ids in prop::collection::vec(arb_stream_id(), 2..5),
            events_per_stream in 1usize..10
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = InMemoryEventStore::new();
                let mut stream_versions: HashMap<StreamId, EventVersion> = HashMap::new();
                
                // Initialize each stream with its own version tracking
                for stream_id in &stream_ids {
                    stream_versions.insert(stream_id.clone(), EventVersion::initial());
                }
                
                // Write multiple batches to all streams
                for batch in 0..events_per_stream {
                    let mut stream_events = Vec::new();
                    
                    for stream_id in &stream_ids {
                        let current_version = stream_versions[stream_id];
                        let events = vec![EventToWrite::new(
                            EventId::new(),
                            VersionTestEvent {
                                sequence: batch as u64,
                                data: format!("batch-{}-stream-{}", batch, stream_id.as_ref()),
                            }
                        )];
                        
                        let expected_version = if current_version == EventVersion::initial() {
                            ExpectedVersion::New
                        } else {
                            ExpectedVersion::Exact(current_version)
                        };
                        
                        stream_events.push(StreamEvents::new(
                            stream_id.clone(),
                            expected_version,
                            events
                        ));
                    }
                    
                    // Write to all streams atomically
                    let results = store.write_events_multi(stream_events).await.unwrap();
                    
                    // Update version tracking and verify monotonicity
                    for stream_id in &stream_ids {
                        let new_version = results.get(stream_id).unwrap();
                        let old_version = stream_versions[stream_id];
                        
                        // New version should be exactly one more than old version
                        let expected_new = if old_version == EventVersion::initial() {
                            EventVersion::try_new(1).unwrap()
                        } else {
                            EventVersion::try_new(u64::from(old_version) + 1).unwrap()
                        };
                        
                        prop_assert_eq!(*new_version, expected_new);
                        prop_assert!(new_version > &old_version);
                        
                        stream_versions.insert(stream_id.clone(), *new_version);
                    }
                }
                
                // Verify final state for all streams
                for stream_id in &stream_ids {
                    let stream_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                    let events = stream_data.events;
                    
                    prop_assert_eq!(events.len(), events_per_stream);
                    
                    // Verify version sequence for this stream
                    for (i, event) in events.iter().enumerate() {
                        let expected_version = EventVersion::try_new(i as u64 + 1).unwrap();
                        prop_assert_eq!(event.event_version, expected_version);
                    }
                    
                    // Verify monotonic ordering
                    for window in events.windows(2) {
                        prop_assert!(window[0].event_version < window[1].event_version);
                    }
                }
            });
        }
    }
}

/// Property test: Version validation prevents gaps and decreases.
///
/// This test verifies that the event store properly validates version
/// expectations and prevents invalid version sequences.
#[test]
fn prop_version_validation_prevents_invalid_sequences() {
    proptest! {
        #[test]
        fn test_version_validation(
            stream_id in arb_stream_id(),
            initial_count in 1usize..10,
            invalid_version_offset in -5i64..10i64
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let store = InMemoryEventStore::new();
                
                // Write initial events
                let initial_events: Vec<_> = (0..initial_count).map(|i| {
                    EventToWrite::new(
                        EventId::new(),
                        VersionTestEvent {
                            sequence: i as u64,
                            data: format!("initial-{}", i),
                        }
                    )
                }).collect();
                
                let initial_stream_events = vec![StreamEvents::new(
                    stream_id.clone(),
                    ExpectedVersion::New,
                    initial_events
                )];
                
                let result = store.write_events_multi(initial_stream_events).await.unwrap();
                let current_version = *result.get(&stream_id).unwrap();
                
                // Try to write with an invalid expected version
                let current_version_value = u64::from(current_version);
                let invalid_version_value = (current_version_value as i64 + invalid_version_offset) as u64;
                
                // Skip test if the invalid version would actually be valid
                if invalid_version_value == current_version_value + 1 || invalid_version_value == 0 {
                    return Ok(());
                }
                
                if let Ok(invalid_expected_version) = EventVersion::try_new(invalid_version_value) {
                    let invalid_events = vec![EventToWrite::new(
                        EventId::new(),
                        VersionTestEvent {
                            sequence: 9999,
                            data: "should-fail".to_string(),
                        }
                    )];
                    
                    let invalid_stream_events = vec![StreamEvents::new(
                        stream_id.clone(),
                        ExpectedVersion::Exact(invalid_expected_version),
                        invalid_events
                    )];
                    
                    // This write should fail due to version mismatch
                    let invalid_result = store.write_events_multi(invalid_stream_events).await;
                    
                    // Should get a concurrency error or similar
                    prop_assert!(invalid_result.is_err());
                    
                    // Verify the stream state is unchanged
                    let unchanged_data = store.read_streams(&[stream_id.clone()], &ReadOptions::new()).await.unwrap();
                    prop_assert_eq!(unchanged_data.events.len(), initial_count);
                    
                    // Verify the last event still has the correct version
                    if let Some(last_event) = unchanged_data.events.last() {
                        prop_assert_eq!(last_event.event_version, current_version);
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[tokio::test]
    async fn test_basic_version_monotonicity() {
        let store = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("test-stream").unwrap();
        
        // Write first event
        let events1 = vec![EventToWrite::new(
            EventId::new(),
            VersionTestEvent {
                sequence: 1,
                data: "first".to_string(),
            }
        )];
        
        let stream_events1 = vec![StreamEvents::new(
            stream_id.clone(),
            ExpectedVersion::New,
            events1
        )];
        
        let result1 = store.write_events_multi(stream_events1).await.unwrap();
        let version1 = *result1.get(&stream_id).unwrap();
        assert_eq!(version1, EventVersion::try_new(1).unwrap());
        
        // Write second event
        let events2 = vec![EventToWrite::new(
            EventId::new(),
            VersionTestEvent {
                sequence: 2,
                data: "second".to_string(),
            }
        )];
        
        let stream_events2 = vec![StreamEvents::new(
            stream_id.clone(),
            ExpectedVersion::Exact(version1),
            events2
        )];
        
        let result2 = store.write_events_multi(stream_events2).await.unwrap();
        let version2 = *result2.get(&stream_id).unwrap();
        assert_eq!(version2, EventVersion::try_new(2).unwrap());
        assert!(version2 > version1);
    }

    #[tokio::test]
    async fn test_version_conflict_detection() {
        let store = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("test-stream").unwrap();
        
        // Write initial event
        let events = vec![EventToWrite::new(
            EventId::new(),
            VersionTestEvent {
                sequence: 1,
                data: "initial".to_string(),
            }
        )];
        
        let stream_events = vec![StreamEvents::new(
            stream_id.clone(),
            ExpectedVersion::New,
            events
        )];
        
        let result = store.write_events_multi(stream_events).await.unwrap();
        let version = *result.get(&stream_id).unwrap();
        
        // Try to write with wrong expected version
        let invalid_events = vec![EventToWrite::new(
            EventId::new(),
            VersionTestEvent {
                sequence: 2,
                data: "should-fail".to_string(),
            }
        )];
        
        let invalid_version = EventVersion::try_new(999).unwrap();
        let invalid_stream_events = vec![StreamEvents::new(
            stream_id.clone(),
            ExpectedVersion::Exact(invalid_version),
            invalid_events
        )];
        
        let invalid_result = store.write_events_multi(invalid_stream_events).await;
        assert!(invalid_result.is_err());
        
        // Verify stream is unchanged
        let data = store.read_streams(&[stream_id], &ReadOptions::new()).await.unwrap();
        assert_eq!(data.events.len(), 1);
        assert_eq!(data.events[0].event_version, version);
    }
}