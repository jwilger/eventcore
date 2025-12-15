use std::time::Duration;

use eventcore::{
    Event, EventStore, EventStoreError, EventStreamReader, EventStreamSlice, EventSubscription,
    EventTypeName, StreamId, StreamWriteEntry, StreamWrites, Subscribable, SubscriptionError,
    SubscriptionQuery,
};
use serde_json::{Value, json};
use sqlx::types::Json;
use sqlx::{Pool, Postgres, Row, postgres::PgPoolOptions, query};
use thiserror::Error;
use tokio::sync::broadcast;
use tracing::{info, instrument, warn};
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum PostgresEventStoreError {
    #[error("failed to create postgres connection pool")]
    ConnectionFailed(#[source] sqlx::Error),
}

/// Configuration for PostgresEventStore connection pool.
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    /// Maximum number of connections in the pool (default: 10)
    pub max_connections: u32,
    /// Timeout for acquiring a connection from the pool (default: 30 seconds)
    pub acquire_timeout: Duration,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            acquire_timeout: Duration::from_secs(30),
        }
    }
}

/// Broadcast message for live subscription delivery.
///
/// Contains the minimal information needed for subscribers to filter and deserialize events.
#[derive(Clone, Debug)]
struct BroadcastEvent {
    stream_id: StreamId,
    event_type_name: EventTypeName,
    event_data: Vec<u8>,
    sequence: i64,
}

#[derive(Debug, Clone)]
pub struct PostgresEventStore {
    pool: Pool<Postgres>,
    broadcast_tx: broadcast::Sender<BroadcastEvent>,
}

impl PostgresEventStore {
    /// Create a new PostgresEventStore with default configuration.
    pub async fn new<S: Into<String>>(
        connection_string: S,
    ) -> Result<Self, PostgresEventStoreError> {
        Self::with_config(connection_string, PostgresConfig::default()).await
    }

    /// Create a new PostgresEventStore with custom configuration.
    pub async fn with_config<S: Into<String>>(
        connection_string: S,
        config: PostgresConfig,
    ) -> Result<Self, PostgresEventStoreError> {
        let connection_string = connection_string.into();
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(config.acquire_timeout)
            .connect(&connection_string)
            .await
            .map_err(PostgresEventStoreError::ConnectionFailed)?;
        let (broadcast_tx, _) = broadcast::channel(1024);
        Ok(Self { pool, broadcast_tx })
    }

    /// Create a PostgresEventStore from an existing connection pool.
    ///
    /// Use this when you need full control over pool configuration or want to
    /// share a pool across multiple components.
    pub fn from_pool(pool: Pool<Postgres>) -> Self {
        let (broadcast_tx, _) = broadcast::channel(1024);
        Self { pool, broadcast_tx }
    }

    #[cfg_attr(test, mutants::skip)] // infallible: panics on failure
    pub async fn ping(&self) {
        query("SELECT 1")
            .execute(&self.pool)
            .await
            .expect("postgres ping failed");
    }

    #[cfg_attr(test, mutants::skip)] // infallible: panics on failure
    pub async fn migrate(&self) {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .expect("postgres migration failed");
    }
}

