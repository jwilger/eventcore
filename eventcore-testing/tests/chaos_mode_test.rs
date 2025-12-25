use eventcore::{
    Event, EventStore, EventStoreError, Operation, StreamId, StreamVersion, StreamWrites,
};
use eventcore_memory::InMemoryEventStore;
use eventcore_testing::chaos::{ChaosConfig, ChaosEventStoreExt};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestEvent {
    stream_id: StreamId,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }
}

#[tokio::test]
async fn chaos_mode_can_force_read_failure() {
    // Given: deterministic chaos forcing all read operations to fail
    let base_store = InMemoryEventStore::new();
    let chaos_store =
        base_store.with_chaos(ChaosConfig::deterministic().with_failure_probability(1.0));
    let stream_id = StreamId::try_new("chaos-read-stream").expect("valid stream id");

    // When: attempting to read from the chaos-enabled store
    let read_result = chaos_store.read_stream::<TestEvent>(stream_id).await;
    let error = match read_result {
        Ok(_) => panic!("expected chaos-enabled read to fail"),
        Err(err) => err,
    };

    // Then: the failure reason surfaces as a generic store failure
    assert_eq!(
        error,
        EventStoreError::StoreFailure {
            operation: Operation::ReadStream,
        }
    );
}

#[tokio::test]
async fn chaos_mode_can_force_version_conflict_on_write() {
    // Given: deterministic chaos forcing all writes to conflict on version checks
    let base_store = InMemoryEventStore::new();
    let chaos_store =
        base_store.with_chaos(ChaosConfig::deterministic().with_version_conflict_probability(1.0));
    let stream_id = StreamId::try_new("chaos-write-stream").expect("valid stream id");

    // And: a single-event write targeting the empty stream
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .and_then(|writes| {
            writes.append(TestEvent {
                stream_id: stream_id.clone(),
            })
        })
        .expect("writes builder should succeed");

    // When: attempting to append the events through the chaos-enabled store
    let append_result = chaos_store.append_events(writes).await;
    let error = match append_result {
        Ok(_) => panic!("expected chaos-enabled write to fail"),
        Err(err) => err,
    };

    // Then: the write is rejected as a version conflict
    assert_eq!(error, EventStoreError::VersionConflict);
}
