use std::env;

use eventcore::{Event, EventStore, StreamId, StreamVersion, StreamWrites};
use eventcore_postgres::PostgresEventStore;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, postgres::PgPoolOptions};

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
async fn developer_observes_atomic_multi_stream_commit() {
    // Given: a migrated Postgres store starting from a clean slate
    let (store, connection_string) = make_store().await;

    let source_stream =
        StreamId::try_new("account/A").expect("source account stream id should be valid");
    let destination_stream =
        StreamId::try_new("account/B").expect("destination account stream id should be valid");

    // And: a multi-stream write registering both accounts at version 0 with one event each
    let writes = build_multi_stream_writes(&source_stream, &destination_stream);

    store
        .append_events(writes)
        .await
        .expect("postgres store should append multi-stream batch");

    let committed_rows =
        count_rows_with_transaction(&connection_string, &source_stream, &destination_stream)
            .await
            .expect("atomic verification should read committed rows inside a transaction");

    assert!(
        committed_rows == 2,
        "postgres multi-stream commit should persist two rows across streams; committed_rows={committed_rows}",
    );
}

fn postgres_connection_string() -> String {
    env::var("EVENTCORE_TEST_POSTGRES_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "postgres://postgres:postgres@localhost:5433/eventcore_test".to_string())
}

async fn make_store() -> (PostgresEventStore, String) {
    let connection_string = postgres_connection_string();

    clean_database(&connection_string)
        .await
        .expect("multi-stream test should start with clean database");

    let store = PostgresEventStore::new(connection_string.clone())
        .await
        .expect("multi-stream test should construct postgres store");

    store
        .migrate()
        .await
        .expect("multi-stream test migrations should succeed");

    (store, connection_string)
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

fn build_multi_stream_writes(
    source_stream: &StreamId,
    destination_stream: &StreamId,
) -> StreamWrites {
    StreamWrites::new()
        .register_stream(source_stream.clone(), StreamVersion::new(0))
        .and_then(|writes| {
            writes.register_stream(destination_stream.clone(), StreamVersion::new(0))
        })
        .and_then(|writes| {
            writes.append(TestEvent {
                stream_id: source_stream.clone(),
                payload: "credit source account".to_string(),
            })
        })
        .and_then(|writes| {
            writes.append(TestEvent {
                stream_id: destination_stream.clone(),
                payload: "debit destination account".to_string(),
            })
        })
        .expect("multi-stream writes should register both streams and append events")
}

async fn count_rows_with_transaction(
    connection_string: &str,
    source_stream: &StreamId,
    destination_stream: &StreamId,
) -> Result<i64, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(connection_string)
        .await?;

    let mut transaction = pool.begin().await?;

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM eventcore_events WHERE stream_id IN ($1, $2)")
            .bind(source_stream.as_ref())
            .bind(destination_stream.as_ref())
            .fetch_one(&mut *transaction)
            .await?;

    transaction.rollback().await?;

    Ok(count)
}
