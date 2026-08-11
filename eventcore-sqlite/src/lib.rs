//! SQLite event store backend for EventCore.
//!
//! This crate provides a lightweight, file-based [`SqliteEventStore`] suitable
//! for testing, development, and single-machine deployments.
//!
//! Optional SQLCipher encryption is supported via the `encryption_key` field
//! in [`SqliteConfig`]. When an encryption key is provided, the database file
//! is encrypted at rest using SQLCipher.
//!
//! SQLCipher encryption only takes effect when the `encryption` Cargo feature
//! is enabled (it is off by default). Without it the crate links vanilla
//! SQLite, the `encryption_key` PRAGMA is silently ignored, and the database
//! is written unencrypted.
//!
//! `SqliteEventStore` implements `EventStore`, `EventReader`, `CheckpointStore`,
//! and projector coordination from `eventcore-types`.
//!
//! Construct a store using [`SqliteEventStore::from_connection`] with a
//! `rusqlite::Connection`. Use `:memory:` for in-memory stores in tests.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use eventcore_types::{
    CheckpointStore, CommandStateSnapshot, CommandStateSnapshotId, Event, EventFilter, EventPage,
    EventReader, EventStore, EventStoreError, EventStream, EventStreamSlice, Operation,
    ProjectorCoordinator, StreamId, StreamPosition, StreamVersion, StreamWriteEntry, StreamWrites,
};
pub use rusqlite;
use rusqlite::OptionalExtension;
use rusqlite::{params, params_from_iter};
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur when creating or using a [`SqliteEventStore`].
#[derive(Debug, Error)]
pub enum SqliteEventStoreError {
    #[error("failed to open SQLite connection: {0}")]
    ConnectionFailed(#[source] rusqlite::Error),

    #[error("migration failed: {0}")]
    MigrationFailed(#[source] rusqlite::Error),

    #[error("internal task failed: {0}")]
    TaskFailed(String),
}

/// Errors from checkpoint store operations on a [`SqliteEventStore`].
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

/// Errors from projector coordination (advisory locking) on a [`SqliteEventStore`].
#[derive(Debug, Error)]
pub enum SqliteCoordinationError {
    #[error(
        "leadership not acquired for subscription '{subscription_name}': another instance holds the lock"
    )]
    LeadershipNotAcquired { subscription_name: String },

