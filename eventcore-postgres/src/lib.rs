use std::time::Duration;

use eventcore_types::{
    Event, EventFilter, EventPage, EventReader, EventStore, EventStoreError, EventStreamReader,
    EventStreamSlice, Operation, StreamId, StreamPosition, StreamWriteEntry, StreamWrites,
};
use nutype::nutype;
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

/// Maximum number of database connections in the pool.
///
/// MaxConnections represents the connection pool size limit. It must be at least 1,
/// enforced by using NonZeroU32 as the underlying type.
///
/// # Examples
///
/// ```ignore
/// use eventcore_postgres::MaxConnections;
/// use std::num::NonZeroU32;
///
/// let small_pool = MaxConnections::new(NonZeroU32::new(5).expect("5 is non-zero"));
/// let standard = MaxConnections::new(NonZeroU32::new(10).expect("10 is non-zero"));
/// let large_pool = MaxConnections::new(NonZeroU32::new(50).expect("50 is non-zero"));
///
/// // Zero connections not allowed by type system
/// // let zero = NonZeroU32::new(0); // Returns None
/// ```
#[nutype(derive(Debug, Clone, Copy, PartialEq, Eq, Display, AsRef, Into))]
pub struct MaxConnections(std::num::NonZeroU32);

/// Configuration for PostgresEventStore connection pool.
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    /// Maximum number of connections in the pool (default: 10)
    pub max_connections: MaxConnections,
    /// Timeout for acquiring a connection from the pool (default: 30 seconds)
    pub acquire_timeout: Duration,
    /// Idle timeout for connections in the pool (default: 10 minutes)
    pub idle_timeout: Duration,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        const DEFAULT_MAX_CONNECTIONS: std::num::NonZeroU32 = match std::num::NonZeroU32::new(10) {
            Some(v) => v,
            None => unreachable!(),
        };

        Self {
            max_connections: MaxConnections::new(DEFAULT_MAX_CONNECTIONS),
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
        let max_connections: std::num::NonZeroU32 = config.max_connections.into();
        let pool = PgPoolOptions::new()
            .max_connections(max_connections.get())
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

impl EventReader for PostgresEventStore {
    type Error = EventStoreError;

    async fn read_events<E: Event>(
        &self,
        filter: EventFilter,
        page: EventPage,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        // Query events ordered by event_id (UUID7, monotonically increasing).
        // Use event_id directly as the global position - no need for ROW_NUMBER.
        let after_event_id: Option<Uuid> = page.after_position().map(|p| p.into_inner());
        let limit: i64 = page.limit().into_inner() as i64;

        let rows = if let Some(prefix) = filter.stream_prefix() {
            let prefix_str = prefix.as_ref();

            if let Some(after_id) = after_event_id {
                let query_str = r#"
                    SELECT event_id, event_data, stream_id
                    FROM eventcore_events
                    WHERE event_id > $1
                      AND stream_id LIKE $2 || '%'
                    ORDER BY event_id
                    LIMIT $3
                "#;
                query(query_str)
                    .bind(after_id)
                    .bind(prefix_str)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await
            } else {
                let query_str = r#"
                    SELECT event_id, event_data, stream_id
                    FROM eventcore_events
                    WHERE stream_id LIKE $1 || '%'
                    ORDER BY event_id
                    LIMIT $2
                "#;
                query(query_str)
                    .bind(prefix_str)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await
            }
        } else if let Some(after_id) = after_event_id {
            let query_str = r#"
                SELECT event_id, event_data, stream_id
                FROM eventcore_events
                WHERE event_id > $1
                ORDER BY event_id
                LIMIT $2
            "#;
            query(query_str)
                .bind(after_id)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
        } else {
            let query_str = r#"
                SELECT event_id, event_data, stream_id
                FROM eventcore_events
                ORDER BY event_id
                LIMIT $1
            "#;
            query(query_str).bind(limit).fetch_all(&self.pool).await
        }
        .map_err(|error| map_sqlx_error(error, Operation::ReadStream))?;

        let events: Vec<(E, StreamPosition)> = rows
            .into_iter()
            .filter_map(|row| {
                let event_data: Json<Value> = row.get("event_data");
                let event_id: Uuid = row.get("event_id");
                serde_json::from_value::<E>(event_data.0)
                    .ok()
                    .map(|e| (e, StreamPosition::new(event_id)))
            })
            .collect();

        Ok(events)
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

/// Error type for PostgresCoordinator operations.
#[derive(Debug, Error)]
pub enum PostgresCoordinatorError {
    /// Failed to establish database connection for coordination.
    #[error("failed to create postgres connection for coordinator")]
    ConnectionFailed(#[source] sqlx::Error),
}

/// PostgreSQL-based distributed coordinator for projection leadership.
///
/// Uses PostgreSQL advisory locks to coordinate leadership among multiple instances.
/// Only one instance can hold leadership at a time. The guard becomes invalid after
/// the heartbeat timeout expires without calling `heartbeat()`.
///
/// # RAII Pattern
///
/// The returned `PostgresCoordinatorGuard` implements RAII - leadership is automatically
/// released when the guard is dropped.
///
/// # Database Requirements
///
/// Requires a PostgreSQL database connection for distributed coordination.
/// Each coordinator instance maintains its own connection pool for advisory locks.
pub struct PostgresCoordinator {
    connection_string: String,
    heartbeat_timeout: Duration,
}

/// Fixed advisory lock ID for coordinator leadership.
/// Future versions can hash projector name for multiple coordinators.
const COORDINATOR_LOCK_ID: i64 = 314159265;

impl PostgresCoordinator {
    /// Creates a new PostgresCoordinator with the given connection string and heartbeat timeout.
    ///
    /// # Parameters
    ///
    /// - `connection_string`: PostgreSQL connection string (e.g., "postgres://user:pass@host/db")
    /// - `heartbeat_timeout`: Duration after which a guard becomes invalid without heartbeat
    ///
    /// # Errors
    ///
    /// Returns `PostgresCoordinatorError::ConnectionFailed` if the connection cannot be established.
    pub fn new(
        connection_string: String,
        heartbeat_timeout: Duration,
    ) -> Result<Self, PostgresCoordinatorError> {
        Ok(Self {
            connection_string,
            heartbeat_timeout,
        })
    }

    /// Attempts to acquire leadership, returning a guard if successful.
    ///
    /// Returns `None` if another instance currently holds leadership.
    /// Returns `Some(guard)` if leadership was acquired successfully.
    ///
    /// This is an async method because it requires database communication.
    pub async fn try_acquire(&self) -> Option<PostgresCoordinatorGuard> {
        use sqlx::Row;
        use sqlx::postgres::PgPoolOptions;

        // Create dedicated connection pool for this guard
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(&self.connection_string)
            .await
            .ok()?;

        // Try to acquire advisory lock
        let acquired: bool = sqlx::query("SELECT pg_try_advisory_lock($1)")
            .bind(COORDINATOR_LOCK_ID)
            .fetch_one(&pool)
            .await
            .ok()
            .and_then(|row| row.try_get(0).ok())?;

        if !acquired {
            return None;
        }

        // Create heartbeat table if not exists
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS coordinator_heartbeat (
                lock_id BIGINT PRIMARY KEY,
                last_heartbeat TIMESTAMPTZ NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .ok()?;

        // Insert initial heartbeat timestamp
        sqlx::query(
            r#"
            INSERT INTO coordinator_heartbeat (lock_id, last_heartbeat)
            VALUES ($1, NOW())
            ON CONFLICT (lock_id)
            DO UPDATE SET last_heartbeat = NOW()
            "#,
        )
        .bind(COORDINATOR_LOCK_ID)
        .execute(&pool)
        .await
        .ok()?;

        Some(PostgresCoordinatorGuard {
            pool,
            heartbeat_timeout: self.heartbeat_timeout,
        })
    }
}

/// Guard representing active leadership in distributed coordination.
///
/// Implements RAII pattern - leadership is released when dropped.
/// The guard tracks heartbeat timestamps and becomes invalid after
/// the configured timeout expires without calling `heartbeat()`.
pub struct PostgresCoordinatorGuard {
    pool: sqlx::Pool<sqlx::Postgres>,
    heartbeat_timeout: Duration,
}

impl PostgresCoordinatorGuard {
    /// Checks if this guard is still valid.
    ///
    /// Returns `false` if the heartbeat timeout has expired without a heartbeat.
    /// Returns `true` if the guard is still within the heartbeat timeout window.
    ///
    /// This is an async method because it requires database communication to check
    /// the heartbeat timestamp.
    pub async fn is_valid(&self) -> bool {
        use sqlx::Row;

        // Query the time elapsed since last heartbeat
        let result = sqlx::query(
            r#"
            SELECT NOW() - last_heartbeat AS elapsed
            FROM coordinator_heartbeat
            WHERE lock_id = $1
            "#,
        )
        .bind(COORDINATOR_LOCK_ID)
        .fetch_optional(&self.pool)
        .await;

        match result {
            Ok(Some(row)) => {
                // PostgreSQL interval type - extract as PgInterval and convert
                if let Ok(interval) = row.try_get::<sqlx::postgres::types::PgInterval, _>("elapsed")
                {
                    // PgInterval has microseconds field
                    let elapsed_micros = interval.microseconds;
                    let elapsed = Duration::from_micros(elapsed_micros as u64);
                    elapsed < self.heartbeat_timeout
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Sends a heartbeat signal to refresh the guard's validity.
    ///
    /// Updates the last-heartbeat timestamp in the database, extending the
    /// validity window by the configured heartbeat timeout.
    ///
    /// This is an async method because it requires database communication.
    pub async fn heartbeat(&self) {
        let _ = sqlx::query(
            r#"
            UPDATE coordinator_heartbeat
            SET last_heartbeat = NOW()
            WHERE lock_id = $1
            "#,
        )
        .bind(COORDINATOR_LOCK_ID)
        .execute(&self.pool)
        .await;
    }
}
