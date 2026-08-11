//! PostgreSQL event store backend for EventCore.
//!
//! This crate provides a production-grade [`PostgresEventStore`] backed by PostgreSQL,
//! with ACID transactions, advisory locks, and connection pooling via `sqlx`.
//!
//! Event immutability is enforced by PostgreSQL triggers. Stream pattern matching
//! follows the conventions described in ADR-0047.
//!
//! `PostgresEventStore` implements [`EventStore`], [`EventReader`], [`CheckpointStore`],
//! and [`ProjectorCoordinator`] from `eventcore-types`. Its normal migration
//! creates the durable command-state snapshot table used by snapshot-enabled
//! commands.
//!
//! # Getting Started
//!
//! ```no_run
//! use eventcore_postgres::PostgresEventStore;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let store = PostgresEventStore::new("postgres://user:pass@localhost/mydb").await?;
//! # Ok(())
//! # }
//! ```

use std::time::Duration;

use eventcore_types::{
    CheckpointStore, CommandStateReplayCheckpoint, CommandStateSnapshot, CommandStateSnapshotId,
    Event, EventFilter, EventPage, EventReader, EventStore, EventStoreError, EventStream,
    EventStreamSlice, Operation, ProjectorCoordinator, StreamId, StreamPosition, StreamVersion,
    StreamWrites,
};
use futures::StreamExt;
use nutype::nutype;
use serde_json::value::RawValue;
use serde_json::{Value, json};
use sqlx::types::Json;
use sqlx::{Pool, Postgres, QueryBuilder, Row, postgres::PgPoolOptions, query};
use thiserror::Error;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

/// Errors that can occur when creating a [`PostgresEventStore`].
#[derive(Debug, Error)]
pub enum PostgresEventStoreError {
    /// Failed to create the postgres connection pool.
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
/// ```no_run
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

/// PostgreSQL-backed event store implementing EventCore's storage traits.
///
/// Provides atomic multi-stream event writes with optimistic concurrency control,
/// advisory locks for projector coordination, and connection pooling.
///
/// Use [`PostgresEventStore::new`] (or [`PostgresEventStore::with_config`]) with a
/// PostgreSQL connection URL to create an instance.
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

    /// Verify connectivity by issuing a trivial `SELECT 1` query.
    ///
    /// # Panics
    ///
    /// Panics if the database is unreachable or the query fails.
    #[cfg_attr(test, mutants::skip)] // infallible: panics on failure
    pub async fn ping(&self) {
        let _ = query("SELECT 1")
            .execute(&self.pool)
            .await
            .expect("postgres ping failed");
    }

