//! Contract tests verifying EventStore StoreFailure operation names.
//!
//! This test documents the expected operation names for StoreFailure errors
//! using a strongly-typed Operation enum rather than stringly-typed values.
//!
//! The Operation enum should have variants matching EventStore trait methods:
//! - `Operation::ReadStream` for EventStore::read_stream failures
//! - `Operation::AppendEvents` for EventStore::append_events failures
//!
//! This ensures consistency across all EventStore implementations at compile time.

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
async fn store_failure_read_operation_uses_read_stream_variant() {
    // Given: A chaos store configured to always fail read operations
    let base_store = InMemoryEventStore::new();
    let chaos_store =
        base_store.with_chaos(ChaosConfig::deterministic().with_failure_probability(1.0));
    let stream_id = StreamId::try_new("operation-name-read-test").expect("valid stream id");

    // When: A read operation fails
    let result = chaos_store.read_stream::<TestEvent>(stream_id).await;
    let error = match result {
        Ok(_) => panic!("expected read operation to fail with StoreFailure"),
        Err(err) => err,
    };

    // Then: The operation is the strongly-typed ReadStream variant
    assert_eq!(
        error,
        EventStoreError::StoreFailure {
            operation: Operation::ReadStream,
        }
    );
}

#[tokio::test]
async fn store_failure_append_operation_uses_append_events_variant() {
    // Given: A chaos store configured to always fail append operations
    let base_store = InMemoryEventStore::new();
    let chaos_store =
        base_store.with_chaos(ChaosConfig::deterministic().with_failure_probability(1.0));
    let stream_id = StreamId::try_new("operation-name-append-test").expect("valid stream id");

    // And: A valid write batch
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .and_then(|writes| {
            writes.append(TestEvent {
                stream_id: stream_id.clone(),
            })
        })
        .expect("writes builder should succeed");

    // When: An append operation fails
    let result = chaos_store.append_events(writes).await;
    let error = match result {
        Ok(_) => panic!("expected append operation to fail with StoreFailure"),
        Err(err) => err,
    };

    // Then: The operation is the strongly-typed AppendEvents variant
    assert_eq!(
        error,
        EventStoreError::StoreFailure {
            operation: Operation::AppendEvents,
        }
    );
}