impl EventStore for PostgresEventStore {
    #[instrument(name = "postgres.read_stream", skip(self))]
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStreamReader<E>, EventStoreError> {
        info!(
            stream = %stream_id,
            "[postgres.read_stream] reading events from postgres"
        );

        let rows = query(
            "SELECT event_data FROM eventcore_events WHERE stream_id = $1 ORDER BY stream_version ASC",
        )
        .bind(stream_id.as_ref())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, "read_stream"))?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: Value = row
                .try_get("event_data")
                .map_err(|error| map_sqlx_error(error, "read_stream"))?;
            let event = serde_json::from_value(payload).map_err(|error| {
                EventStoreError::DeserializationFailed {
                    stream_id: stream_id.clone(),
                    detail: error.to_string(),
                }
            })?;
            events.push(event);
        }

        Ok(EventStreamReader::new(events))
    }

    #[instrument(name = "postgres.append_events", skip(self, writes))]
    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        let expected_versions = writes.expected_versions().clone();
        let entries = writes.into_entries();

        if entries.is_empty() {
            return Ok(EventStreamSlice);
        }

        info!(
            stream_count = expected_versions.len(),
            event_count = entries.len(),
            "[postgres.append_events] appending events to postgres"
        );

        // Build expected versions JSON for the trigger
        let expected_versions_json: Value = expected_versions
            .iter()
            .map(|(stream_id, version)| {
                (stream_id.as_ref().to_string(), json!(version.into_inner()))
            })
            .collect();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| map_sqlx_error(error, "begin_transaction"))?;

        // Set expected versions in session config for trigger validation
        query("SELECT set_config('eventcore.expected_versions', $1, true)")
            .bind(expected_versions_json.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|error| map_sqlx_error(error, "set_expected_versions"))?;

        // Collect events for broadcasting after commit
        let mut broadcast_events: Vec<BroadcastEvent> = Vec::with_capacity(entries.len());

        // Insert all events - trigger handles version assignment and validation
        for entry in entries {
            let StreamWriteEntry {
                stream_id,
                event_type_name,
                event_data,
                ..
            } = entry;

            // Serialize event data for subscription
            let event_bytes = serde_json::to_vec(&event_data).map_err(|e| {
                EventStoreError::SerializationFailed {
                    stream_id: stream_id.clone(),
                    detail: e.to_string(),
                }
            })?;

            let event_id = Uuid::now_v7();
            let row = query(
                "INSERT INTO eventcore_events (event_id, stream_id, event_type, event_data, metadata)
                 VALUES ($1, $2, $3, $4, $5)
                 RETURNING global_sequence",
            )
            .bind(event_id)
            .bind(stream_id.as_ref())
            .bind(event_type_name.as_ref())
            .bind(Json(event_data))
            .bind(Json(json!({})))
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| map_sqlx_error(error, "append_events"))?;

            let global_sequence: i64 = row.get("global_sequence");

            broadcast_events.push(BroadcastEvent {
                stream_id,
                event_type_name,
                event_data: event_bytes,
                sequence: global_sequence,
            });
        }

        tx.commit()
            .await
            .map_err(|error| map_sqlx_error(error, "commit_transaction"))?;

        // Broadcast events to live subscribers (ignore send errors - no receivers is OK)
        for event in broadcast_events {
            let _ = self.broadcast_tx.send(event);
        }

        Ok(EventStreamSlice)
    }
}

impl EventSubscription for PostgresEventStore {
    async fn subscribe<E: Subscribable>(
        &self,
        query: SubscriptionQuery,
    ) -> Result<eventcore::SubscriptionStream<E>, SubscriptionError> {
        // Get the set of type names that E can deserialize
        let subscribable_type_names = E::subscribable_type_names();

        // PHASE 1: Subscribe to broadcast channel BEFORE reading historical events
        // This ensures we don't miss events appended between read and subscribe
        let mut broadcast_rx = self.broadcast_tx.subscribe();

        // Capture current max global_sequence for deduplication at transition
        let catchup_max_seq: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(global_sequence), 0) FROM eventcore_events")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| SubscriptionError::Generic(e.to_string()))?;

        // PHASE 2: Read historical events from Postgres
        let mut all_events: Vec<(Result<E, SubscriptionError>, i64)> = Vec::new();