    /// Apply the bundled schema migrations via `sqlx::migrate!("./migrations")`.
    ///
    /// This creates the tables EventCore requires, including the
    /// `eventcore_subscription_versions` table used by [`PostgresCheckpointStore`].
    ///
    /// # Panics
    ///
    /// Panics if applying the migrations fails.
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
    ) -> Result<EventStream<E>, EventStoreError> {
        info!(
            stream = %stream_id,
            "[postgres.read_stream] reading events from postgres"
        );

        // Clone the pool (an `Arc` internally) so the returned stream owns its
        // connection handle and can be `'static`. The query uses sqlx's lazy
        // `fetch`, which pulls rows from the database incrementally rather than
        // buffering the entire result set with `fetch_all` — the real memory
        // win behind #364 for large streams.
        let pool = self.pool.clone();

        let stream = async_stream::stream! {
            let mut rows = query(
                "SELECT event_data FROM eventcore_events WHERE stream_id = $1 ORDER BY stream_version ASC",
            )
            .bind(stream_id.as_ref())
            .fetch(&pool);

            while let Some(row) = rows.next().await {
                let row = match row {
                    Ok(row) => row,
                    Err(error) => {
                        yield Err(map_sqlx_error(error, Operation::ReadStream));
                        break;
                    }
                };

                let payload: Value = match row.try_get("event_data") {
                    Ok(payload) => payload,
                    Err(error) => {
                        yield Err(map_sqlx_error(error, Operation::ReadStream));
                        break;
                    }
                };

                // A per-row decode failure surfaces as an `Err` item, matching
                // the previous behavior where a type mismatch failed the read
                // (see the read_stream_errors_on_type_mismatch contract test).
                match serde_json::from_value::<E>(payload) {
                    Ok(event) => yield Ok(event),
                    Err(error) => {
                        yield Err(EventStoreError::DeserializationFailed {
                            stream_id: stream_id.clone(),
                            detail: error.to_string(),
                        });
                        break;
                    }
                }
            }
        };

        Ok(EventStream::new(stream))
    }

    #[instrument(name = "postgres.read_stream_after", skip(self))]
    async fn read_stream_after<E: Event>(
        &self,
        stream_id: StreamId,
        exclusive_version: StreamVersion,
    ) -> Result<EventStream<E>, EventStoreError> {
        let pool = self.pool.clone();
        let version: usize = exclusive_version.into();

        let stream = async_stream::stream! {
            let mut rows = query(
                "SELECT event_data FROM eventcore_events \
                 WHERE stream_id = $1 AND stream_version > $2 \
                 ORDER BY stream_version ASC",
            )
            .bind(stream_id.as_ref())
            .bind(version as i64)
            .fetch(&pool);

            while let Some(row) = rows.next().await {
                let row = match row {
                    Ok(row) => row,
                    Err(error) => {
                        yield Err(map_sqlx_error(error, Operation::ReadStream));
                        break;
                    }
                };

                let payload: Value = match row.try_get("event_data") {
                    Ok(payload) => payload,
                    Err(error) => {
                        yield Err(map_sqlx_error(error, Operation::ReadStream));
                        break;
                    }
                };

                match serde_json::from_value::<E>(payload) {
                    Ok(event) => yield Ok(event),
                    Err(error) => {
                        yield Err(EventStoreError::DeserializationFailed {
                            stream_id: stream_id.clone(),
                            detail: error.to_string(),
                        });
                        break;
                    }
                }
            }
        };

        Ok(EventStream::new(stream))
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
        let _ = query("SELECT set_config('eventcore.expected_versions', $1, true)")
            .bind(expected_versions_json.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|error| map_sqlx_error(error, Operation::SetExpectedVersions))?;

        // Insert all events with a single multi-row INSERT per chunk. The
        // BEFORE INSERT trigger still assigns gap-free stream versions and
        // enforces optimistic concurrency for each row in VALUES order,
        // exactly as it did for the previous per-event loop — replacing N
        // round-trips with one statement. Chunking keeps the bound-parameter
        // count well under Postgres' 65535-parameter limit (5 binds/event).
        // Drop the type-erased event payload (only the in-memory store needs
        // it) and keep just the Send + Sync fields used for SQL binding, so the
        // borrows held by the insert loop stay Send across the awaits.
        let rows: Vec<(StreamId, &'static str, Box<RawValue>)> = entries
            .into_iter()
            .map(|entry| (entry.stream_id, entry.event_type, entry.event_data))
            .collect();

        const MAX_EVENTS_PER_INSERT: usize = 1000;
        for chunk in rows.chunks(MAX_EVENTS_PER_INSERT) {
            let mut builder = QueryBuilder::<Postgres>::new(
                "INSERT INTO eventcore_events (event_id, stream_id, event_type, event_data, metadata) ",
            );
            let _ = builder.push_values(chunk, |mut row, (stream_id, event_type, event_data)| {
                let _ = row
                    .push_bind(Uuid::now_v7())
                    .push_bind(stream_id.as_ref())
                    .push_bind(*event_type)
                    .push_bind(Json(event_data))
                    .push_bind(Json(json!({})));
            });
            let _ = builder
                .build()
                .execute(&mut *tx)
                .await
                .map_err(|error| map_sqlx_error(error, Operation::AppendEvents))?;
        }

        tx.commit()
            .await
            .map_err(|error| map_sqlx_error(error, Operation::CommitTransaction))?;

        Ok(EventStreamSlice)
    }

    async fn load_command_state_snapshot(
        &self,
        snapshot_id: CommandStateSnapshotId,
    ) -> Result<Option<CommandStateSnapshot>, EventStoreError> {
        let row = query(
            "SELECT state, stream_versions, replay_checkpoints \
             FROM eventcore_command_state_snapshots \
             WHERE snapshot_id = $1",
        )
        .bind(snapshot_id.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, Operation::ReadStream))?;

        row.map(|row| {
            let state: Json<Value> = row
                .try_get("state")
                .map_err(|error| map_sqlx_error(error, Operation::ReadStream))?;
            let stream_versions: Json<std::collections::HashMap<StreamId, StreamVersion>> = row
                .try_get("stream_versions")
                .map_err(|error| map_sqlx_error(error, Operation::ReadStream))?;
            let replay_checkpoints: Json<Vec<CommandStateReplayCheckpoint>> = row
                .try_get("replay_checkpoints")
                .map_err(|error| map_sqlx_error(error, Operation::ReadStream))?;

            Ok(CommandStateSnapshot::new(state.0, stream_versions.0)
                .with_replay_checkpoints(replay_checkpoints.0))
        })
        .transpose()
    }

    async fn save_command_state_snapshot(
        &self,
        snapshot_id: CommandStateSnapshotId,
        snapshot: CommandStateSnapshot,
    ) -> Result<(), EventStoreError> {
        // The conflict predicate is evaluated while PostgreSQL holds the
        // conflicting row lock. This means concurrent refreshes can never let
        // a stale vector replace one that includes more events.
        let _ = query(
            "INSERT INTO eventcore_command_state_snapshots \
                 (snapshot_id, state, stream_versions, replay_checkpoints, updated_at) \
             VALUES ($1, $2, $3, $4, NOW()) \
             ON CONFLICT (snapshot_id) DO UPDATE \
             SET state = EXCLUDED.state, \
                 stream_versions = EXCLUDED.stream_versions, \
                 replay_checkpoints = EXCLUDED.replay_checkpoints, \
                 updated_at = NOW() \
             WHERE NOT EXISTS ( \
                 SELECT 1 \
                 FROM jsonb_each_text(eventcore_command_state_snapshots.stream_versions) \
                     AS current(stream_id, stream_version) \
                 WHERE NOT (EXCLUDED.stream_versions ? current.stream_id) \
                    OR (EXCLUDED.stream_versions ->> current.stream_id)::BIGINT \
                       < current.stream_version::BIGINT \
             )",
        )
        .bind(snapshot_id.as_ref())
        .bind(Json(snapshot.state))
        .bind(Json(snapshot.stream_versions))
        .bind(Json(snapshot.replay_checkpoints))
        .execute(&self.pool)
        .await
        .map_err(|error| map_sqlx_error(error, Operation::AppendEvents))?;

        Ok(())
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
        let _ = query(
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

        // Filter by event_type in SQL so non-matching types don't consume
        // batch slots (fixes issue #372). Use explicit filter if set,
        // otherwise derive from E::event_type_name().
        let type_filter = filter.event_type().unwrap_or_else(|| E::event_type_name());

        // Glob pattern pushdown translates the pattern to an anchored POSIX
        // regex matched with the `~` operator (ADR-0047). The translation
        // escapes all regex metacharacters in literal segments so user input
        // cannot inject regex syntax.
        let pattern_regex = filter
            .stream_pattern()
            .map(|p| glob_to_anchored_regex(p.as_ref()));

        // Build the query dynamically so prefix XOR pattern, the optional
        // cursor, and the event_type predicate compose without a combinatorial
        // explosion of hand-written query strings.
        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT event_id, event_data, stream_id FROM eventcore_events WHERE event_type = ",
        );
        let _ = builder.push_bind(type_filter);

        if let Some(after_id) = after_event_id {
            let _ = builder.push(" AND event_id > ").push_bind(after_id);
        }

        if let Some(prefix) = filter.stream_prefix() {
            let _ = builder
                .push(" AND stream_id LIKE ")
                .push_bind(prefix.as_ref().to_string())
                .push(" || '%'");
        } else if let Some(regex) = pattern_regex {
            let _ = builder.push(" AND stream_id ~ ").push_bind(regex);
        }

        let _ = builder.push(" ORDER BY event_id LIMIT ").push_bind(limit);

        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
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

/// Translate a POSIX glob pattern into an anchored POSIX regular expression
/// suitable for PostgreSQL's `~` operator (ADR-0047).
///
/// Mapping:
/// - `*` → `.*` (matches any sequence, including the `/` separator)
/// - `?` → `.` (matches exactly one character)
/// - `[...]` / `[!...]` → a regex character class (`[!` is normalized to the
///   regex negation `[^`); the class contents are copied verbatim so ranges
///   like `[0-9]` and `[a-z]` work, while a closing `]` ends the class
/// - every other character is a literal and is regex-escaped
///
/// The result is anchored with `^...$` so the whole stream ID must match,
/// mirroring `glob::Pattern::matches`. Because the pattern has already been
/// validated as a compilable `glob::Pattern`, brackets are balanced; a stray
/// `[` (impossible for a valid pattern) is treated as a literal.
fn glob_to_anchored_regex(glob: &str) -> String {
    let mut regex = String::with_capacity(glob.len() + 2);
    regex.push('^');

    let mut chars = glob.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '[' => {
                // Collect the bracket expression up to the closing ']'.
                let mut class = String::new();
                let mut closed = false;
                if matches!(chars.peek(), Some('!')) {
                    let _ = chars.next();
                    class.push('^');
                }
                for inner in chars.by_ref() {
                    if inner == ']' {
                        closed = true;
                        break;
                    }
                    class.push(inner);
                }
                if closed {
                    regex.push('[');
                    regex.push_str(&class);
                    regex.push(']');
                } else {
                    // Not a real class (a validated glob never reaches here);
                    // treat the '[' and collected chars as literals.
                    regex.push_str(&regex_escape("["));
                    regex.push_str(&regex_escape(&class));
                }
            }
            other => regex.push_str(&regex_escape(&other.to_string())),
        }
    }

    regex.push('$');
    regex
}

