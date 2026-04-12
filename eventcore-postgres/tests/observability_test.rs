mod common;

use eventcore_types::{Event, EventStore, StreamId, StreamVersion, StreamWrites};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestEvent {
    stream_id: StreamId,
    payload: String,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "TestEvent"
    }
}

fn unique_stream_id(prefix: &str) -> StreamId {
    StreamId::try_new(format!("{}-{}", prefix, Uuid::now_v7())).expect("valid stream id")
}

#[tokio::test]
#[tracing_test::traced_test]
async fn developer_observes_postgres_tracing_spans() {
    // Given: A migrated Postgres store instrumented with tracing spans
    let store = common::create_test_store().await;

    // And: A stream with a single event write (unique per test run)
    let stream_id = unique_stream_id("observability-test");
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(0))
        .and_then(|writes| {
            writes.append(TestEvent {
                stream_id: stream_id.clone(),
                payload: "initial deposit".to_string(),
            })
        })
        .expect("should build stream writes for observability test");

    let _ = store
        .append_events(writes)
        .await
        .expect("postgres store should append events for observability test");

    // When: Developer reads the stream to exercise read spans
    let _events = store
        .read_stream::<TestEvent>(stream_id.clone())
        .await
        .expect("postgres store should read stream for observability test");

    // Then: Tracing spans are emitted for both append and read operations
    assert!(
        logs_contain("postgres.append_events") && logs_contain("postgres.read_stream"),
        "postgres adapter should emit append and read tracing spans",
    );
}
