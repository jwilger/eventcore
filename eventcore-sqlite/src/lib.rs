use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use eventcore_types::{
    CheckpointStore, Event, EventFilter, EventPage, EventReader, EventStore, EventStoreError,
    EventStreamReader, EventStreamSlice, Operation, ProjectorCoordinator, StreamId, StreamPosition,
    StreamWriteEntry, StreamWrites,
};
use rusqlite::OptionalExtension;
use rusqlite::params;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SqliteEventStoreError {
    #[error("failed to open SQLite connection: {0}")]
    ConnectionFailed(#[source] rusqlite::Error),

    #[error("migration failed: {0}")]
    MigrationFailed(#[source] rusqlite::Error),

    #[error("internal task failed: {0}")]
    TaskFailed(String),
}

#[derive(Debug, Error)]
pub enum SqliteCheckpointError {
    #[error("database operation failed: {0}")]
    DatabaseError(#[source] rusqlite::Error),

    #[error("corrupted checkpoint: invalid position UUID '{position}': {source}")]
    CorruptedCheckpoint {
        position: String,
        #[source]
        source: uuid::Error,
    },

    #[error("internal task failed: {0}")]
    TaskFailed(String),
}

#[derive(Debug, Error)]
pub enum SqliteCoordinationError {
    #[error("leadership not acquired: another instance holds the lock")]
    LeadershipNotAcquired { subscription_name: String },
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Configuration for SQLite event store connections.
///
/// The `encryption_key` field is redacted from `Debug` output to prevent
/// accidental exposure in logs.
#[derive(Clone)]
pub struct SqliteConfig {
    pub path: PathBuf,
    pub encryption_key: Option<String>,
}

impl std::fmt::Debug for SqliteConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteConfig")
            .field("path", &self.path)
            .field(
                "encryption_key",
                &self.encryption_key.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Shared connection helper
// ---------------------------------------------------------------------------

/// Internal helper for opening and configuring a SQLite connection.
/// Shared by `SqliteEventStore` and `SqliteCheckpointStore` to avoid
/// duplicating connection setup logic.
fn open_connection(path: &PathBuf) -> Result<rusqlite::Connection, SqliteEventStoreError> {
    let conn = rusqlite::Connection::open(path).map_err(SqliteEventStoreError::ConnectionFailed)?;
    // WAL mode is a no-op for in-memory databases but kept for code consistency.
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(SqliteEventStoreError::ConnectionFailed)?;
    Ok(conn)
}

fn open_in_memory_connection() -> Result<rusqlite::Connection, SqliteEventStoreError> {
    let conn =
        rusqlite::Connection::open_in_memory().map_err(SqliteEventStoreError::ConnectionFailed)?;
    // WAL mode is a no-op for in-memory databases but kept for code consistency.
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(SqliteEventStoreError::ConnectionFailed)?;
    Ok(conn)
}

fn apply_encryption_key(
    conn: &rusqlite::Connection,
    key: &str,
) -> Result<(), SqliteEventStoreError> {
    conn.pragma_update(None, "key", key)
        .map_err(SqliteEventStoreError::ConnectionFailed)
}

/// Map a `JoinError` from `spawn_blocking` into an `EventStoreError`.
fn map_join_error(e: tokio::task::JoinError, operation: Operation) -> EventStoreError {
    error!(error = %e, ?operation, "[sqlite] spawn_blocking task failed");
    EventStoreError::StoreFailure { operation }
}

/// Map a `JoinError` from `spawn_blocking` into a `SqliteEventStoreError`.
fn map_join_error_migration(e: tokio::task::JoinError) -> SqliteEventStoreError {
    SqliteEventStoreError::TaskFailed(e.to_string())
}

/// Map a `JoinError` from `spawn_blocking` into a `SqliteCheckpointError`.
fn map_join_error_checkpoint(e: tokio::task::JoinError) -> SqliteCheckpointError {
    SqliteCheckpointError::TaskFailed(e.to_string())
}

// ---------------------------------------------------------------------------
// Shared checkpoint helpers
// ---------------------------------------------------------------------------

fn checkpoint_load(
    conn: &rusqlite::Connection,
    name: &str,
) -> Result<Option<StreamPosition>, SqliteCheckpointError> {
    let mut stmt = conn
        .prepare(
            "SELECT last_position FROM eventcore_subscription_versions WHERE subscription_name = ?1",
        )
        .map_err(SqliteCheckpointError::DatabaseError)?;

    let result: Option<String> = stmt
        .query_row(params![name], |row| row.get(0))
        .optional()
        .map_err(SqliteCheckpointError::DatabaseError)?;

    match result {
        Some(pos_str) => {
            let uuid = Uuid::parse_str(&pos_str).map_err(|e| {
                SqliteCheckpointError::CorruptedCheckpoint {
                    position: pos_str,
                    source: e,
                }
            })?;
            Ok(Some(StreamPosition::new(uuid)))
        }
        None => Ok(None),
    }
}

fn checkpoint_save(
    conn: &rusqlite::Connection,
    name: &str,
    position_str: &str,
) -> Result<(), SqliteCheckpointError> {
    conn.execute(
        "INSERT INTO eventcore_subscription_versions (subscription_name, last_position, updated_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT (subscription_name) DO UPDATE SET last_position = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![name, position_str],
    )
    .map_err(SqliteCheckpointError::DatabaseError)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared coordination helpers
// ---------------------------------------------------------------------------

fn try_acquire_lock(
    locks: &std::sync::RwLock<HashSet<String>>,
    subscription_name: &str,
) -> Result<(), SqliteCoordinationError> {
    let mut guard = locks.write().expect("coordination lock poisoned");
    if guard.contains(subscription_name) {
        return Err(SqliteCoordinationError::LeadershipNotAcquired {
            subscription_name: subscription_name.to_string(),
        });
    }
    guard.insert(subscription_name.to_string());
    Ok(())
}

// ---------------------------------------------------------------------------
// SqliteEventStore
// ---------------------------------------------------------------------------

pub struct SqliteEventStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
    locks: Arc<std::sync::RwLock<HashSet<String>>>,
}

impl std::fmt::Debug for SqliteEventStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteEventStore").finish_non_exhaustive()
    }
}

impl SqliteEventStore {
    pub fn new(config: SqliteConfig) -> Result<Self, SqliteEventStoreError> {
        let conn = open_connection(&config.path)?;
        if let Some(ref key) = config.encryption_key {
            apply_encryption_key(&conn, key)?;
        }
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            locks: Arc::new(std::sync::RwLock::new(HashSet::new())),
        })
    }

    pub fn in_memory() -> Result<Self, SqliteEventStoreError> {
        let conn = open_in_memory_connection()?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            locks: Arc::new(std::sync::RwLock::new(HashSet::new())),
        })
    }

    pub async fn migrate(&self) -> Result<(), SqliteEventStoreError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS eventcore_events (
                    event_id TEXT PRIMARY KEY,
                    stream_id TEXT NOT NULL,
                    stream_version INTEGER NOT NULL,
                    event_type TEXT NOT NULL,
                    event_data TEXT NOT NULL,
                    metadata TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_eventcore_events_stream_version
                    ON eventcore_events (stream_id, stream_version);
                CREATE INDEX IF NOT EXISTS idx_eventcore_events_stream_id
                    ON eventcore_events (stream_id);
                CREATE TABLE IF NOT EXISTS eventcore_subscription_versions (
                    subscription_name TEXT PRIMARY KEY,
                    last_position TEXT NOT NULL,
                    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );",
            )
            .map_err(SqliteEventStoreError::MigrationFailed)?;
            Ok(())
        })
        .await
        .map_err(map_join_error_migration)?
    }
}

