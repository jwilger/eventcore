use std::sync::{Arc, Barrier};

use eventcore::{Event, StreamId};
use eventcore_memory::InMemoryEventStore;
use eventcore_testing::deterministic::DeterministicConflictStore;
use eventcore_types::{EventStore, EventStoreError, StreamVersion, StreamWrites};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestEvent {
    stream_id: StreamId,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "TestEvent"
    }
}

#[tokio::test]
async fn zero_conflicts_delegates_to_inner_store_immediately() {
    // Given: DeterministicConflictStore with 0 conflicts
    let inner_store = InMemoryEventStore::new();
    let store = DeterministicConflictStore::new(inner_store, 0);
    let stream_id = StreamId::try_new("test-stream").expect("valid stream id");

    // And: a single-event write
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .and_then(|writes| {
            writes.append(TestEvent {
                stream_id: stream_id.clone(),
            })
        })
        .expect("writes builder should succeed");

    // When: append_events is called
    let result = store.append_events(writes).await;

    // Then: it delegates to inner store immediately (no error)
    assert!(result.is_ok());
}

#[test]
fn concurrent_appends_inject_exactly_the_configured_conflicts_then_delegate() {
    // Given: simultaneous independent writes competing for seven injected conflicts.
    let store = Arc::new(DeterministicConflictStore::new(
        InMemoryEventStore::new(),
        7,
    ));
    let barrier = Arc::new(Barrier::new(32));
    let workers: Vec<_> = (0..32)
        .map(|index| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let stream_id = StreamId::try_new(format!("concurrent-stream-{index}"))
                    .expect("valid stream id");
                let writes = StreamWrites::new()
                    .register_stream(stream_id.clone(), StreamVersion::new(0))
                    .and_then(|writes| writes.append(TestEvent { stream_id }))
                    .expect("valid writes");

                // When: every worker appends after the same barrier.
                let _ = barrier.wait();
                futures::executor::block_on(store.append_events(writes))
            })
        })
        .collect();

    // Then: exactly seven synthetic conflicts occur; all other writes reach the store.
    let mut conflicts = 0;
    for worker in workers {
        match worker.join().expect("worker succeeds") {
            Ok(_) => {}
            Err(EventStoreError::VersionConflict { stream_id, .. }) => {
                assert_eq!(
                    stream_id,
                    StreamId::try_new("deterministic-conflict").expect("valid stream id")
                );
                conflicts += 1;
            }
            Err(error) => panic!("unexpected store error: {error}"),
        }
    }
    assert_eq!(conflicts, 7);

    // And: exhausted conflicts stay at zero on a subsequent append.
    let stream_id = StreamId::try_new("after-contention").expect("valid stream id");
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .and_then(|writes| writes.append(TestEvent { stream_id }))
        .expect("valid writes");
    assert!(futures::executor::block_on(store.append_events(writes)).is_ok());
}
