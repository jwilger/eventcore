mod common;

use common::{PostgresTestFixture, TestEvent, unique_stream_id};
use eventcore::{EventStore, StreamId, StreamVersion, StreamWrites};
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
