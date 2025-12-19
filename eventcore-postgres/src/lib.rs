use std::time::Duration;

use eventcore::{
    Event, EventStore, EventStoreError, EventStreamReader, EventStreamSlice, Operation, StreamId,
    StreamWriteEntry, StreamWrites,
};
use serde_json::{Value, json};
use sqlx::types::Json;
use sqlx::{Pool, Postgres, Row, postgres::PgPoolOptions, query};
use thiserror::Error;
use tracing::{error, info, instrument, warn};
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
    /// Idle timeout for connections in the pool (default: 10 minutes)
    pub idle_timeout: Duration,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            acquire_timeout: Duration::from_secs(30),
            idle_timeout: Duration::from_secs(600), // 10 minutes
        }
    }
}

#[derive(Debug, Clone)]
pub struct PostgresEventStore {
    pool: Pool<Postgres>,
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
            .idle_timeout(config.idle_timeout)
            .connect(&connection_string)
            .await
            .map_err(PostgresEventStoreError::ConnectionFailed)?;
        Ok(Self { pool })
    }

    /// Create a PostgresEventStore from an existing connection pool.
    ///
    /// Use this when you need full control over pool configuration or want to
    /// share a pool across multiple components.
    pub fn from_pool(pool: Pool<Postgres>) -> Self {
        Self { pool }
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
        .map_err(|error| map_sqlx_error(error, Operation::ReadStream))?;

        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: Value = row
                .try_get("event_data")
                .map_err(|error| map_sqlx_error(error, Operation::ReadStream))?;
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
            .map_err(|error| map_sqlx_error(error, Operation::BeginTransaction))?;

        // Set expected versions in session config for trigger validation
        query("SELECT set_config('eventcore.expected_versions', $1, true)")
            .bind(expected_versions_json.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|error| map_sqlx_error(error, Operation::SetExpectedVersions))?;

        // Insert all events - trigger handles version assignment and validation
        for entry in entries {
            let StreamWriteEntry {
                stream_id,
                event_type,
                event_data,
                ..
            } = entry;

            let event_id = Uuid::now_v7();
            query(
                "INSERT INTO eventcore_events (event_id, stream_id, event_type, event_data, metadata)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(event_id)
            .bind(stream_id.as_ref())
            .bind(event_type)
            .bind(Json(event_data))
            .bind(Json(json!({})))
            .execute(&mut *tx)
            .await
            .map_err(|error| map_sqlx_error(error, Operation::AppendEvents))?;
        }

        tx.commit()
            .await
            .map_err(|error| map_sqlx_error(error, Operation::CommitTransaction))?;

        Ok(EventStreamSlice)
    }
}

