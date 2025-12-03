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
use tracing::{info, instrument, warn};
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

    /// Test-only constructor that exists solely for mutation testing and integration harnesses.
    pub fn from_pool_for_tests(pool: Pool<Postgres>) -> Self {
        Self { pool }
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
        info!(
            stream_count = expected_versions.len(),
            "[postgres.append_events] appending events to postgres"
        );

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
                        warn!(
                            stream = %stream_id,
                            expected = expected_version.into_inner(),
                            actual = current,
                            "[postgres.version_conflict] optimistic concurrency mismatch detected"
                        );
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
                    .map_err(|error| map_insert_error(error, &stream_id))?;
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
    if let sqlx::Error::Database(db_error) = &error
        && let Some(code) = db_error.code().as_deref()
    {
        return if code == "23505" {
            EventStoreError::VersionConflict
        } else {
            EventStoreError::StoreFailure { operation }
        };
    }

    EventStoreError::StoreFailure { operation }
}

fn map_insert_error(error: sqlx::Error, stream_id: &StreamId) -> EventStoreError {
    if let sqlx::Error::Database(db_error) = &error
        && db_error.code().as_deref() == Some("23505")
    {
        warn!(
            stream = %stream_id,
            "[postgres.version_conflict] unique constraint detected during insert"
        );
        return EventStoreError::VersionConflict;
    }

    map_sqlx_error(error, "append_events")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{Executor, postgres::PgPoolOptions};
    use std::env;
    #[allow(unused_imports)]
    use tokio::test;
    use uuid::Uuid;

    fn postgres_connection_string() -> String {
        env::var("EVENTCORE_TEST_POSTGRES_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                "postgres://postgres:postgres@localhost:5433/eventcore_test".to_string()
            })
    }

    #[tokio::test]
    async fn map_sqlx_error_translates_unique_constraint_violations() {
        // Given: Developer has a table with a unique constraint to trigger duplicates
        let connection_string = postgres_connection_string();
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&connection_string)
            .await
            .expect("postgres test database should be reachable for map_sqlx_error tests");
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
        let mapped_error = map_sqlx_error(duplicate_error, "append_events");

        // Then: Developer sees version conflict error for 23505 violations
        assert!(
            matches!(mapped_error, EventStoreError::VersionConflict),
            "unique constraint violations should map to version conflict"
        );
    }
}
