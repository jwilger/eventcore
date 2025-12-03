use std::env;

use eventcore::{Event, EventStore, StreamId, StreamVersion, StreamWrites};
use eventcore_postgres::PostgresEventStore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
async fn developer_migrates_metadata_column_without_data_loss() {
    let connection_string = postgres_connection_string();

    install_legacy_schema(&connection_string)
        .await
        .expect("legacy schema should be installed");
    seed_legacy_event(&connection_string)
        .await
        .expect("legacy event should be inserted");

    let store = PostgresEventStore::new(connection_string.clone())
        .await
        .expect("store should connect after legacy schema install");

    store
        .migrate()
        .await
        .expect("migrate should upgrade legacy schema");

    assert!(
        metadata_column_exists(&connection_string).await,
        "metadata column should exist after migration"
    );

    let pool = acquire_pool(&connection_string).await;
    let existing_metadata: Value = sqlx::query_scalar(
        "SELECT metadata FROM eventcore_events WHERE event_type = 'LegacyEvent' LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("existing row should be readable after migration");
    assert_eq!(existing_metadata, json!({}));

    let stream_id = default_stream_id();
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(1))
        .and_then(|writes| {
            writes.append(TestEvent {
                stream_id: stream_id.clone(),
                payload: "new event after migration".to_string(),
            })
        })
        .expect("writes should build after migration");

    store
        .append_events(writes)
        .await
        .expect("postgres store should accept appends after migration");

    let pool = acquire_pool(&connection_string).await;
    let row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM eventcore_events")
        .fetch_one(&pool)
        .await
        .expect("should count events after migration");
    assert_eq!(row_count, 2, "both legacy and new events should exist");
}

fn postgres_connection_string() -> String {
    env::var("EVENTCORE_TEST_POSTGRES_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "postgres://postgres:postgres@localhost:5433/eventcore_test".to_string())
}

async fn install_legacy_schema(connection_string: &str) -> Result<(), sqlx::Error> {
    let pool = acquire_pool(connection_string).await;

    pool.execute("DROP TABLE IF EXISTS eventcore_events CASCADE")
        .await?;
    pool.execute("DROP TABLE IF EXISTS _sqlx_migrations CASCADE")
        .await?;

    pool.execute(
        r#"
        CREATE TABLE eventcore_events (
            event_id UUID PRIMARY KEY,
            stream_id TEXT NOT NULL,
            stream_version BIGINT NOT NULL,
            event_type TEXT NOT NULL,
            event_data JSONB NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .await?;

    Ok(())
}

async fn seed_legacy_event(connection_string: &str) -> Result<(), sqlx::Error> {
    let pool = acquire_pool(connection_string).await;

    pool.execute(
        r#"
        INSERT INTO eventcore_events (event_id, stream_id, stream_version, event_type, event_data)
        VALUES (
            '00000000-0000-0000-0000-000000000001',
            'account/legacy',
            1,
            'LegacyEvent',
            '{"payload":"legacy"}'::jsonb
        )
        "#,
    )
    .await?;

    Ok(())
}

async fn metadata_column_exists(connection_string: &str) -> bool {
    let pool = acquire_pool(connection_string).await;

    let row = sqlx::query(
        r#"
        SELECT column_name
        FROM information_schema.columns
        WHERE table_name = 'eventcore_events'
          AND column_name = 'metadata'
        "#,
    )
    .fetch_optional(&pool)
    .await
    .expect("should query information schema for metadata column");

    row.is_some()
}

fn default_stream_id() -> StreamId {
    StreamId::try_new("account/legacy").expect("default stream id should be valid")
}

async fn acquire_pool(connection_string: &str) -> sqlx::Pool<sqlx::Postgres> {
    PgPoolOptions::new()
        .max_connections(1)
        .connect(connection_string)
        .await
        .expect("should connect to postgres")
}
