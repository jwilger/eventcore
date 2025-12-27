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