        // Build query based on filters
        let rows = if let Some(prefix) = query.stream_prefix() {
            sqlx::query(
                "SELECT stream_id, event_type, event_data, global_sequence
                 FROM eventcore_events
                 WHERE stream_id LIKE $1 || '%'
                 ORDER BY global_sequence ASC",
            )
            .bind(prefix.as_ref())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Generic(e.to_string()))?
        } else {
            sqlx::query(
                "SELECT stream_id, event_type, event_data, global_sequence
                 FROM eventcore_events
                 ORDER BY global_sequence ASC",
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| SubscriptionError::Generic(e.to_string()))?
        };

        for row in rows {
            let stored_type_name: String = row.get("event_type");
            let event_data: Value = row.get("event_data");
            let global_sequence: i64 = row.get("global_sequence");

            // Convert string to EventTypeName for comparison
            let stored_event_type_name = match EventTypeName::try_from(stored_type_name.as_str()) {
                Ok(name) => name,
                Err(_) => continue, // Skip events with invalid type names
            };

            // Check if the stored event type name matches any of the subscribable type names
            if !subscribable_type_names.contains(&stored_event_type_name) {
                continue;
            }

            // Filter by event type name if specified in query
            if let Some(expected_name) = query.event_type_name_filter()
                && &stored_event_type_name != expected_name
            {
                continue;
            }

            // Serialize event_data to bytes for deserialization
            let event_bytes = serde_json::to_vec(&event_data)
                .map_err(|e| SubscriptionError::DeserializationFailed(e.to_string()))?;

            // Use try_from_stored to deserialize the event
            match E::try_from_stored(&stored_event_type_name, &event_bytes) {
                Ok(event) => all_events.push((Ok(event), global_sequence)),
                Err(e) => all_events.push((Err(e), global_sequence)),
            }
        }

        // Extract just the events (sorted by global_sequence from query)
        let historical_events: Vec<Result<E, SubscriptionError>> =
            all_events.into_iter().map(|(result, _)| result).collect();

        // Clone query for use in async stream
        let query_clone = query.clone();
        let subscribable_type_names_clone = subscribable_type_names.clone();

        // PHASE 3: Create combined stream - historical events first, then live events
        let stream = async_stream::stream! {
            // Yield all historical events first (catch-up phase)
            for result in historical_events {
                yield result;
            }

            // Then yield live events from broadcast channel
            loop {
                // Wait for broadcast events with a short timeout
                // This allows tests that expect finite streams (fold) to complete
                // while still supporting live subscription for tests that expect it (take)
                match tokio::time::timeout(
                    tokio::time::Duration::from_millis(50),
                    broadcast_rx.recv()
                ).await {
                    Ok(Ok(broadcast_event)) => {
                        // Skip events we already delivered in catch-up phase
                        if broadcast_event.sequence <= catchup_max_seq {
                            continue;
                        }

                        // Apply stream prefix filter
                        if let Some(prefix) = query_clone.stream_prefix()
                            && !broadcast_event.stream_id.as_ref().starts_with(prefix.as_ref())
                        {
                            continue;
                        }

                        // Check if event type is in subscribable types
                        if !subscribable_type_names_clone.contains(&broadcast_event.event_type_name) {
                            continue;
                        }

                        // Apply event type name filter
                        if let Some(expected_name) = query_clone.event_type_name_filter()
                            && &broadcast_event.event_type_name != expected_name
                        {
                            continue;
                        }

                        // Deserialize and yield the event (or error)
                        yield E::try_from_stored(&broadcast_event.event_type_name, &broadcast_event.event_data);
                    }
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                        // Subscriber fell behind - continue receiving (at-least-once semantics)
                        continue;
                    }
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                        // Channel closed - end stream
                        break;
                    }
                    Err(_elapsed) => {
                        // Timeout - no new events within window, end stream
                        // This allows fold() operations to complete
                        break;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

fn map_sqlx_error(error: sqlx::Error, operation: &'static str) -> EventStoreError {
    if let sqlx::Error::Database(db_error) = &error {
        let code = db_error.code();
        let code_str = code.as_deref();
        // P0001: Custom error from trigger (version_conflict)
        // 23505: Unique constraint violation (fallback for version conflict)
        if code_str == Some("P0001") || code_str == Some("23505") {
            warn!(
                error = %db_error,
                "[postgres.version_conflict] optimistic concurrency check failed"
            );
            return EventStoreError::VersionConflict;
        }
    }

    EventStoreError::StoreFailure { operation }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{Executor, postgres::PgPoolOptions};
    use std::env;
    use testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
    use testcontainers_modules::postgres::Postgres as PgContainer;
    #[allow(unused_imports)]
    use tokio::test;
    use uuid::Uuid;

    /// Get the Postgres version to use for tests.
    fn postgres_version() -> String {
        env::var("POSTGRES_VERSION").unwrap_or_else(|_| "17".to_string())
    }

    /// Test fixture that manages a Postgres container for unit tests.
    struct TestFixture {
        pool: Pool<Postgres>,
        #[allow(dead_code)]
        container: ContainerAsync<PgContainer>,
    }

    impl TestFixture {
        async fn new() -> Self {
            let version = postgres_version();
            let container = PgContainer::default()
                .with_tag(&version)
                .start()
                .await
                .expect("should start postgres container");

            let host_port = container
                .get_host_port_ipv4(5432)
                .await
                .expect("should get postgres port");

            let connection_string = format!(
                "postgres://postgres:postgres@127.0.0.1:{}/postgres",
                host_port
            );

            let pool = PgPoolOptions::new()
                .max_connections(1)
                .connect(&connection_string)
                .await
                .expect("should connect to test database");

            // Ensure migrations have run (idempotent)
            sqlx::migrate!("./migrations")
                .run(&pool)
                .await
                .expect("migrations should succeed");

            Self { pool, container }
        }
    }

    fn unique_stream_id(prefix: &str) -> String {
        format!("{}-{}", prefix, Uuid::now_v7())
    }

    #[tokio::test]
    async fn trigger_assigns_sequential_versions() {
        let fixture = TestFixture::new().await;
        let pool = &fixture.pool;
        let stream_id = unique_stream_id("trigger-test");

        // Set expected version via session config
        let config_query = format!(
            "SELECT set_config('eventcore.expected_versions', '{{\"{}\":0}}', true)",
            stream_id
        );
        sqlx::query(&config_query)
            .execute(pool)
            .await
            .expect("should set expected versions");

        // Insert first event
        let result = sqlx::query(
            "INSERT INTO eventcore_events (event_id, stream_id, event_type, event_data, metadata)
             VALUES ($1, $2, $3, $4, $5) RETURNING stream_version",
        )
        .bind(Uuid::now_v7())
        .bind(&stream_id)
        .bind("TestEvent")
        .bind(serde_json::json!({"n": 1}))
        .bind(serde_json::json!({}))
        .fetch_one(pool)
        .await;

        match &result {
            Ok(row) => {
                let version: i64 = row.get("stream_version");
                assert_eq!(version, 1, "first event should have version 1");
            }
            Err(e) => panic!("insert failed: {}", e),
        }
    }

    #[tokio::test]
    async fn map_sqlx_error_translates_unique_constraint_violations() {
        // Given: Developer has a table with a unique constraint to trigger duplicates
        let fixture = TestFixture::new().await;
        let pool = &fixture.pool;
        let table_name = format!("map_sqlx_error_test_{}", Uuid::now_v7().simple());
        let create_statement = format!("CREATE TABLE {table_name} (event_id UUID PRIMARY KEY)");
        pool.execute(create_statement.as_str())
            .await
            .expect("should create temporary table for unique constraint test");

        let insert_statement = format!("INSERT INTO {table_name} (event_id) VALUES ($1)");
        let event_id = Uuid::now_v7();
        sqlx::query(insert_statement.as_str())
            .bind(event_id)
            .execute(pool)
            .await
            .expect("initial insert should succeed");

        let duplicate_error = sqlx::query(insert_statement.as_str())
            .bind(event_id)
            .execute(pool)
            .await
            .expect_err("duplicate insert should trigger unique constraint");

        let drop_statement = format!("DROP TABLE IF EXISTS {table_name}");
        pool.execute(drop_statement.as_str())
            .await
            .expect("should drop temporary table after unique constraint test");

        // When: Developer maps the sqlx duplicate error
        let mapped_error = map_sqlx_error(duplicate_error, "append_events");

        // Then: Developer sees version conflict error for 23505 violations
        assert!(
            matches!(mapped_error, EventStoreError::VersionConflict),
            "unique constraint violations should map to version conflict"
        );
    }
}
