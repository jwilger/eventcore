//! Integration tests for PostgreSQL internal behavior.
//!
//! These tests verify low-level PostgreSQL functionality specific to our schema:
//! - Database trigger behavior for version assignment
//! - Advisory lock behavior for coordination

mod common;

use eventcore_postgres::PostgresProjectorCoordinator;
use eventcore_types::{ProjectorCoordinator, StreamPosition};
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn trigger_assigns_sequential_versions() {
    // Given: A Postgres database with the eventcore schema
    let _store = common::create_test_store().await;
    let connection_string = common::connection_string();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&connection_string)
        .await
        .expect("should connect to test database");

    let stream_id = format!("trigger-test-{}", Uuid::now_v7());

    // Set expected version via session config
    let config_query = format!(
        "SELECT set_config('eventcore.expected_versions', '{{\"{}\":0}}', true)",
        stream_id
    );
    let _ = sqlx::query(&config_query)
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
    let _store = common::create_test_store().await;
    let connection_string = common::connection_string();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&connection_string)
        .await
        .expect("should connect to test database");

    let stream_id = format!("immutability-test-{}", Uuid::now_v7());

    // Insert an event to have something to update
    let _ = sqlx::query(&format!(
        "SELECT set_config('eventcore.expected_versions', '{{\"{}\":0}}', true)",
        stream_id
    ))
    .execute(&pool)
    .await
    .expect("should set expected versions");

    let event_id = Uuid::now_v7();
    let _ = sqlx::query(
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

    // Then: Database raises error preventing the update with clear message
    let error =
        update_result.expect_err("UPDATE on event log should be prevented by database trigger");
    let error_message = error.to_string();
    assert!(
        error_message.contains("immutable"),
        "Error message should clearly indicate immutability violation, got: {}",
        error_message
    );
}

#[tokio::test]
async fn trigger_prevents_delete_on_event_log() {
    // Given: A Postgres database with events appended to a stream
    let _store = common::create_test_store().await;
    let connection_string = common::connection_string();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&connection_string)
        .await
        .expect("should connect to test database");

    let stream_id = format!("delete-prevention-test-{}", Uuid::now_v7());

    // Insert an event to have something to delete
    let _ = sqlx::query(&format!(
        "SELECT set_config('eventcore.expected_versions', '{{\"{}\":0}}', true)",
        stream_id
    ))
    .execute(&pool)
    .await
    .expect("should set expected versions");

    let event_id = Uuid::now_v7();
    let _ = sqlx::query(
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

    // Then: Database raises error preventing the deletion with clear message
    let error =
        delete_result.expect_err("DELETE on event log should be prevented by database trigger");
    let error_message = error.to_string();
    assert!(
        error_message.contains("immutable"),
        "Error message should clearly indicate immutability violation, got: {}",
        error_message
    );
}

/// Helper to compute advisory lock key from subscription name (same FNV-1a algorithm as production code)
fn compute_lock_key(subscription_name: &str) -> i64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in subscription_name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash as i64
}

/// Helper to check if an advisory lock is held by querying pg_locks
async fn is_advisory_lock_held(pool: &sqlx::Pool<sqlx::Postgres>, lock_key: i64) -> bool {
    let result = sqlx::query(
        "SELECT COUNT(*) as count FROM pg_locks
         WHERE locktype = 'advisory' AND objid = $1 AND granted = true",
    )
    .bind(lock_key as i32) // objid is int4
    .fetch_one(pool)
    .await
    .expect("should query pg_locks");

    let count: i64 = result.get("count");
    count > 0
}

#[tokio::test(flavor = "multi_thread")]
async fn advisory_lock_released_on_guard_drop_verifies_pg_locks() {
    let _store = common::create_test_store().await;
    let connection_string = common::connection_string();

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("should connect to test database");

    let coordinator = PostgresProjectorCoordinator::from_pool(pool.clone());
    let subscription_name = format!("pg-locks-test-{}", Uuid::now_v7());
    let lock_key = compute_lock_key(&subscription_name);

    // When: We acquire leadership
    let guard = coordinator
        .try_acquire(&subscription_name)
        .await
        .expect("should acquire leadership");

    // Then: The advisory lock should be visible in pg_locks
    assert!(
        is_advisory_lock_held(&pool, lock_key).await,
        "advisory lock should be held after acquiring leadership"
    );

    // Force pool churn
    for _ in 0..10 {
        let _conn = pool.acquire().await.expect("should acquire connection");
    }

    // When: We drop the guard (releasing leadership)
    drop(guard);

    // Then: The advisory lock should no longer be visible in pg_locks
    assert!(
        !is_advisory_lock_held(&pool, lock_key).await,
        "advisory lock should be released after dropping guard"
    );
}

#[tokio::test]
async fn postgres_event_store_saves_and_loads_checkpoint() {
    use eventcore_types::CheckpointStore;
    let store = common::create_test_store().await;

    let position = StreamPosition::new(Uuid::now_v7());
    let subscription_name = format!("event-store-checkpoint-test-{}", Uuid::now_v7());
    store
        .save(&subscription_name, position)
        .await
        .expect("should save checkpoint");

    let loaded = store
        .load(&subscription_name)
        .await
        .expect("should load checkpoint");
    assert_eq!(loaded, Some(position));
}

#[tokio::test(flavor = "multi_thread")]
async fn advisory_lock_held_blocks_second_acquisition_verified_via_pg_locks() {
    let _store = common::create_test_store().await;
    let connection_string = common::connection_string();

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection_string)
        .await
        .expect("should connect to test database");

    let coordinator = PostgresProjectorCoordinator::from_pool(pool.clone());
    let subscription_name = format!("pg-locks-blocking-test-{}", Uuid::now_v7());
    let lock_key = compute_lock_key(&subscription_name);

    assert!(
        !is_advisory_lock_held(&pool, lock_key).await,
        "no advisory lock should be held initially"
    );

    let guard = coordinator
        .try_acquire(&subscription_name)
        .await
        .expect("should acquire leadership");

    assert!(
        is_advisory_lock_held(&pool, lock_key).await,
        "advisory lock should be held after first acquisition"
    );

    let second_result = coordinator.try_acquire(&subscription_name).await;
    assert!(
        second_result.is_err(),
        "second acquisition should fail while lock is held"
    );

    drop(guard);

    assert!(
        !is_advisory_lock_held(&pool, lock_key).await,
        "advisory lock should be released after guard drop"
    );

    let third_guard = coordinator
        .try_acquire(&subscription_name)
        .await
        .expect("should acquire leadership after previous guard dropped");

    drop(third_guard);
}

#[tokio::test]
async fn postgres_event_store_implements_projector_coordinator() {
    use eventcore_types::ProjectorCoordinator;
    let store = common::create_test_store().await;

    let guard = store.try_acquire("test-projector").await;
    assert!(guard.is_ok(), "should acquire leadership");
}