impl EventStore for SqliteEventStore {
    #[instrument(name = "sqlite.read_stream", skip(self))]
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStreamReader<E>, EventStoreError> {
        info!(
            stream = %stream_id,
            "[sqlite.read_stream] reading events from sqlite"
        );

        let conn = self.conn.clone();
        let sid = stream_id.clone();
        let rows = tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let mut stmt = conn
                .prepare(
                    "SELECT event_data FROM eventcore_events WHERE stream_id = ?1 ORDER BY stream_version ASC",
                )
                .map_err(|e| {
                    error!(error = %e, "[sqlite.read_stream] prepare failed");
                    EventStoreError::StoreFailure {
                        operation: Operation::ReadStream,
                    }
                })?;
            let rows: Vec<String> = stmt
                .query_map(params![sid.as_ref()], |row| row.get(0))
                .map_err(|e| {
                    error!(error = %e, "[sqlite.read_stream] query failed");
                    EventStoreError::StoreFailure {
                        operation: Operation::ReadStream,
                    }
                })?
                .collect::<Result<Vec<String>, _>>()
                .map_err(|e| {
                    error!(error = %e, "[sqlite.read_stream] row extraction failed");
                    EventStoreError::StoreFailure {
                        operation: Operation::ReadStream,
                    }
                })?;
            Ok::<Vec<String>, EventStoreError>(rows)
        })
        .await
        .map_err(|e| map_join_error(e, Operation::ReadStream))??;

        let mut events = Vec::with_capacity(rows.len());
        for json_str in rows {
            let event: E = serde_json::from_str(&json_str).map_err(|e| {
                EventStoreError::DeserializationFailed {
                    stream_id: stream_id.clone(),
                    detail: e.to_string(),
                }
            })?;
            events.push(event);
        }

        Ok(EventStreamReader::new(events))
    }

    #[instrument(name = "sqlite.append_events", skip(self, writes))]
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
            "[sqlite.append_events] appending events to sqlite"
        );

        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let tx = conn.unchecked_transaction().map_err(|e| {
                error!(error = %e, "[sqlite.append_events] begin transaction failed");
                EventStoreError::StoreFailure {
                    operation: Operation::BeginTransaction,
                }
            })?;

            for (stream_id, expected_version) in &expected_versions {
                let current: usize = tx
                    .query_row(
                        "SELECT COALESCE(MAX(stream_version), 0) FROM eventcore_events WHERE stream_id = ?1",
                        params![stream_id.as_ref()],
                        |row| row.get(0),
                    )
                    .map_err(|e| {
                        error!(error = %e, "[sqlite.append_events] version check failed");
                        EventStoreError::StoreFailure {
                            operation: Operation::AppendEvents,
                        }
                    })?;

                if current != expected_version.into_inner() {
                    warn!(
                        stream = %stream_id,
                        expected = expected_version.into_inner(),
                        actual = current,
                        "[sqlite.version_conflict] optimistic concurrency check failed"
                    );
                    return Err(EventStoreError::VersionConflict);
                }
            }

            // Initialize per-stream version counters from expected versions;
            // each will be incremented before assignment.
            let mut current_versions: HashMap<&StreamId, usize> = expected_versions
                .iter()
                .map(|(sid, v)| (sid, v.into_inner()))
                .collect();

            for entry in &entries {
                let StreamWriteEntry {
                    stream_id,
                    event_type,
                    event_data,
                    ..
                } = entry;

                let event_id = Uuid::now_v7().to_string();
                let version_counter = current_versions
                    .get_mut(stream_id)
                    .expect("stream must be registered");
                *version_counter += 1;
                let version = *version_counter;

                let event_json = serde_json::to_string(event_data).map_err(|e| {
                    error!(error = %e, "[sqlite.append_events] serialization failed");
                    EventStoreError::StoreFailure {
                        operation: Operation::AppendEvents,
                    }
                })?;

                tx.execute(
                    "INSERT INTO eventcore_events (event_id, stream_id, stream_version, event_type, event_data, metadata)
                     VALUES (?1, ?2, ?3, ?4, ?5, '{}')",
                    params![
                        event_id,
                        stream_id.as_ref(),
                        version,
                        event_type,
                        event_json,
                    ],
                )
                .map_err(|e| {
                    error!(error = %e, "[sqlite.append_events] insert failed");
                    EventStoreError::StoreFailure {
                        operation: Operation::AppendEvents,
                    }
                })?;
            }

            tx.commit().map_err(|e| {
                error!(error = %e, "[sqlite.append_events] commit failed");
                EventStoreError::StoreFailure {
                    operation: Operation::CommitTransaction,
                }
            })?;

            Ok(EventStreamSlice)
        })
        .await
        .map_err(|e| map_join_error(e, Operation::AppendEvents))?
    }
}