/// Escape every POSIX-regex metacharacter in a literal segment so it matches
/// itself, preventing regex injection from stream-pattern input.
fn regex_escape(literal: &str) -> String {
    const METACHARACTERS: &[char] = &[
        '.', '^', '$', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '\\',
    ];
    let mut escaped = String::with_capacity(literal.len());
    for c in literal.chars() {
        if METACHARACTERS.contains(&c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
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
            return parse_version_conflict_from_db_error(db_error.message());
        }
    }

    error!(
        error = %error,
        operation = %operation,
        "[postgres.database_error] database operation failed"
    );
    EventStoreError::StoreFailure { operation }
}

/// Parse version conflict details from the PostgreSQL trigger error message.
///
/// The trigger produces messages like:
///   `version_conflict: stream "my-stream" expected version 0, actual 1`
///
/// If parsing fails, falls back to a VersionConflict with a sentinel stream_id
/// indicating the details could not be extracted.
fn parse_version_conflict_from_db_error(message: &str) -> EventStoreError {
    // Pattern: version_conflict: stream "STREAM_ID" expected version EXPECTED, actual ACTUAL
    if let Some(parsed) = try_parse_conflict_message(message) {
        return parsed;
    }

    // Fallback: unique constraint violation (23505) or unparseable trigger message.
    // Use a sentinel stream_id since we don't have the details.
    let fallback_stream_id =
        StreamId::try_new("unknown-conflict-stream").expect("static stream id is valid");
    EventStoreError::VersionConflict {
        stream_id: fallback_stream_id,
        expected: StreamVersion::new(0),
        actual: StreamVersion::new(0),
    }
}

