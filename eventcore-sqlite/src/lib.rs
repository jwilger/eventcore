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
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum SqliteEventStoreError {
    #[error("failed to open SQLite connection: {0}")]
    ConnectionFailed(#[source] rusqlite::Error),

    #[error("migration failed: {0}")]
    MigrationFailed(#[source] rusqlite::Error),
}

#[derive(Debug, Error)]
pub enum SqliteCheckpointError {
    #[error("database operation failed: {0}")]
    DatabaseError(#[source] rusqlite::Error),
}

#[derive(Debug, Error)]
pub enum SqliteCoordinationError {
    #[error("leadership not acquired: another instance holds the lock")]
    LeadershipNotAcquired { subscription_name: String },

    #[error("lock poisoned: {message}")]
    LockPoisoned { message: String },
}

#[derive(Debug, Clone)]
pub struct SqliteConfig {
    pub path: PathBuf,
    pub encryption_key: Option<String>,
}

pub struct SqliteEventStore {
    conn: Arc<Mutex<rusqlite::Connection>>,
    locks: Arc<RwLock<HashSet<String>>>,
}

impl std::fmt::Debug for SqliteEventStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteEventStore").finish_non_exhaustive()
    }
}

impl SqliteEventStore {
    pub fn new(config: SqliteConfig) -> Result<Self, SqliteEventStoreError> {
        let conn = rusqlite::Connection::open(&config.path)
            .map_err(SqliteEventStoreError::ConnectionFailed)?;

        if let Some(ref key) = config.encryption_key {
            conn.pragma_update(None, "key", key)
                .map_err(SqliteEventStoreError::ConnectionFailed)?;
        }

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(SqliteEventStoreError::ConnectionFailed)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            locks: Arc::new(RwLock::new(HashSet::new())),
        })
    }

    pub fn in_memory() -> Result<Self, SqliteEventStoreError> {
        let conn = rusqlite::Connection::open_in_memory()
            .map_err(SqliteEventStoreError::ConnectionFailed)?;

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(SqliteEventStoreError::ConnectionFailed)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            locks: Arc::new(RwLock::new(HashSet::new())),
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
        .expect("spawn_blocking panicked")
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
        .expect("spawn_blocking panicked")?;

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

            // Check all expected versions
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

            // Track per-stream next_version for assigning stream_version to each event
            let mut next_versions: HashMap<&StreamId, usize> = expected_versions
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
                let next_version = next_versions
                    .get_mut(stream_id)
                    .expect("stream must be registered");
                *next_version += 1;
                let version = *next_version;

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
        .expect("spawn_blocking panicked")
    }
}

impl EventReader for SqliteEventStore {
    type Error = EventStoreError;

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
        .expect("spawn_blocking panicked")?;

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

    async fn load(&self, name: &str) -> Result<Option<StreamPosition>, Self::Error> {
        let conn = self.conn.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
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
                    let uuid =
                        Uuid::parse_str(&pos_str).map_err(|e| {
                            SqliteCheckpointError::DatabaseError(
                                rusqlite::Error::FromSqlConversionFailure(
                                    0,
                                    rusqlite::types::Type::Text,
                                    Box::new(e),
                                ),
                            )
                        })?;
                    Ok(Some(StreamPosition::new(uuid)))
                }
                None => Ok(None),
            }
        })
        .await
        .expect("spawn_blocking panicked")
    }

    async fn save(&self, name: &str, position: StreamPosition) -> Result<(), Self::Error> {
        let conn = self.conn.clone();
        let name = name.to_string();
        let position_str = position.into_inner().to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT INTO eventcore_subscription_versions (subscription_name, last_position, updated_at)
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ON CONFLICT (subscription_name) DO UPDATE SET last_position = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![name, position_str],
            )
            .map_err(SqliteCheckpointError::DatabaseError)?;
            Ok(())
        })
        .await
        .expect("spawn_blocking panicked")
    }
}

impl ProjectorCoordinator for SqliteEventStore {
    type Error = SqliteCoordinationError;
    type Guard = SqliteCoordinationGuard;

