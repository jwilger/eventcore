use std::env;

use eventcore::{Event, EventStore, StreamId, StreamVersion, StreamWrites};
use eventcore_postgres::PostgresEventStore;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, postgres::PgPoolOptions};

fn postgres_connection_string() -> String {
    env::var("EVENTCORE_TEST_POSTGRES_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "postgres://postgres:postgres@localhost:5433/eventcore_test".to_string())
}

async fn clean_database(connection_string: &str) -> Result<(), sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(connection_string)
        .await?;

    pool.execute("DROP TABLE IF EXISTS eventcore_events CASCADE")
        .await?;

    pool.execute("DROP TABLE IF EXISTS _sqlx_migrations CASCADE")
        .await?;

    Ok(())
}

async fn make_store() -> PostgresEventStore {
    let connection_string = postgres_connection_string();

    clean_database(&connection_string)
        .await
        .expect("observability test should start from clean database");

    let store = PostgresEventStore::new(connection_string.clone())
        .await
        .expect("observability test should construct postgres event store");

    store
        .migrate()
        .await
        .expect("observability test migrations should succeed");

    store
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TestEvent {
    stream_id: StreamId,
    payload: String,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }
}

#[tokio::test]
#[tracing_test::traced_test]
async fn developer_observes_postgres_tracing_spans() {
    // Given: A migrated Postgres store instrumented with tracing spans
    let store = make_store().await;

    // And: A stream with a single event write
    let stream_id =
        StreamId::try_new("account-123").expect("valid stream id for observability test");
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