    #[error("coordination lock poisoned: {message}")]
    LockPoisoned { message: String },
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
    /// Filesystem location of the database file.
    pub path: PathBuf,
    /// Optional SQLCipher key. Only honored when the `encryption` Cargo
    /// feature is enabled; on the default (vanilla SQLite) build it is
    /// silently ignored and the database is written unencrypted.
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
fn open_connection(
    path: &PathBuf,
    encryption_key: Option<&str>,
) -> Result<rusqlite::Connection, SqliteEventStoreError> {
    let conn = rusqlite::Connection::open(path).map_err(SqliteEventStoreError::ConnectionFailed)?;
    // SQLCipher requires PRAGMA key before any other PRAGMA, otherwise the
    // database file is treated as unencrypted and subsequent operations fail.
    if let Some(key) = encryption_key {
        apply_encryption_key(&conn, key)?;
        // Validate the key works before proceeding.
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .map_err(SqliteEventStoreError::ConnectionFailed)?;
    }
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
    let _ = conn.execute(
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
    let mut guard = locks
        .write()
        .map_err(|e| SqliteCoordinationError::LockPoisoned {
            message: e.to_string(),
        })?;
    if guard.contains(subscription_name) {
        return Err(SqliteCoordinationError::LeadershipNotAcquired {
            subscription_name: subscription_name.to_string(),
        });
    }
    let _ = guard.insert(subscription_name.to_string());
    Ok(())
}

// ---------------------------------------------------------------------------
// SqliteEventStore
// ---------------------------------------------------------------------------

/// SQLite-backed event store implementing EventCore's storage traits.
///
/// Supports single-file or in-memory persistence. Construct via
/// [`SqliteEventStore::from_connection`] with a `rusqlite::Connection`; for an
/// in-memory store, open the connection with `":memory:"` (useful in tests).
///
/// All SQLite operations are executed via `spawn_blocking` for async compatibility.
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
    /// Open (or create) a SQLite store at the configured path, applying
    /// EventCore's default pragmas (WAL, plus the SQLCipher key when configured
    /// and the `encryption` feature is enabled).
    pub fn new(config: SqliteConfig) -> Result<Self, SqliteEventStoreError> {
        let conn = open_connection(&config.path, config.encryption_key.as_deref())?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            locks: Arc::new(std::sync::RwLock::new(HashSet::new())),
        })
    }

    /// Create an ephemeral in-memory store, useful for tests.
    pub fn in_memory() -> Result<Self, SqliteEventStoreError> {
        let conn = open_in_memory_connection()?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            locks: Arc::new(std::sync::RwLock::new(HashSet::new())),
        })
    }

    /// Construct a store from a connection the consumer has already opened
    /// and configured.
    ///
    /// The connection is taken as-is. The consumer is responsible for any
    /// pragmas (journal mode, encryption keys, attached databases, etc.).
    /// Use this constructor when you need fine-grained control over connection
    /// setup; otherwise prefer [`SqliteEventStore::new`] or
    /// [`SqliteEventStore::in_memory`].
    pub fn from_connection(conn: rusqlite::Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
            locks: Arc::new(std::sync::RwLock::new(HashSet::new())),
        }
    }

    /// Create EventCore's event, subscription, and command-state projection
    /// tables if they do not exist.
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
                );
                CREATE TABLE IF NOT EXISTS eventcore_command_state_snapshots (
                    snapshot_id TEXT PRIMARY KEY,
                    snapshot_data TEXT NOT NULL,
                    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                );",
            )
            .map_err(SqliteEventStoreError::MigrationFailed)?;
            Ok(())
        })
        .await
        .map_err(map_join_error_migration)?
    }

    async fn read_stream_from<E: Event>(
        &self,
        stream_id: StreamId,
        exclusive_version: Option<StreamVersion>,
    ) -> Result<EventStream<E>, EventStoreError> {
        let conn = self.conn.clone();
        let sid = stream_id.clone();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<String, EventStoreError>>(64);

        let _read_task = tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let sql = match exclusive_version {
                Some(_) => {
                    "SELECT event_data FROM eventcore_events WHERE stream_id = ?1 AND stream_version > ?2 ORDER BY stream_version ASC"
                }
                None => {
                    "SELECT event_data FROM eventcore_events WHERE stream_id = ?1 ORDER BY stream_version ASC"
                }
            };
            let mut stmt = match conn.prepare(sql) {
                Ok(stmt) => stmt,
                Err(e) => {
                    error!(error = %e, "[sqlite.read_stream] prepare failed");
                    let _ = tx.blocking_send(Err(EventStoreError::StoreFailure {
                        operation: Operation::ReadStream,
                    }));
                    return;
                }
            };

            let rows = match exclusive_version {
                Some(version) => stmt.query(params![sid.as_ref(), version.into_inner()]),
                None => stmt.query(params![sid.as_ref()]),
            };
            let mut rows = match rows {
                Ok(rows) => rows,
                Err(e) => {
                    error!(error = %e, "[sqlite.read_stream] query failed");
                    let _ = tx.blocking_send(Err(EventStoreError::StoreFailure {
                        operation: Operation::ReadStream,
                    }));
                    return;
                }
            };

            loop {
                match rows.next() {
                    Ok(Some(row)) => match row.get::<_, String>(0) {
                        Ok(json) => {
                            if tx.blocking_send(Ok(json)).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "[sqlite.read_stream] row extraction failed");
                            let _ = tx.blocking_send(Err(EventStoreError::StoreFailure {
                                operation: Operation::ReadStream,
                            }));
                            break;
                        }
                    },
                    Ok(None) => break,
                    Err(e) => {
                        error!(error = %e, "[sqlite.read_stream] row iteration failed");
                        let _ = tx.blocking_send(Err(EventStoreError::StoreFailure {
                            operation: Operation::ReadStream,
                        }));
                        break;
                    }
                }
            }
        });

        let stream = async_stream::stream! {
            while let Some(item) = rx.recv().await {
                match item {
                    Ok(json) => match serde_json::from_str::<E>(&json) {
                        Ok(event) => yield Ok(event),
                        Err(e) => {
                            yield Err(EventStoreError::DeserializationFailed {
                                stream_id: stream_id.clone(),
                                detail: e.to_string(),
                            });
                            break;
                        }
                    },
                    Err(e) => {
                        yield Err(e);
                        break;
                    }
                }
            }
        };

        Ok(EventStream::new(stream))
    }
}

impl EventStore for SqliteEventStore {
    #[instrument(name = "sqlite.read_stream", skip(self))]
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStream<E>, EventStoreError> {
        info!(
            stream = %stream_id,
            "[sqlite.read_stream] reading events from sqlite"
        );

        self.read_stream_from(stream_id, None).await
    }

