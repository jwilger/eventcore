use std::time::Duration;

use eventcore_types::{
    CheckpointStore, Event, EventFilter, EventPage, EventReader, EventStore, EventStoreError,
    EventStreamReader, EventStreamSlice, Operation, ProjectorCoordinator, StreamId, StreamPosition,
    StreamWriteEntry, StreamWrites,
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

impl CheckpointStore for PostgresEventStore {
    type Error = PostgresCheckpointError;

    async fn load(&self, name: &str) -> Result<Option<StreamPosition>, Self::Error> {
        let row = query("SELECT last_position FROM eventcore_subscription_versions WHERE subscription_name = $1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(PostgresCheckpointError::DatabaseError)?;

        match row {
            Some(row) => {
                let position: Uuid = row.get("last_position");
                Ok(Some(StreamPosition::new(position)))
            }
            None => Ok(None),
        }
    }

    async fn save(&self, name: &str, position: StreamPosition) -> Result<(), Self::Error> {
        let position_uuid: Uuid = position.into_inner();
        query(
            "INSERT INTO eventcore_subscription_versions (subscription_name, last_position, updated_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT (subscription_name) DO UPDATE SET last_position = $2, updated_at = NOW()",
        )
        .bind(name)
        .bind(position_uuid)
        .execute(&self.pool)
        .await
        .map_err(PostgresCheckpointError::DatabaseError)?;

        Ok(())
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

/// Error type for PostgresCheckpointStore operations.
#[derive(Debug, Error)]
pub enum PostgresCheckpointError {
    /// Failed to create connection pool.
    #[error("failed to create postgres connection pool")]
    ConnectionFailed(#[source] sqlx::Error),

    /// Database operation failed.
    #[error("database operation failed: {0}")]
    DatabaseError(#[source] sqlx::Error),
}

/// Postgres-backed checkpoint store for tracking projection progress.
///
/// `PostgresCheckpointStore` stores checkpoint positions in a PostgreSQL table,
/// providing durability across process restarts. It implements the `CheckpointStore`
/// trait from eventcore-types.
///
/// # Schema
///
/// The store uses the `eventcore_subscription_versions` table with:
/// - `subscription_name`: Unique identifier for each projector/subscription
/// - `last_position`: UUID7 representing the global stream position
/// - `updated_at`: Timestamp of the last checkpoint update
#[derive(Debug, Clone)]
pub struct PostgresCheckpointStore {
    pool: Pool<Postgres>,
}

impl PostgresCheckpointStore {
    /// Create a new PostgresCheckpointStore with default configuration.
    pub async fn new<S: Into<String>>(
        connection_string: S,
    ) -> Result<Self, PostgresCheckpointError> {
        Self::with_config(connection_string, PostgresConfig::default()).await
    }

    /// Create a new PostgresCheckpointStore with custom configuration.
    pub async fn with_config<S: Into<String>>(
        connection_string: S,
        config: PostgresConfig,
    ) -> Result<Self, PostgresCheckpointError> {
        let connection_string = connection_string.into();
        let max_connections: std::num::NonZeroU32 = config.max_connections.into();
        let pool = PgPoolOptions::new()
            .max_connections(max_connections.get())
            .acquire_timeout(config.acquire_timeout)
            .idle_timeout(config.idle_timeout)
            .connect(&connection_string)
            .await
            .map_err(PostgresCheckpointError::ConnectionFailed)?;

        // Run migrations to ensure table exists
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| {
                PostgresCheckpointError::DatabaseError(sqlx::Error::Migrate(Box::new(e)))
            })?;

        Ok(Self { pool })
    }

    /// Create a PostgresCheckpointStore from an existing connection pool.
    ///
    /// Use this when you need full control over pool configuration or want to
    /// share a pool across multiple components.
    pub fn from_pool(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

impl CheckpointStore for PostgresCheckpointStore {
    type Error = PostgresCheckpointError;

    async fn load(&self, name: &str) -> Result<Option<StreamPosition>, Self::Error> {
        let row = query("SELECT last_position FROM eventcore_subscription_versions WHERE subscription_name = $1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(PostgresCheckpointError::DatabaseError)?;

        match row {
            Some(row) => {
                let position: Uuid = row.get("last_position");
                Ok(Some(StreamPosition::new(position)))
            }
            None => Ok(None),
        }
    }

    async fn save(&self, name: &str, position: StreamPosition) -> Result<(), Self::Error> {
        let position_uuid: Uuid = position.into_inner();
        query(
            "INSERT INTO eventcore_subscription_versions (subscription_name, last_position, updated_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT (subscription_name) DO UPDATE SET last_position = $2, updated_at = NOW()",
        )
        .bind(name)
        .bind(position_uuid)
        .execute(&self.pool)
        .await
        .map_err(PostgresCheckpointError::DatabaseError)?;

        Ok(())
    }
}

// ============================================================================
// PostgresProjectorCoordinator - Distributed projector coordination via Postgres
// ============================================================================

/// Error type for projector coordination operations.
#[derive(Debug, Error)]
pub enum CoordinationError {
    /// Leadership could not be acquired (another instance holds the lock).
    #[error("leadership not acquired: another instance holds the lock")]
    LeadershipNotAcquired,

    /// Database operation failed.
    #[error("database operation failed: {0}")]
    DatabaseError(#[source] sqlx::Error),
}

/// Guard type that releases leadership when dropped.
///
/// Holds the advisory lock key and the actual database connection that acquired
/// the lock. This is critical because PostgreSQL advisory locks are session-scoped:
/// the unlock must happen on the same connection that acquired the lock.
pub struct CoordinationGuard {
    lock_key: i64,
    /// The actual connection that holds the advisory lock.
    /// Must be Option so we can take ownership in Drop.
    connection: Option<sqlx::pool::PoolConnection<Postgres>>,
}

impl std::fmt::Debug for CoordinationGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoordinationGuard")
            .field("lock_key", &self.lock_key)
            .finish_non_exhaustive()
    }
}

impl Drop for CoordinationGuard {
    fn drop(&mut self) {
        // Take ownership of the connection - we need the same connection that acquired the lock
        if let Some(mut connection) = self.connection.take() {
            let lock_key = self.lock_key;

            // Check runtime flavor to determine the appropriate unlock strategy.
            // block_in_place panics on single-threaded runtimes, so we must check first.
            let handle = tokio::runtime::Handle::current();
            let is_multi_thread =
                handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread;

            if is_multi_thread {
                // Multi-threaded runtime: use block_in_place for synchronous unlock
                tokio::task::block_in_place(|| {
                    handle.block_on(async {
                        // Unlock on the SAME connection that acquired the lock
                        let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
                            .bind(lock_key)
                            .execute(&mut *connection)
                            .await;
                        // Connection is returned to pool when dropped here
                    });
                });
            } else {
                // Single-threaded runtime: spawn a task for async unlock.
                // The lock will be released when the spawned task executes.
                tokio::spawn(async move {
                    let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
                        .bind(lock_key)
                        .execute(&mut *connection)
                        .await;
                });
            }
        }
    }
}

/// Postgres-backed projector coordinator for distributed leadership.
///
/// `PostgresProjectorCoordinator` uses PostgreSQL advisory locks to ensure
/// only one projector instance processes events for a given subscription
/// at a time, preventing duplicate processing in distributed deployments.
#[derive(Debug, Clone)]
pub struct PostgresProjectorCoordinator {
    pool: Pool<Postgres>,
}

impl PostgresProjectorCoordinator {
    /// Create a new PostgresProjectorCoordinator with default configuration.
    pub async fn new<S: Into<String>>(connection_string: S) -> Result<Self, CoordinationError> {
        Self::with_config(connection_string, PostgresConfig::default()).await
    }

    /// Create a new PostgresProjectorCoordinator with custom configuration.
    pub async fn with_config<S: Into<String>>(
        connection_string: S,
        config: PostgresConfig,
    ) -> Result<Self, CoordinationError> {
        let connection_string = connection_string.into();
        let max_connections: std::num::NonZeroU32 = config.max_connections.into();
        let pool = PgPoolOptions::new()
            .max_connections(max_connections.get())
            .acquire_timeout(config.acquire_timeout)
            .idle_timeout(config.idle_timeout)
            .connect(&connection_string)
            .await
            .map_err(CoordinationError::DatabaseError)?;

        Ok(Self { pool })
    }

    /// Create a PostgresProjectorCoordinator from an existing connection pool.
    pub fn from_pool(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }
}

impl ProjectorCoordinator for PostgresProjectorCoordinator {
    type Error = CoordinationError;
    type Guard = CoordinationGuard;

    async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Derive advisory lock key from subscription name
        let mut hasher = DefaultHasher::new();
        subscription_name.hash(&mut hasher);
        let lock_key = hasher.finish() as i64;

        // Acquire a dedicated connection from the pool.
        // This connection MUST be kept for the lifetime of the guard because
        // PostgreSQL advisory locks are session-scoped.
        let mut connection = self
            .pool
            .acquire()
            .await
            .map_err(CoordinationError::DatabaseError)?;

        // Attempt to acquire advisory lock (non-blocking) on this specific connection
        let row = sqlx::query("SELECT pg_try_advisory_lock($1)")
            .bind(lock_key)
            .fetch_one(&mut *connection)
            .await
            .map_err(CoordinationError::DatabaseError)?;

        let acquired: bool = row.get(0);

        if acquired {
            Ok(CoordinationGuard {
                lock_key,
                connection: Some(connection),
            })
        } else {
            // Lock not acquired - connection will be returned to pool here
            Err(CoordinationError::LeadershipNotAcquired)
        }
    }
}

impl ProjectorCoordinator for PostgresEventStore {
    type Error = CoordinationError;
    type Guard = CoordinationGuard;

    async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Derive advisory lock key from subscription name
        let mut hasher = DefaultHasher::new();
        subscription_name.hash(&mut hasher);
        let lock_key = hasher.finish() as i64;

        // Acquire a dedicated connection from the pool.
        // This connection MUST be kept for the lifetime of the guard because
        // PostgreSQL advisory locks are session-scoped.
        let mut connection = self
            .pool
            .acquire()
            .await
            .map_err(CoordinationError::DatabaseError)?;

        // Attempt to acquire advisory lock (non-blocking) on this specific connection
        let row = sqlx::query("SELECT pg_try_advisory_lock($1)")
            .bind(lock_key)
            .fetch_one(&mut *connection)
            .await
            .map_err(CoordinationError::DatabaseError)?;

        let acquired: bool = row.get(0);

        if acquired {
            Ok(CoordinationGuard {
                lock_key,
                connection: Some(connection),
            })
        } else {
            // Lock not acquired - connection will be returned to pool here
            Err(CoordinationError::LeadershipNotAcquired)
        }
    }
}
