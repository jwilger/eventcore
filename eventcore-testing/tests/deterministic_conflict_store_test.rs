use eventcore::{Event, StreamId};
use eventcore_memory::InMemoryEventStore;
use eventcore_testing::deterministic::DeterministicConflictStore;
use eventcore_types::{EventStore, StreamVersion, StreamWrites};
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