fn map_sqlx_error(error: sqlx::Error, operation: Operation) -> EventStoreError {
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

    error!(
        error = %error,
        operation = %operation,
        "[postgres.database_error] database operation failed"
    );
    EventStoreError::StoreFailure { operation }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{Executor, postgres::PgPoolOptions};
    use std::env;
    use std::sync::OnceLock;
    use testcontainers::{Container, ImageExt, ReuseDirective, runners::SyncRunner};
    use testcontainers_modules::postgres::Postgres as PgContainer;
    #[allow(unused_imports)]
    use tokio::test;
    use uuid::Uuid;

    /// Container name for the shared reusable Postgres instance.
    const CONTAINER_NAME: &str = "eventcore-test-postgres";

    /// Shared container and connection string for all unit tests.
    /// The container persists between test runs for faster iteration.
    static SHARED_CONTAINER: OnceLock<SharedPostgres> = OnceLock::new();

    struct SharedPostgres {
        connection_string: String,
        #[allow(dead_code)]
        container: Container<PgContainer>,
    }

    /// Get the Postgres version to use for tests.
    fn postgres_version() -> String {
        env::var("POSTGRES_VERSION").unwrap_or_else(|_| "17".to_string())
    }

    /// Start a reusable container with retry logic for cross-process races.
    ///
    /// When nextest runs test binaries in parallel, multiple processes may try to
    /// create the same named container simultaneously. This retries on "name already
    /// in use" errors, allowing the other process to finish creation.
    fn start_container_with_retry() -> Container<PgContainer> {
        let version = postgres_version();
        let max_retries = 10;
        let retry_delay = std::time::Duration::from_millis(500);

        for attempt in 0..max_retries {
            match PgContainer::default()
                .with_tag(&version)
                .with_container_name(CONTAINER_NAME)
                .with_reuse(ReuseDirective::Always)
                .start()
            {
                Ok(container) => return container,
                Err(e) => {
                    let error_str = e.to_string();
                    if error_str.contains("already in use") && attempt < max_retries - 1 {
                        // Another process is creating the container, wait and retry
                        std::thread::sleep(retry_delay);
                        continue;
                    }
                    panic!("should start postgres container: {}", e);
                }
            }
        }
        panic!(
            "failed to start postgres container after {} retries",
            max_retries
        );
    }

    fn get_shared_postgres() -> &'static SharedPostgres {
        SHARED_CONTAINER.get_or_init(|| {
            // Run container setup in a separate thread to avoid tokio runtime conflicts
            std::thread::spawn(|| {
                let container = start_container_with_retry();

                let host_port = container
                    .get_host_port_ipv4(5432)
                    .expect("should get postgres port");

                let connection_string = format!(
                    "postgres://postgres:postgres@127.0.0.1:{}/postgres",
                    host_port
                );

                // Run migrations using a temporary runtime
                // Retry connection in case postgres is still starting up
                let rt = tokio::runtime::Runtime::new()
                    .expect("should create tokio runtime for migrations");
                rt.block_on(async {
                    let max_conn_retries = 30;
                    let conn_retry_delay = std::time::Duration::from_millis(500);
                    let mut pool = None;

                    for attempt in 0..max_conn_retries {
                        match PgPoolOptions::new()
                            .max_connections(1)
                            .connect(&connection_string)
                            .await
                        {
                            Ok(p) => {
                                pool = Some(p);
                                break;
                            }
                            Err(e) => {
                                if attempt < max_conn_retries - 1 {
                                    tokio::time::sleep(conn_retry_delay).await;
                                    continue;
                                }
                                panic!(
                                    "should connect to test database after {} retries: {}",
                                    max_conn_retries, e
                                );
                            }
                        }
                    }

                    let pool = pool.expect("pool should be set");
                    sqlx::migrate!("./migrations")
                        .run(&pool)
                        .await
                        .expect("migrations should succeed");
                });

                SharedPostgres {
                    connection_string,
                    container,
                }
            })
            .join()
            .expect("container setup thread should complete")
        })
    }

    async fn get_test_pool() -> Pool<Postgres> {
        let shared = get_shared_postgres();
        PgPoolOptions::new()
            .max_connections(1)
            .connect(&shared.connection_string)
            .await
            .expect("should connect to shared postgres container")
    }

    fn unique_stream_id(prefix: &str) -> String {
        format!("{}-{}", prefix, Uuid::now_v7())
    }

    #[tokio::test]
    async fn trigger_assigns_sequential_versions() {
        let pool = get_test_pool().await;
        let stream_id = unique_stream_id("trigger-test");

        // Set expected version via session config
        let config_query = format!(
            "SELECT set_config('eventcore.expected_versions', '{{\"{}\":0}}', true)",
            stream_id
        );
        sqlx::query(&config_query)
            .execute(&pool)
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
        .fetch_one(&pool)
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
        let pool = get_test_pool().await;
        let table_name = format!("map_sqlx_error_test_{}", Uuid::now_v7().simple());
        let create_statement = format!("CREATE TABLE {table_name} (event_id UUID PRIMARY KEY)");
        pool.execute(create_statement.as_str())
            .await
            .expect("should create temporary table for unique constraint test");

        let insert_statement = format!("INSERT INTO {table_name} (event_id) VALUES ($1)");
        let event_id = Uuid::now_v7();
        sqlx::query(insert_statement.as_str())
            .bind(event_id)
            .execute(&pool)
            .await
            .expect("initial insert should succeed");

        let duplicate_error = sqlx::query(insert_statement.as_str())
            .bind(event_id)
            .execute(&pool)
            .await
            .expect_err("duplicate insert should trigger unique constraint");

        let drop_statement = format!("DROP TABLE IF EXISTS {table_name}");
        pool.execute(drop_statement.as_str())
            .await
            .expect("should drop temporary table after unique constraint test");

        // When: Developer maps the sqlx duplicate error
        let mapped_error = map_sqlx_error(duplicate_error, Operation::AppendEvents);

        // Then: Developer sees version conflict error for 23505 violations
        assert!(
            matches!(mapped_error, EventStoreError::VersionConflict),
            "unique constraint violations should map to version conflict"
        );
    }
}