impl EventReader for SqliteEventStore {
    type Error = EventStoreError;

    #[instrument(name = "sqlite.read_events", skip(self))]
    async fn read_events<E: Event>(
        &self,
        filter: EventFilter,
        page: EventPage,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        let conn = self.conn.clone();
        let after_event_id: Option<String> =
            page.after_position().map(|p| p.into_inner().to_string());
        let limit = page.limit().into_inner() as i64;
        let prefix = filter.stream_prefix().map(|p| p.as_ref().to_string());

        let rows = tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();

            let (sql, param_values): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
                match (&prefix, &after_event_id) {
                    // UUIDv7 event IDs sort lexicographically in chronological order,
                    // so text comparison (`event_id > ?1`) preserves insertion order
                    // for cursor-based pagination.
                    (Some(pfx), Some(after_id)) => (
                        "SELECT event_id, event_data FROM eventcore_events WHERE event_id > ?1 AND stream_id LIKE ?2 ORDER BY event_id LIMIT ?3"
                            .to_string(),
                        vec![
                            Box::new(after_id.clone()) as Box<dyn rusqlite::types::ToSql>,
                            Box::new(format!("{}%", pfx)),
                            Box::new(limit),
                        ],
                    ),
                    (Some(pfx), None) => (
                        "SELECT event_id, event_data FROM eventcore_events WHERE stream_id LIKE ?1 ORDER BY event_id LIMIT ?2"
                            .to_string(),
                        vec![
                            Box::new(format!("{}%", pfx)) as Box<dyn rusqlite::types::ToSql>,
                            Box::new(limit),
                        ],
                    ),
                    (None, Some(after_id)) => (
                        "SELECT event_id, event_data FROM eventcore_events WHERE event_id > ?1 ORDER BY event_id LIMIT ?2"
                            .to_string(),
                        vec![
                            Box::new(after_id.clone()) as Box<dyn rusqlite::types::ToSql>,
                            Box::new(limit),
                        ],
                    ),
                    (None, None) => (
                        "SELECT event_id, event_data FROM eventcore_events ORDER BY event_id LIMIT ?1"
                            .to_string(),
                        vec![Box::new(limit) as Box<dyn rusqlite::types::ToSql>],
                    ),
                };

            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| {
                error!(error = %e, "[sqlite.read_events] prepare failed");
                EventStoreError::StoreFailure {
                    operation: Operation::ReadStream,
                }
            })?;