fn try_parse_conflict_message(message: &str) -> Option<EventStoreError> {
    let rest = message.strip_prefix("version_conflict: stream \"")?;
    let stream_end = rest.find('"')?;
    let stream_id_str = &rest[..stream_end];
    let after_stream = &rest[stream_end..];

    let expected_str = after_stream
        .strip_prefix("\" expected version ")?
        .split(',')
        .next()?;
    let actual_str = after_stream.rsplit("actual ").next()?;

    let expected = expected_str.trim().parse::<usize>().ok()?;
    let actual = actual_str.trim().parse::<usize>().ok()?;
    let stream_id = StreamId::try_new(stream_id_str).ok()?;

    Some(EventStoreError::VersionConflict {
        stream_id,
        expected: StreamVersion::new(expected),
        actual: StreamVersion::new(actual),
    })
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
        let _ = query(
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
    #[error(
        "leadership not acquired for subscription '{subscription_name}': another instance holds the lock"
    )]
    LeadershipNotAcquired { subscription_name: String },

    /// Database operation failed.
    #[error("database operation failed: {0}")]
    DatabaseError(#[source] sqlx::Error),
}

/// Guard type that releases leadership when dropped.
///
/// Holds the advisory lock key and the actual database connection that acquired
/// the lock. This is critical because PostgreSQL advisory locks are session-scoped:
/// the unlock must happen on the same connection that acquired the lock.
///
/// # Lock Release Behavior
///
/// The guard attempts to explicitly release the advisory lock when dropped:
///
/// - **Multi-threaded runtime**: Uses `block_in_place` to synchronously release
///   the lock before the guard is fully dropped.
///
/// - **Single-threaded runtime**: Spawns a task to release the lock asynchronously.
///   This task may not execute before process shutdown, in which case the lock is
///   released when the PostgreSQL session ends (connection closes).
///
/// # PostgreSQL Session-Scoped Locks
///
/// PostgreSQL advisory locks acquired with `pg_try_advisory_lock` are session-scoped
/// and automatically released when the database connection closes. This provides a
/// safety net: even if explicit unlock fails or is skipped, the lock will be released
/// when:
/// - The connection is returned to the pool and recycled
/// - The connection pool is shut down
/// - The database connection times out
///
/// For production deployments, configure appropriate connection pool idle timeouts
/// to ensure timely lock release on ungraceful shutdown.
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
                        if let Err(e) = query("SELECT pg_advisory_unlock($1)")
                            .bind(lock_key)
                            .execute(&mut *connection)
                            .await
                        {
                            warn!(
                                lock_key = lock_key,
                                error = %e,
                                "failed to release advisory lock on drop"
                            );
                        }
                        // Connection is returned to pool when dropped here
                    });
                });
            } else {
                // Single-threaded runtime: spawn a task for async unlock.
                // Note: This task may not execute before process shutdown. In that case,
                // the advisory lock is released when the PostgreSQL session ends (the
                // connection closes). See struct-level documentation for details.
                drop(tokio::spawn(async move {
                    if let Err(e) = query("SELECT pg_advisory_unlock($1)")
                        .bind(lock_key)
                        .execute(&mut *connection)
                        .await
                    {
                        warn!(
                            lock_key = lock_key,
                            error = %e,
                            "failed to release advisory lock on drop (async)"
                        );
                    }
                }));
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

/// Compute a stable FNV-1a hash of the subscription name to derive an advisory lock key.
///
/// This uses the FNV-1a algorithm (64-bit) which produces deterministic output
/// across Rust versions, unlike `DefaultHasher` which is explicitly not stable.
fn advisory_lock_key(subscription_name: &str) -> i64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in subscription_name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash as i64
}

