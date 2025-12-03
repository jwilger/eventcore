use std::{env, sync::Arc};

use eventcore::{Event, EventStore, EventStoreError, StreamId, StreamVersion, StreamWrites};
use eventcore_postgres::PostgresEventStore;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, postgres::PgPoolOptions};
use tokio::sync::Barrier;

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
        .expect("concurrency test should start from clean database");

    let store = PostgresEventStore::new(connection_string.clone())
        .await
        .expect("concurrency test should construct postgres event store");

    store
        .migrate()
        .await
        .expect("concurrency test migrations should succeed");

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

fn build_single_stream_writes(
    stream_id: &StreamId,
    expected_version: StreamVersion,
    payload: &str,
) -> StreamWrites {
    StreamWrites::new()
        .register_stream(stream_id.clone(), expected_version)
        .and_then(|writes| {
            writes.append(TestEvent {
                stream_id: stream_id.clone(),
                payload: payload.to_string(),
            })
        })
        .expect("single stream writes should register stream and append event")
}

#[tokio::test]
#[tracing_test::traced_test]
async fn developer_retries_after_postgres_version_conflict() {
    // Given: A migrated Postgres store that enforces optimistic concurrency
    let store = make_store().await;
    let stream_id =
        StreamId::try_new("account/concurrency").expect("valid stream id for concurrency test");

    // And: Two separate writes targeting the same stream at version 0
    let first_writes =
        build_single_stream_writes(&stream_id, StreamVersion::new(0), "first deposit");
    let second_writes =
        build_single_stream_writes(&stream_id, StreamVersion::new(0), "second deposit");

    // When: Both writes attempt to append concurrently
    let barrier = Arc::new(Barrier::new(3));
    let first_store = store.clone();
    let first_barrier = barrier.clone();
    let first_handle = tokio::spawn(async move {
        first_barrier.wait().await;
        first_store.append_events(first_writes).await
    });

    let second_store = store.clone();
    let second_barrier = barrier.clone();
    let second_handle = tokio::spawn(async move {
        second_barrier.wait().await;
        second_store.append_events(second_writes).await
    });

    barrier.wait().await;

    let first_result = first_handle
        .await
        .expect("first append task should join without panic");
    let second_result = second_handle
        .await
        .expect("second append task should join without panic");

    let success_count = [&first_result, &second_result]
        .into_iter()
        .filter(|result| result.is_ok())
        .count();
    let conflict_count = [&first_result, &second_result]
        .into_iter()
        .filter(|result| matches!(result, Err(EventStoreError::VersionConflict)))
        .count();

    // And: Developer retries with expected version 1 after reloading state
    let retry_writes = build_single_stream_writes(
        &stream_id,
        StreamVersion::new(1),
        "retry deposit after conflict",
    );
    let retry_result = store.append_events(retry_writes).await;

    // Then: Exactly one write succeeds initially, the other reports conflict, retry succeeds, two events persist, and instrumentation logs conflict
    let read_result = store.read_stream::<TestEvent>(stream_id.clone()).await;
    let total_events = read_result
        .as_ref()
        .map(|reader| reader.len())
        .unwrap_or_default();
    let logs_contain_conflict = logs_contain("postgres.version_conflict");
    let retry_succeeded = retry_result.is_ok();
    let retry_state = match &retry_result {
        Ok(_) => "ok".to_string(),
        Err(err) => err.to_string(),
    };

    assert!(
        success_count == 1
            && conflict_count == 1
            && retry_succeeded
            && read_result.is_ok()
            && total_events == 2
            && logs_contain_conflict,
        "postgres store should surface version conflict, allow retry, persist events, and emit instrumentation; success_count={success_count}, conflict_count={conflict_count}, retry_state={retry_state}, total_events={total_events}, logs_contain_conflict={logs_contain_conflict}",
    );
}
