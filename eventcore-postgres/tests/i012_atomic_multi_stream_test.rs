mod common;

use common::{PostgresTestFixture, TestEvent, unique_stream_id};
use eventcore_types::{EventStore, StreamId, StreamVersion, StreamWrites};
use sqlx::postgres::PgPoolOptions;

#[tokio::test]
async fn developer_observes_atomic_multi_stream_commit() {
    // Given: a migrated Postgres store
    let fixture = PostgresTestFixture::new().await;
    let store = &fixture.store;

    // Use unique stream IDs for parallel test execution
    let source_stream = unique_stream_id("account/source");
    let destination_stream = unique_stream_id("account/dest");

    // And: a multi-stream write registering both accounts at version 0 with one event each
    let writes = build_multi_stream_writes(&source_stream, &destination_stream);

    store
        .append_events(writes)
        .await
        .expect("postgres store should append multi-stream batch");

    let committed_rows = count_rows_with_transaction(
        &fixture.connection_string,
        &source_stream,
        &destination_stream,
    )
    .await
    .expect("atomic verification should read committed rows inside a transaction");

    assert!(
        committed_rows == 2,
        "postgres multi-stream commit should persist two rows across streams; committed_rows={committed_rows}",
    );
}

#[tokio::test]
async fn append_events_persists_batch_metadata_for_each_row() {
    let fixture = PostgresTestFixture::new().await;
    let store = &fixture.store;
    let source_stream = unique_stream_id("account/source-metadata");
    let destination_stream = unique_stream_id("account/dest-metadata");
    let writes = build_multi_stream_writes(&source_stream, &destination_stream).with_metadata(
        serde_json::json!({
            "project_id": "018f17f2-44df-7cc7-86e0-8c2e6f6d8a57"
        }),
    );

    store
        .append_events(writes)
        .await
        .expect("postgres store should append multi-stream batch with metadata");

    let persisted_metadata = load_metadata(
        &fixture.connection_string,
        &source_stream,
        &destination_stream,
    )
    .await
    .expect("metadata query should succeed");

    assert_eq!(
        persisted_metadata,
        vec![
            serde_json::json!({
                "project_id": "018f17f2-44df-7cc7-86e0-8c2e6f6d8a57"
            }),
            serde_json::json!({
                "project_id": "018f17f2-44df-7cc7-86e0-8c2e6f6d8a57"
            }),
        ]
    );
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

async fn load_metadata(
    connection_string: &str,
    source_stream: &StreamId,
    destination_stream: &StreamId,
) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(connection_string)
        .await?;

    sqlx::query_scalar(
        "SELECT metadata
         FROM eventcore_events
         WHERE stream_id IN ($1, $2)
         ORDER BY stream_id ASC, stream_version ASC",
    )
    .bind(source_stream.as_ref())
    .bind(destination_stream.as_ref())
    .fetch_all(&pool)
    .await
}
