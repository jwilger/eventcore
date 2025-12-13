mod common;

use common::{PostgresTestFixture, TestEvent, unique_stream_id};
use eventcore::{EventStore, StreamVersion, StreamWrites};

#[tokio::test]
#[tracing_test::traced_test]
async fn developer_observes_postgres_tracing_spans() {
    // Given: A migrated Postgres store instrumented with tracing spans
    let fixture = PostgresTestFixture::new().await;
    let store = &fixture.store;

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

    store
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
