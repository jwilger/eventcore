//! Integration tests for PostgreSQL internal behavior.
//!
//! These tests verify low-level PostgreSQL functionality specific to our schema:
//! - Database trigger behavior for version assignment

mod common;

use common::PostgresTestFixture;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn trigger_assigns_sequential_versions() {
    // Given: A Postgres database with the eventcore schema
    let fixture = PostgresTestFixture::new().await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&fixture.connection_string)
        .await
        .expect("should connect to test database");

    let stream_id = format!("trigger-test-{}", Uuid::now_v7());

    // Set expected version via session config
    let config_query = format!(
        "SELECT set_config('eventcore.expected_versions', '{{\"{}\":0}}', true)",
        stream_id
    );
    sqlx::query(&config_query)
        .execute(&pool)
        .await
        .expect("should set expected versions");

    // When: Developer inserts first event directly into the events table
    let result = sqlx::query(
        "INSERT INTO eventcore_events (event_id, stream_id, event_type, event_data, metadata)
         VALUES ($1, $2, $3, $4, $5) RETURNING stream_version",
    )
    .bind(Uuid::now_v7())
    .bind(&stream_id)
    .bind("TestEvent")
    .bind(serde_json::json!({"n": 1}))
    .bind(serde_json::json!({}))
    .fetch_one(&pool)
    .await;

    // Then: The database trigger assigns version 1 to the first event
    match &result {
        Ok(row) => {
            let version: i64 = row.get("stream_version");
            assert_eq!(version, 1, "first event should have version 1");
        }
        Err(e) => panic!("insert failed: {}", e),
    }
}

#[tokio::test]
async fn trigger_prevents_update_on_event_log() {
    // Given: A Postgres database with events appended to a stream
    let fixture = PostgresTestFixture::new().await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&fixture.connection_string)
        .await
        .expect("should connect to test database");

    let stream_id = format!("immutability-test-{}", Uuid::now_v7());

    // Insert an event to have something to update
    sqlx::query(&format!(
        "SELECT set_config('eventcore.expected_versions', '{{\"{}\":0}}', true)",
        stream_id
    ))
    .execute(&pool)
    .await
    .expect("should set expected versions");

    let event_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO eventcore_events (event_id, stream_id, event_type, event_data, metadata)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(event_id)
    .bind(&stream_id)
    .bind("TestEvent")
    .bind(serde_json::json!({"original": true}))
    .bind(serde_json::json!({}))
    .execute(&pool)
    .await
    .expect("should insert event");

    // When: User attempts UPDATE on the events table via raw SQL
    let update_result =
        sqlx::query("UPDATE eventcore_events SET event_data = $1 WHERE event_id = $2")
            .bind(serde_json::json!({"tampered": true}))
            .bind(event_id)
            .execute(&pool)
            .await;

    // Then: Database raises error preventing the update
    assert!(
        update_result.is_err(),
        "UPDATE on event log should be prevented by database trigger"
    );
}

#[tokio::test]
async fn trigger_prevents_delete_on_event_log() {
    // Given: A Postgres database with events appended to a stream
    let fixture = PostgresTestFixture::new().await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&fixture.connection_string)
        .await
        .expect("should connect to test database");

    let stream_id = format!("delete-prevention-test-{}", Uuid::now_v7());

    // Insert an event to have something to delete
    sqlx::query(&format!(
        "SELECT set_config('eventcore.expected_versions', '{{\"{}\":0}}', true)",
        stream_id
    ))
    .execute(&pool)
    .await
    .expect("should set expected versions");

    let event_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO eventcore_events (event_id, stream_id, event_type, event_data, metadata)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(event_id)
    .bind(&stream_id)
    .bind("TestEvent")
    .bind(serde_json::json!({"data": "to be deleted"}))
    .bind(serde_json::json!({}))
    .execute(&pool)
    .await
    .expect("should insert event");

    // When: User attempts DELETE on the events table via raw SQL
    let delete_result = sqlx::query("DELETE FROM eventcore_events WHERE event_id = $1")
        .bind(event_id)
        .execute(&pool)
        .await;

    // Then: Database raises error preventing the deletion
    assert!(
        delete_result.is_err(),
        "DELETE on event log should be prevented by database trigger"
    );
}