    async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
        let mut guard = self.locks.write().await;

        if guard.contains(subscription_name) {
            return Err(SqliteCoordinationError::LeadershipNotAcquired {
                subscription_name: subscription_name.to_string(),
            });
        }

        guard.insert(subscription_name.to_string());

        Ok(SqliteCoordinationGuard {
            subscription_name: subscription_name.to_string(),
            locks: Arc::clone(&self.locks),
        })
    }
}

#[derive(Debug)]
pub struct SqliteCoordinationGuard {
    subscription_name: String,
    locks: Arc<RwLock<HashSet<String>>>,
}

impl Drop for SqliteCoordinationGuard {
    fn drop(&mut self) {
        // Try to remove the lock. If we can't get the write lock, the lock set
        // will be cleaned up when the RwLock is dropped.
        let locks = self.locks.clone();
        let name = self.subscription_name.clone();

        // Use try_write to avoid blocking in drop
        if let Ok(mut guard) = locks.try_write() {
            guard.remove(&name);
        } else {
            // Spawn a task to clean up asynchronously
            tokio::spawn(async move {
                let mut guard = locks.write().await;
                guard.remove(&name);
            });
        }
    }
}

// Standalone checkpoint store
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
        let conn = rusqlite::Connection::open(&config.path)
            .map_err(SqliteEventStoreError::ConnectionFailed)?;

        if let Some(ref key) = config.encryption_key {
            conn.pragma_update(None, "key", key)
                .map_err(SqliteEventStoreError::ConnectionFailed)?;
        }

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(SqliteEventStoreError::ConnectionFailed)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn in_memory() -> Result<Self, SqliteEventStoreError> {
        let conn = rusqlite::Connection::open_in_memory()
            .map_err(SqliteEventStoreError::ConnectionFailed)?;

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(SqliteEventStoreError::ConnectionFailed)?;

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
        .expect("spawn_blocking panicked")
    }
}

impl CheckpointStore for SqliteCheckpointStore {
    type Error = SqliteCheckpointError;

    async fn load(&self, name: &str) -> Result<Option<StreamPosition>, Self::Error> {
        let conn = self.conn.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
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
                    let uuid =
                        Uuid::parse_str(&pos_str).map_err(|e| {
                            SqliteCheckpointError::DatabaseError(
                                rusqlite::Error::FromSqlConversionFailure(
                                    0,
                                    rusqlite::types::Type::Text,
                                    Box::new(e),
                                ),
                            )
                        })?;
                    Ok(Some(StreamPosition::new(uuid)))
                }
                None => Ok(None),
            }
        })
        .await
        .expect("spawn_blocking panicked")
    }

    async fn save(&self, name: &str, position: StreamPosition) -> Result<(), Self::Error> {
        let conn = self.conn.clone();
        let name = name.to_string();
        let position_str = position.into_inner().to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            conn.execute(
                "INSERT INTO eventcore_subscription_versions (subscription_name, last_position, updated_at)
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 ON CONFLICT (subscription_name) DO UPDATE SET last_position = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![name, position_str],
            )
            .map_err(SqliteCheckpointError::DatabaseError)?;
            Ok(())
        })
        .await
        .expect("spawn_blocking panicked")
    }
}

// Standalone projector coordinator
#[derive(Debug, Clone, Default)]
pub struct SqliteProjectorCoordinator {
    locks: Arc<RwLock<HashSet<String>>>,
}

impl SqliteProjectorCoordinator {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProjectorCoordinator for SqliteProjectorCoordinator {
    type Error = SqliteCoordinationError;
    type Guard = SqliteCoordinationGuard;

    async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
        let mut guard = self.locks.write().await;

        if guard.contains(subscription_name) {
            return Err(SqliteCoordinationError::LeadershipNotAcquired {
                subscription_name: subscription_name.to_string(),
            });
        }

        guard.insert(subscription_name.to_string());

        Ok(SqliteCoordinationGuard {
            subscription_name: subscription_name.to_string(),
            locks: Arc::clone(&self.locks),
        })
    }
}