    #[instrument(name = "sqlite.read_stream_after", skip(self))]
    async fn read_stream_after<E: Event>(
        &self,
        stream_id: StreamId,
        exclusive_version: StreamVersion,
    ) -> Result<EventStream<E>, EventStoreError> {
        self.read_stream_from(stream_id, Some(exclusive_version))
            .await
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
                    return Err(EventStoreError::VersionConflict {
                        stream_id: stream_id.clone(),
                        expected: *expected_version,
                        actual: StreamVersion::new(current),
                    });
                }
            }

            // Initialize per-stream version counters from expected versions;
            // each will be incremented before assignment.
            let mut current_versions: HashMap<&StreamId, usize> = expected_versions
                .iter()
                .map(|(sid, v)| (sid, v.into_inner()))
                .collect();

            // Assign per-stream versions and serialize each event once, building
            // the flat list of bound values for a multi-row INSERT. All events in
            // this call share one transaction, so they can be written in a single
            // (chunked) statement rather than one round-trip per event.
            let mut bound_values: Vec<rusqlite::types::Value> =
                Vec::with_capacity(entries.len() * COLUMNS_PER_ROW);
            for entry in &entries {
                let StreamWriteEntry {
                    stream_id,
                    event_type,
                    event_data,
                    ..
                } = entry;

                let event_id = Uuid::now_v7().to_string();
                let version_counter = current_versions.get_mut(stream_id).ok_or_else(|| {
                    EventStoreError::UndeclaredStream {
                        stream_id: stream_id.clone(),
                    }
                })?;
                *version_counter += 1;
                let version = *version_counter;

                // event_data is already serialized JSON (serialized once at
                // append time); bind its raw string directly instead of
                // re-serializing a serde_json::Value.
                let event_json = event_data.get().to_owned();

                bound_values.push(rusqlite::types::Value::Text(event_id));
                bound_values.push(rusqlite::types::Value::Text(stream_id.as_ref().to_string()));
                bound_values.push(rusqlite::types::Value::Integer(version as i64));
                bound_values.push(rusqlite::types::Value::Text(event_type.to_string()));
                bound_values.push(rusqlite::types::Value::Text(event_json));
            }

            // Chunk so the bound-parameter count stays well under SQLite's
            // SQLITE_MAX_VARIABLE_NUMBER limit regardless of build configuration.
            const COLUMNS_PER_ROW: usize = 5;
            const MAX_ROWS_PER_INSERT: usize = 100;
            const ROW_PLACEHOLDER: &str = "(?,?,?,?,?,'{}')";
            for chunk in bound_values.chunks(MAX_ROWS_PER_INSERT * COLUMNS_PER_ROW) {
                let row_count = chunk.len() / COLUMNS_PER_ROW;
                let values_clause = vec![ROW_PLACEHOLDER; row_count].join(",");
                let sql = format!(
                    "INSERT INTO eventcore_events (event_id, stream_id, stream_version, event_type, event_data, metadata) VALUES {values_clause}"
                );
                let _ = tx
                    .execute(&sql, params_from_iter(chunk.iter()))
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

    #[instrument(name = "sqlite.snapshot.load", skip(self))]
    async fn load_command_state_snapshot(
        &self,
        snapshot_id: CommandStateSnapshotId,
    ) -> Result<Option<CommandStateSnapshot>, EventStoreError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let snapshot_data: Option<String> = conn
                .query_row(
                    "SELECT snapshot_data FROM eventcore_command_state_snapshots WHERE snapshot_id = ?1",
                    params![snapshot_id.as_ref()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| {
                    error!(error = %e, "[sqlite.snapshot.load] query failed");
                    EventStoreError::StoreFailure {
                        operation: Operation::ReadStream,
                    }
                })?;

            snapshot_data
                .map(|data| {
                    serde_json::from_str(&data).map_err(|e| {
                        error!(error = %e, "[sqlite.snapshot.load] snapshot deserialization failed");
                        EventStoreError::StoreFailure {
                            operation: Operation::ReadStream,
                        }
                    })
                })
                .transpose()
        })
        .await
        .map_err(|e| map_join_error(e, Operation::ReadStream))?
    }

    #[instrument(name = "sqlite.snapshot.save", skip(self, snapshot))]
    async fn save_command_state_snapshot(
        &self,
        snapshot_id: CommandStateSnapshotId,
        snapshot: CommandStateSnapshot,
    ) -> Result<(), EventStoreError> {
        let serialized_snapshot = serde_json::to_string(&snapshot).map_err(|e| {
            error!(error = %e, "[sqlite.snapshot.save] snapshot serialization failed");
            EventStoreError::StoreFailure {
                operation: Operation::AppendEvents,
            }
        })?;
        let conn = self.conn.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            let tx = conn.unchecked_transaction().map_err(|e| {
                error!(error = %e, "[sqlite.snapshot.save] begin transaction failed");
                EventStoreError::StoreFailure {
                    operation: Operation::BeginTransaction,
                }
            })?;
            let current_data: Option<String> = tx
                .query_row(
                    "SELECT snapshot_data FROM eventcore_command_state_snapshots WHERE snapshot_id = ?1",
                    params![snapshot_id.as_ref()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| {
                    error!(error = %e, "[sqlite.snapshot.save] existing snapshot query failed");
                    EventStoreError::StoreFailure {
                        operation: Operation::AppendEvents,
                    }
                })?;

            let should_save = match current_data {
                Some(data) => {
                    let current: CommandStateSnapshot = serde_json::from_str(&data).map_err(|e| {
                        error!(error = %e, "[sqlite.snapshot.save] existing snapshot deserialization failed");
                        EventStoreError::StoreFailure {
                            operation: Operation::AppendEvents,
                        }
                    })?;
                    snapshot.covers(&current)
                }
                None => true,
            };

            if should_save {
                let _ = tx
                    .execute(
                        "INSERT INTO eventcore_command_state_snapshots (snapshot_id, snapshot_data, updated_at)
                         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                         ON CONFLICT (snapshot_id) DO UPDATE SET
                             snapshot_data = excluded.snapshot_data,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                        params![snapshot_id.as_ref(), serialized_snapshot],
                    )
                    .map_err(|e| {
                        error!(error = %e, "[sqlite.snapshot.save] upsert failed");
                        EventStoreError::StoreFailure {
                            operation: Operation::AppendEvents,
                        }
                    })?;
            }

            tx.commit().map_err(|e| {
                error!(error = %e, "[sqlite.snapshot.save] commit failed");
                EventStoreError::StoreFailure {
                    operation: Operation::CommitTransaction,
                }
            })
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
        // Glob pattern pushdown uses SQLite's native GLOB operator (ADR-0047).
        let pattern = filter.stream_pattern().map(|p| p.as_ref().to_string());
        let type_filter = filter
            .event_type()
            .unwrap_or_else(|| E::event_type_name())
            .to_string();

        let rows = tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();

            // Build the WHERE clause dynamically so prefix XOR pattern, the
            // optional cursor, and the event_type predicate compose without a
            // combinatorial explosion of hand-written query strings. Bound
            // values are appended in the same order the placeholders are
            // emitted. UUIDv7 event IDs sort lexicographically in chronological
            // order, so text comparison (`event_id > ?`) preserves insertion
            // order for cursor-based pagination.
            let mut conditions: Vec<String> = Vec::new();
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(after_id) = &after_event_id {
                conditions.push(format!("event_id > ?{}", param_values.len() + 1));
                param_values.push(Box::new(after_id.clone()));
            }

            if let Some(pfx) = &prefix {
                conditions.push(format!("stream_id LIKE ?{}", param_values.len() + 1));
                param_values.push(Box::new(format!("{}%", pfx)));
            } else if let Some(pat) = &pattern {
                conditions.push(format!("stream_id GLOB ?{}", param_values.len() + 1));
                param_values.push(Box::new(pat.clone()));
            }

            conditions.push(format!("event_type = ?{}", param_values.len() + 1));
            param_values.push(Box::new(type_filter.clone()));

            let limit_placeholder = format!("?{}", param_values.len() + 1);
            param_values.push(Box::new(limit));

            let sql = format!(
                "SELECT event_id, event_data FROM eventcore_events WHERE {} ORDER BY event_id LIMIT {}",
                conditions.join(" AND "),
                limit_placeholder,
            );

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
            let _ = guard.remove(&self.subscription_name);
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

/// Standalone SQLite-backed [`CheckpointStore`] for persisting projection
/// positions independently of an event store.
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
    /// Open (or create) a SQLite checkpoint store at the configured path,
    /// applying EventCore's default pragmas (WAL, plus the SQLCipher key when
    /// configured and the `encryption` feature is enabled).
    pub fn new(config: SqliteConfig) -> Result<Self, SqliteEventStoreError> {
        let conn = open_connection(&config.path, config.encryption_key.as_deref())?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create an ephemeral in-memory checkpoint store, useful for tests.
    pub fn in_memory() -> Result<Self, SqliteEventStoreError> {
        let conn = open_in_memory_connection()?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Construct a checkpoint store from a connection the consumer has
    /// already opened and configured.
    ///
    /// The connection is taken as-is. The consumer is responsible for any
    /// pragmas (journal mode, encryption keys, attached databases, etc.).
    /// Use this constructor when you need fine-grained control over connection
    /// setup; otherwise prefer [`SqliteCheckpointStore::new`] or
    /// [`SqliteCheckpointStore::in_memory`].
    pub fn from_connection(conn: rusqlite::Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    /// Create the `eventcore_subscription_versions` table if it does not exist.
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

/// Standalone in-process [`ProjectorCoordinator`] providing advisory locking
/// so a single instance leads each subscription.
#[derive(Debug, Clone, Default)]
pub struct SqliteProjectorCoordinator {
    locks: Arc<std::sync::RwLock<HashSet<String>>>,
}

impl SqliteProjectorCoordinator {
    /// Create a new coordinator with no leadership locks held.
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