            let rows: Vec<(String, String)> = stmt
                .query_map(params_refs.as_slice(), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| {
                    error!(error = %e, "[sqlite.read_events] query failed");
                    EventStoreError::StoreFailure {
                        operation: Operation::ReadStream,
                    }
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| {
                    error!(error = %e, "[sqlite.read_events] row extraction failed");
                    EventStoreError::StoreFailure {
                        operation: Operation::ReadStream,
                    }
                })?;

            Ok::<Vec<(String, String)>, EventStoreError>(rows)
        })
        .await
        .map_err(|e| map_join_error(e, Operation::ReadStream))??;

        // Silently skip events that cannot be deserialized into the requested
        // type E. This is intentional: EventReader serves polymorphic consumers
        // that may only understand a subset of stored event types. This matches
        // the behavior of the postgres and in-memory backends.
        let events: Vec<(E, StreamPosition)> = rows
            .into_iter()
            .filter_map(|(event_id_str, event_data_str)| {
                let uuid = Uuid::parse_str(&event_id_str).ok()?;
                let event: E = serde_json::from_str(&event_data_str).ok()?;
                Some((event, StreamPosition::new(uuid)))
            })
            .collect();

        Ok(events)
    }
}

impl CheckpointStore for SqliteEventStore {
    type Error = SqliteCheckpointError;

    #[instrument(name = "sqlite.checkpoint.load", skip(self))]
    async fn load(&self, name: &str) -> Result<Option<StreamPosition>, Self::Error> {
        let conn = self.conn.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            checkpoint_load(&conn, &name)
        })
        .await
        .map_err(map_join_error_checkpoint)?
    }

    #[instrument(name = "sqlite.checkpoint.save", skip(self))]
    async fn save(&self, name: &str, position: StreamPosition) -> Result<(), Self::Error> {
        let conn = self.conn.clone();
        let name = name.to_string();
        let position_str = position.into_inner().to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            checkpoint_save(&conn, &name, &position_str)
        })
        .await
        .map_err(map_join_error_checkpoint)?
    }
}