/// Try to acquire a PostgreSQL advisory lock for the given subscription name
/// using the provided connection pool.
async fn try_acquire_advisory_lock(
    pool: &Pool<Postgres>,
    subscription_name: &str,
) -> Result<CoordinationGuard, CoordinationError> {
    let lock_key = advisory_lock_key(subscription_name);

    // Acquire a dedicated connection from the pool.
    // This connection MUST be kept for the lifetime of the guard because
    // PostgreSQL advisory locks are session-scoped.
    let mut connection = pool
        .acquire()
        .await
        .map_err(CoordinationError::DatabaseError)?;

    // Attempt to acquire advisory lock (non-blocking) on this specific connection
    let row = query("SELECT pg_try_advisory_lock($1)")
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
        Err(CoordinationError::LeadershipNotAcquired {
            subscription_name: subscription_name.to_string(),
        })
    }
}

impl ProjectorCoordinator for PostgresProjectorCoordinator {
    type Error = CoordinationError;
    type Guard = CoordinationGuard;

    async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
        try_acquire_advisory_lock(&self.pool, subscription_name).await
    }
}

impl ProjectorCoordinator for PostgresEventStore {
    type Error = CoordinationError;
    type Guard = CoordinationGuard;

    async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
        try_acquire_advisory_lock(&self.pool, subscription_name).await
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
