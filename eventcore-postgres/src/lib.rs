use std::time::Duration;

use eventcore::{
    Event, EventStore, EventStoreError, EventStreamReader, EventStreamSlice, StreamId,
    StreamWriteEntry, StreamWrites,
};
use serde_json::{Value, json};
use sqlx::types::Json;
use sqlx::{Pool, Postgres, Row, postgres::PgPoolOptions, query, query_scalar};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum PostgresEventStoreError {
    #[error("failed to create postgres connection pool")]
    ConnectionFailed(#[source] sqlx::Error),
    #[error("failed to ping postgres")]
    PingFailed(#[source] sqlx::Error),
    #[error("failed to apply postgres migrations")]
    MigrationFailed(#[source] sqlx::migrate::MigrateError),
}

#[derive(Debug, Clone)]
pub struct PostgresEventStore {
    pool: Pool<Postgres>,
}

impl PostgresEventStore {
    pub async fn new<S: Into<String>>(
        connection_string: S,
    ) -> Result<Self, PostgresEventStoreError> {
        let connection_string = connection_string.into();
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&connection_string)
            .await
            .map_err(PostgresEventStoreError::ConnectionFailed)?;
        Ok(Self { pool })
    }

    pub async fn ping(&self) -> Result<(), PostgresEventStoreError> {
        query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(PostgresEventStoreError::PingFailed)
    }

    pub async fn migrate(&self) -> Result<(), PostgresEventStoreError> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(PostgresEventStoreError::MigrationFailed)
    }
}

impl EventStore for PostgresEventStore {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStreamReader<E>, EventStoreError> {
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

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        let expected_versions = writes.expected_versions().clone();
        let mut connection = self
            .pool
            .acquire()
            .await
            .map_err(|error| map_sqlx_error(error, "acquire_connection"))?;

        query("BEGIN")
            .execute(&mut *connection)
            .await
            .map_err(|error| map_sqlx_error(error, "begin_transaction"))?;

        let append_result = {
            let connection = &mut connection;
            async move {
                for (stream_id, expected_version) in &expected_versions {
                    let current_version: Option<i64> = query_scalar(
                        "SELECT stream_version FROM eventcore_events WHERE stream_id = $1 ORDER BY stream_version DESC LIMIT 1 FOR UPDATE",
                    )
                    .bind(stream_id.as_ref())
                    .fetch_optional(&mut **connection)
                    .await
                    .map_err(|error| map_sqlx_error(error, "select_stream_version"))?;

                    let current = current_version.unwrap_or(0);
                    if current as usize != expected_version.into_inner() {
                        return Err(EventStoreError::VersionConflict);
                    }
                }

                let mut version_counters: HashMap<StreamId, usize> = expected_versions
                    .iter()
                    .map(|(stream_id, version)| (stream_id.clone(), version.into_inner()))
                    .collect();

                for entry in writes.into_entries() {
                    let StreamWriteEntry {
                        stream_id,
                        event_type,
                        event_data,
                        ..
                    } = entry;

                    let counter = version_counters
                        .get_mut(&stream_id)
                        .expect("stream should have registered expected version");
                    *counter += 1;
                    let version = *counter as i64;

                    let event_id = Uuid::now_v7();
                    query(
                        "INSERT INTO eventcore_events (event_id, stream_id, stream_version, event_type, event_data, metadata)
                         VALUES ($1, $2, $3, $4, $5, $6)",
                    )
                    .bind(event_id)
                    .bind(stream_id.as_ref())
                    .bind(version)
                    .bind(event_type)
                    .bind(Json(event_data))
                    .bind(Json(json!({})))
                    .execute(&mut **connection)
                    .await
                    .map_err(map_insert_error)?;
                }

                Ok(EventStreamSlice)
            }
        }
        .await;

        match append_result {
            Ok(slice) => {
                query("COMMIT")
                    .execute(&mut *connection)
                    .await
                    .map_err(|error| map_sqlx_error(error, "commit_transaction"))?;
                Ok(slice)
            }
            Err(error) => {
                let _ = query("ROLLBACK").execute(&mut *connection).await;
                Err(error)
            }
        }
    }
}

fn map_sqlx_error(error: sqlx::Error, operation: &'static str) -> EventStoreError {
    match &error {
        sqlx::Error::Database(db_error) if db_error.code().is_some() => {
            if db_error.code().as_deref() == Some("23505") {
                EventStoreError::VersionConflict
            } else {
                EventStoreError::StoreFailure { operation }
            }
        }
        _ => EventStoreError::StoreFailure { operation },
    }
}

fn map_insert_error(error: sqlx::Error) -> EventStoreError {
    if let sqlx::Error::Database(db_error) = &error
        && db_error.code().as_deref() == Some("23505")
    {
        return EventStoreError::VersionConflict;
    }

    map_sqlx_error(error, "append_events")
}