impl ProjectorCoordinator for SqliteEventStore {
    type Error = SqliteCoordinationError;
    type Guard = SqliteCoordinationGuard;

    #[instrument(name = "sqlite.coordinator.try_acquire", skip(self))]
    async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
        try_acquire_lock(&self.locks, subscription_name)?;
        Ok(SqliteCoordinationGuard {
            subscription_name: subscription_name.to_string(),
            locks: Arc::clone(&self.locks),
        })
    }
}

// ---------------------------------------------------------------------------
// SqliteCoordinationGuard
// ---------------------------------------------------------------------------

/// Guard that releases leadership when dropped.
///
/// Uses `std::sync::RwLock` (not `tokio::sync::RwLock`) so that cleanup
/// works reliably in `Drop` without requiring an async runtime.
#[derive(Debug)]
pub struct SqliteCoordinationGuard {
    subscription_name: String,
    locks: Arc<std::sync::RwLock<HashSet<String>>>,
}

impl Drop for SqliteCoordinationGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.locks.write() {
            guard.remove(&self.subscription_name);
        } else {
            error!(
                subscription = %self.subscription_name,
                "[sqlite.coordination_guard] lock poisoned, cannot release leadership"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// SqliteCheckpointStore (standalone)
// ---------------------------------------------------------------------------

pub struct SqliteCheckpointStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
}

impl std::fmt::Debug for SqliteCheckpointStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteCheckpointStore")
            .finish_non_exhaustive()
    }
}

impl SqliteCheckpointStore {
    pub fn new(config: SqliteConfig) -> Result<Self, SqliteEventStoreError> {
        let conn = open_connection(&config.path)?;
        if let Some(ref key) = config.encryption_key {
            apply_encryption_key(&conn, key)?;
        }
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn in_memory() -> Result<Self, SqliteEventStoreError> {
        let conn = open_in_memory_connection()?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn migrate(&self) -> Result<(), SqliteEventStoreError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS eventcore_subscription_versions (
                    subscription_name TEXT PRIMARY KEY,
                    last_position TEXT NOT NULL,
                    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );",
            )
            .map_err(SqliteEventStoreError::MigrationFailed)?;
            Ok(())
        })
        .await
        .map_err(map_join_error_migration)?
    }
}

impl CheckpointStore for SqliteCheckpointStore {
    type Error = SqliteCheckpointError;

    #[instrument(name = "sqlite.checkpoint.load", skip(self))]
    async fn load(&self, name: &str) -> Result<Option<StreamPosition>, Self::Error> {
        let conn = self.conn.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            checkpoint_load(&conn, &name)
        })
        .await
        .map_err(map_join_error_checkpoint)?
    }

    #[instrument(name = "sqlite.checkpoint.save", skip(self))]
    async fn save(&self, name: &str, position: StreamPosition) -> Result<(), Self::Error> {
        let conn = self.conn.clone();
        let name = name.to_string();
        let position_str = position.into_inner().to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            checkpoint_save(&conn, &name, &position_str)
        })
        .await
        .map_err(map_join_error_checkpoint)?
    }
}

// ---------------------------------------------------------------------------
// SqliteProjectorCoordinator (standalone)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SqliteProjectorCoordinator {
    locks: Arc<std::sync::RwLock<HashSet<String>>>,
}

impl SqliteProjectorCoordinator {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProjectorCoordinator for SqliteProjectorCoordinator {
    type Error = SqliteCoordinationError;
    type Guard = SqliteCoordinationGuard;

    #[instrument(name = "sqlite.coordinator.try_acquire", skip(self))]
    async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
        try_acquire_lock(&self.locks, subscription_name)?;
        Ok(SqliteCoordinationGuard {
            subscription_name: subscription_name.to_string(),
            locks: Arc::clone(&self.locks),
        })
    }
}
