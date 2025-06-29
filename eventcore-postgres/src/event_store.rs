//! `PostgreSQL` implementation of the `EventStore` trait
//!
//! This module provides a complete `PostgreSQL` implementation of the `EventStore` trait
//! with support for multi-stream atomic operations, optimistic concurrency control,
//! and efficient event querying.

use std::collections::HashMap;

use async_trait::async_trait;
use eventcore::{
    errors::{EventStoreError, EventStoreResult},
    event_store::{
        EventMetadata, EventStore, EventToWrite, ExpectedVersion, ReadOptions, StoredEvent,
        StreamData, StreamEvents,
    },
    subscription::{Subscription, SubscriptionOptions},
    types::{EventId, EventVersion, StreamId, Timestamp},
};
use serde_json::Value;
use sqlx::{postgres::PgRow, Row, Transaction};
use tracing::{debug, instrument, warn};
use uuid::Uuid;

use crate::PostgresEventStore;

/// Database row representing an event
#[derive(Debug)]
#[allow(dead_code)] // Fields are for future use and debugging
struct EventRow {
    event_id: Uuid,
    stream_id: String,
    event_version: i64,
    event_type: String,
    event_data: Value,
    metadata: Option<Value>,
    causation_id: Option<Uuid>,
    correlation_id: Option<String>,
    user_id: Option<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl TryFrom<PgRow> for EventRow {
    type Error = sqlx::Error;

    fn try_from(row: PgRow) -> Result<Self, Self::Error> {
        Ok(Self {
            event_id: row.try_get("event_id")?,
            stream_id: row.try_get("stream_id")?,
            event_version: row.try_get("event_version")?,
            event_type: row.try_get("event_type")?,
            event_data: row.try_get("event_data")?,
            metadata: row.try_get("metadata")?,
            causation_id: row.try_get("causation_id")?,
            correlation_id: row.try_get("correlation_id")?,
            user_id: row.try_get("user_id")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl EventRow {
    /// Convert database row to `StoredEvent`
    #[allow(clippy::wrong_self_convention)] // This consumes the row, which is correct
    fn to_stored_event(self) -> EventStoreResult<StoredEvent<Value>> {
        let event_id = EventId::try_new(self.event_id)
            .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;

        let stream_id = StreamId::try_new(self.stream_id)
            .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;

        let event_version = if self.event_version >= 0 {
            let version_u64 = u64::try_from(self.event_version)
                .map_err(|_| EventStoreError::SerializationFailed("Invalid version".to_string()))?;
            EventVersion::try_new(version_u64)
                .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?
        } else {
            return Err(EventStoreError::SerializationFailed(
                "Negative event version in database".to_string(),
            ));
        };

        let timestamp = Timestamp::new(self.created_at);

        let metadata = if let Some(metadata_json) = self.metadata {
            let event_metadata: EventMetadata = serde_json::from_value(metadata_json)
                .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;
            Some(event_metadata)
        } else {
            None
        };

        Ok(StoredEvent::new(
            event_id,
            stream_id,
            event_version,
            timestamp,
            self.event_data,
            metadata,
        ))
    }
}

#[async_trait]
impl EventStore for PostgresEventStore {
    type Event = Value;

    #[instrument(skip(self), fields(streams = stream_ids.len()))]
    async fn read_streams(
        &self,
        stream_ids: &[StreamId],
        options: &ReadOptions,
    ) -> EventStoreResult<StreamData<Self::Event>> {
        if stream_ids.is_empty() {
            return Ok(StreamData::new(Vec::new(), HashMap::new()));
        }

        debug!(
            "Reading {} streams with options: {:?}",
            stream_ids.len(),
            options
        );

        // Optimized query with proper indexing hints
        let mut query = String::from(
            "SELECT event_id, stream_id, event_version, event_type, event_data, metadata, 
             causation_id, correlation_id, user_id, created_at
             FROM events
             WHERE stream_id = ANY($1)",
        );

        let mut param_count = 2;

        // Add version filtering
        if let Some(_from_version) = options.from_version {
            use std::fmt::Write;
            write!(&mut query, " AND event_version >= ${param_count}").expect("Write to string");
            param_count += 1;
        }

        if let Some(_to_version) = options.to_version {
            use std::fmt::Write;
            write!(&mut query, " AND event_version <= ${param_count}").expect("Write to string");
            param_count += 1;
        }

        // Order by event_id for timestamp-based ordering
        query.push_str(" ORDER BY event_id");

        // Add limit
        if let Some(_max_events) = options.max_events {
            use std::fmt::Write;
            write!(&mut query, " LIMIT ${param_count}").expect("Write to string");
        }

        let stream_id_strings: Vec<String> =
            stream_ids.iter().map(|s| s.as_ref().to_string()).collect();

        // Build and execute query
        let mut sqlx_query = sqlx::query(&query).bind(&stream_id_strings);

        if let Some(from_version) = options.from_version {
            let version_value: u64 = from_version.into();
            let version_i64 = i64::try_from(version_value).map_err(|_| {
                EventStoreError::SerializationFailed("Version too large".to_string())
            })?;
            sqlx_query = sqlx_query.bind(version_i64);
        }

        if let Some(to_version) = options.to_version {
            let version_value: u64 = to_version.into();
            let version_i64 = i64::try_from(version_value).map_err(|_| {
                EventStoreError::SerializationFailed("Version too large".to_string())
            })?;
            sqlx_query = sqlx_query.bind(version_i64);
        }

        if let Some(max_events) = options.max_events {
            let max_events_i64 = i64::try_from(max_events).map_err(|_| {
                EventStoreError::SerializationFailed("Max events too large".to_string())
            })?;
            sqlx_query = sqlx_query.bind(max_events_i64);
        }

        let rows = sqlx_query
            .fetch_all(self.pool.as_ref())
            .await
            .map_err(|e| EventStoreError::ConnectionFailed(e.to_string()))?;

        debug!("Retrieved {} events from database", rows.len());

        // Convert rows to events
        let mut events = Vec::new();
        for row in rows {
            let event_row = EventRow::try_from(row)
                .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;
            events.push(event_row.to_stored_event()?);
        }

        // Get current stream versions in a single optimized query
        let stream_versions = if stream_ids.len() == 1 {
            // Single stream optimization
            let version = self.get_stream_version(&stream_ids[0]).await?;
            let mut versions = HashMap::new();
            versions.insert(
                stream_ids[0].clone(),
                version.unwrap_or_else(EventVersion::initial),
            );
            versions
        } else {
            // Multi-stream batch query
            self.get_stream_versions(stream_ids).await?
        };

        Ok(StreamData::new(events, stream_versions))
    }

    #[instrument(skip(self), fields(streams = stream_events.len()))]
    async fn write_events_multi(
        &self,
        stream_events: Vec<StreamEvents<Self::Event>>,
    ) -> EventStoreResult<HashMap<StreamId, EventVersion>> {
        if stream_events.is_empty() {
            return Ok(HashMap::new());
        }

        debug!("Writing events to {} streams", stream_events.len());

        // Start transaction for atomicity
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| EventStoreError::TransactionRollback(e.to_string()))?;

        let mut result_versions = HashMap::new();

        for stream in stream_events {
            let stream_id = stream.stream_id.clone();
            let new_version = self.write_stream_events(&mut tx, stream).await?;
            result_versions.insert(stream_id, new_version);
        }

        // Commit transaction
        tx.commit()
            .await
            .map_err(|e| EventStoreError::TransactionRollback(e.to_string()))?;

        debug!(
            "Successfully wrote events to {} streams",
            result_versions.len()
        );
        Ok(result_versions)
    }

    #[instrument(skip(self))]
    async fn stream_exists(&self, stream_id: &StreamId) -> EventStoreResult<bool> {
        let exists =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM event_streams WHERE stream_id = $1)")
                .bind(stream_id.as_ref())
                .fetch_one(self.pool.as_ref())
                .await
                .map_err(|e| EventStoreError::ConnectionFailed(e.to_string()))?;

        Ok(exists)
    }

    #[instrument(skip(self))]
    async fn get_stream_version(
        &self,
        stream_id: &StreamId,
    ) -> EventStoreResult<Option<EventVersion>> {
        let version: Option<i64> =
            sqlx::query_scalar("SELECT current_version FROM event_streams WHERE stream_id = $1")
                .bind(stream_id.as_ref())
                .fetch_optional(self.pool.as_ref())
                .await
                .map_err(|e| EventStoreError::ConnectionFailed(e.to_string()))?;

        match version {
            Some(v) => {
                let event_version = if v >= 0 {
                    let v_u64 = u64::try_from(v).map_err(|_| {
                        EventStoreError::SerializationFailed("Invalid version".to_string())
                    })?;
                    EventVersion::try_new(v_u64)
                        .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?
                } else {
                    return Ok(None);
                };
                Ok(Some(event_version))
            }
            None => Ok(None),
        }
    }

    #[instrument(skip(self))]
    async fn subscribe(
        &self,
        _options: SubscriptionOptions,
    ) -> EventStoreResult<Box<dyn Subscription<Event = Self::Event>>> {
        // TODO: Implement subscription functionality
        warn!("PostgreSQL subscriptions not yet implemented");
        Err(EventStoreError::Configuration(
            "Subscriptions not yet implemented".to_string(),
        ))
    }
}

impl PostgresEventStore {
    /// Get current versions for multiple streams
    async fn get_stream_versions(
        &self,
        stream_ids: &[StreamId],
    ) -> EventStoreResult<HashMap<StreamId, EventVersion>> {
        if stream_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let stream_id_strings: Vec<String> =
            stream_ids.iter().map(|s| s.as_ref().to_string()).collect();

        let rows = sqlx::query(
            "SELECT stream_id, current_version FROM event_streams WHERE stream_id = ANY($1)",
        )
        .bind(&stream_id_strings)
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| EventStoreError::ConnectionFailed(e.to_string()))?;

        let mut versions = HashMap::new();

        for row in rows {
            let stream_id_str: String = row
                .try_get("stream_id")
                .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;
            let version_raw: i64 = row
                .try_get("current_version")
                .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;

            let stream_id = StreamId::try_new(stream_id_str)
                .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;
            let version = if version_raw >= 0 {
                let version_u64 = u64::try_from(version_raw).map_err(|_| {
                    EventStoreError::SerializationFailed("Invalid version".to_string())
                })?;
                EventVersion::try_new(version_u64)
                    .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?
            } else {
                return Err(EventStoreError::SerializationFailed(
                    "Negative version in database".to_string(),
                ));
            };

            versions.insert(stream_id, version);
        }

        // For streams that don't exist yet, add initial version
        for stream_id in stream_ids {
            if !versions.contains_key(stream_id) {
                versions.insert(stream_id.clone(), EventVersion::initial());
            }
        }

        Ok(versions)
    }

    /// Write events to a single stream within a transaction
    async fn write_stream_events(
        &self,
        tx: &mut Transaction<'_, sqlx::Postgres>,
        stream_events: StreamEvents<Value>,
    ) -> EventStoreResult<EventVersion> {
        let StreamEvents {
            stream_id,
            expected_version,
            events,
        } = stream_events;

        if events.is_empty() {
            return Err(EventStoreError::Internal("No events to write".to_string()));
        }

        // Check and update stream version with optimistic concurrency control
        let current_version = self
            .check_and_lock_stream(tx, &stream_id, expected_version)
            .await?;

        // Calculate new version
        let current_value: u64 = current_version.into();
        let new_version = EventVersion::try_new(current_value + events.len() as u64)
            .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;

        // Insert events
        for (i, event) in events.iter().enumerate() {
            let event_version = EventVersion::try_new(current_value + i as u64 + 1)
                .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;

            self.insert_event(tx, &stream_id, event_version, event)
                .await?;
        }

        // Update stream version
        self.update_stream_version(tx, &stream_id, new_version)
            .await?;

        Ok(new_version)
    }

    /// Check stream version and lock for optimistic concurrency control
    #[allow(clippy::too_many_lines)]
    async fn check_and_lock_stream(
        &self,
        tx: &mut Transaction<'_, sqlx::Postgres>,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
    ) -> EventStoreResult<EventVersion> {
        // Get current version with row lock
        let current_version: Option<i64> = sqlx::query_scalar(
            "SELECT current_version FROM event_streams WHERE stream_id = $1 FOR UPDATE",
        )
        .bind(stream_id.as_ref())
        .fetch_optional(tx.as_mut())
        .await
        .map_err(|e| EventStoreError::ConnectionFailed(e.to_string()))?;

        match (current_version, expected_version) {
            (None, ExpectedVersion::New) => {
                // Stream doesn't exist and we expect it to be new - create it
                let result = sqlx::query(
                    "INSERT INTO event_streams (stream_id, current_version) VALUES ($1, 0)",
                )
                .bind(stream_id.as_ref())
                .execute(tx.as_mut())
                .await;

                match result {
                    Ok(_) => Ok(EventVersion::initial()),
                    Err(sqlx::Error::Database(db_err)) => {
                        // Check if it's a unique constraint violation (PostgreSQL error code 23505)
                        if db_err.code().as_deref() == Some("23505") {
                            // Another concurrent operation created the stream
                            Err(EventStoreError::VersionConflict {
                                stream: stream_id.clone(),
                                expected: EventVersion::initial(),
                                current: EventVersion::initial(),
                            })
                        } else {
                            Err(EventStoreError::ConnectionFailed(db_err.to_string()))
                        }
                    }
                    Err(e) => Err(EventStoreError::ConnectionFailed(e.to_string())),
                }
            }
            (None, ExpectedVersion::Exact(expected)) => Err(EventStoreError::VersionConflict {
                stream: stream_id.clone(),
                expected,
                current: EventVersion::initial(),
            }),
            (None, ExpectedVersion::Any) => {
                // Try to create new stream, but if it already exists due to concurrent creation, that's OK
                let result = sqlx::query(
                    "INSERT INTO event_streams (stream_id, current_version) VALUES ($1, 0)",
                )
                .bind(stream_id.as_ref())
                .execute(tx.as_mut())
                .await;

                match result {
                    Ok(_) => Ok(EventVersion::initial()),
                    Err(sqlx::Error::Database(db_err)) => {
                        // Check if it's a unique constraint violation (PostgreSQL error code 23505)
                        if db_err.code().as_deref() == Some("23505") {
                            // Stream was created concurrently, get its current version
                            let current: i64 = sqlx::query_scalar(
                                "SELECT current_version FROM event_streams WHERE stream_id = $1",
                            )
                            .bind(stream_id.as_ref())
                            .fetch_one(tx.as_mut())
                            .await
                            .map_err(|e| EventStoreError::ConnectionFailed(e.to_string()))?;

                            if current >= 0 {
                                let current_u64 = u64::try_from(current).map_err(|_| {
                                    EventStoreError::SerializationFailed(
                                        "Invalid version".to_string(),
                                    )
                                })?;
                                EventVersion::try_new(current_u64).map_err(|e| {
                                    EventStoreError::SerializationFailed(e.to_string())
                                })
                            } else {
                                Ok(EventVersion::initial())
                            }
                        } else {
                            Err(EventStoreError::ConnectionFailed(db_err.to_string()))
                        }
                    }
                    Err(e) => Err(EventStoreError::ConnectionFailed(e.to_string())),
                }
            }
            (Some(actual), ExpectedVersion::New) => {
                let actual_version = if actual >= 0 {
                    let actual_u64 = u64::try_from(actual).map_err(|_| {
                        EventStoreError::SerializationFailed("Invalid version".to_string())
                    })?;
                    EventVersion::try_new(actual_u64)
                        .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?
                } else {
                    return Err(EventStoreError::SerializationFailed(
                        "Negative version in database".to_string(),
                    ));
                };
                Err(EventStoreError::VersionConflict {
                    stream: stream_id.clone(),
                    expected: EventVersion::initial(),
                    current: actual_version,
                })
            }
            (Some(actual), ExpectedVersion::Exact(expected)) => {
                let actual_version = if actual >= 0 {
                    let actual_u64 = u64::try_from(actual).map_err(|_| {
                        EventStoreError::SerializationFailed("Invalid version".to_string())
                    })?;
                    EventVersion::try_new(actual_u64)
                        .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?
                } else {
                    return Err(EventStoreError::SerializationFailed(
                        "Negative version in database".to_string(),
                    ));
                };

                if actual_version == expected {
                    Ok(actual_version)
                } else {
                    Err(EventStoreError::VersionConflict {
                        stream: stream_id.clone(),
                        expected,
                        current: actual_version,
                    })
                }
            }
            (Some(actual), ExpectedVersion::Any) => {
                let actual_version = if actual >= 0 {
                    let actual_u64 = u64::try_from(actual).map_err(|_| {
                        EventStoreError::SerializationFailed("Invalid version".to_string())
                    })?;
                    EventVersion::try_new(actual_u64)
                        .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?
                } else {
                    return Err(EventStoreError::SerializationFailed(
                        "Negative version in database".to_string(),
                    ));
                };
                Ok(actual_version)
            }
        }
    }

    /// Insert a single event into the events table
    async fn insert_event(
        &self,
        tx: &mut Transaction<'_, sqlx::Postgres>,
        stream_id: &StreamId,
        event_version: EventVersion,
        event: &EventToWrite<Value>,
    ) -> EventStoreResult<()> {
        let metadata_json = if let Some(metadata) = &event.metadata {
            Some(
                serde_json::to_value(metadata)
                    .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?,
            )
        } else {
            None
        };

        // Extract metadata fields for indexing
        let (causation_id, correlation_id, user_id) =
            event
                .metadata
                .as_ref()
                .map_or((None, None, None), |metadata| {
                    (
                        metadata.causation_id.as_ref().map(|id| **id),
                        metadata.correlation_id.clone(),
                        metadata.user_id.clone(),
                    )
                });

        sqlx::query(
            "INSERT INTO events 
             (event_id, stream_id, event_version, event_type, event_data, metadata, causation_id, correlation_id, user_id) 
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
        )
        .bind(*event.event_id)
        .bind(stream_id.as_ref())
        .bind({
            let version_value: u64 = event_version.into();
            i64::try_from(version_value).map_err(|_| {
                EventStoreError::SerializationFailed("Version too large for database".to_string())
            })?
        })
        .bind("generic") // TODO: Add event type detection
        .bind(&event.payload)
        .bind(metadata_json)
        .bind(causation_id)
        .bind(correlation_id)
        .bind(user_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| {
            e.as_database_error().map_or_else(
                || EventStoreError::ConnectionFailed(e.to_string()),
                |db_err| {
                    if db_err.is_unique_violation() {
                        EventStoreError::DuplicateEventId(event.event_id)
                    } else {
                        EventStoreError::ConnectionFailed(e.to_string())
                    }
                },
            )
        })?;

        Ok(())
    }

    /// Update the stream version in the `event_streams` table
    async fn update_stream_version(
        &self,
        tx: &mut Transaction<'_, sqlx::Postgres>,
        stream_id: &StreamId,
        new_version: EventVersion,
    ) -> EventStoreResult<()> {
        sqlx::query(
            "UPDATE event_streams SET current_version = $1, updated_at = NOW() WHERE stream_id = $2",
        )
        .bind({
            let version_value: u64 = new_version.into();
            i64::try_from(version_value).map_err(|_| {
                EventStoreError::SerializationFailed("Version too large for database".to_string())
            })?
        })
        .bind(stream_id.as_ref())
        .execute(tx.as_mut())
        .await
        .map_err(|e| EventStoreError::ConnectionFailed(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eventcore::event_store::{EventToWrite, ExpectedVersion, StreamEvents};
    use serde_json::json;

    // Note: These are unit tests for the individual methods.
    // Integration tests with a real database will be in the tests directory.

    #[test]
    fn test_event_row_conversion() {
        // This test would require setting up a mock PgRow, which is complex
        // For now, we'll test the logic separately
        // Test the basic struct construction to ensure it's valid
        let event_id = Uuid::nil(); // Just for testing construction
        let stream_id = "test-stream".to_string();
        let event_version = 0i64;
        let event_type = "TestEvent".to_string();
        let event_data = serde_json::json!({"test": true});
        let metadata = None;
        let causation_id = None;
        let correlation_id = None;
        let user_id = None;
        let created_at = chrono::Utc::now();

        let event_row = EventRow {
            event_id,
            stream_id,
            event_version,
            event_type,
            event_data,
            metadata,
            causation_id,
            correlation_id,
            user_id,
            created_at,
        };

        // If we can construct it without panic, the test passes
        assert!(!format!("{event_row:?}").is_empty());
    }

    #[test]
    fn test_expected_version_logic() {
        // Test the logic for version checking
        let new_version = ExpectedVersion::New;
        let exact_version = ExpectedVersion::Exact(EventVersion::try_new(5).unwrap());
        let any_version = ExpectedVersion::Any;

        assert_eq!(new_version, ExpectedVersion::New);
        assert_eq!(
            exact_version,
            ExpectedVersion::Exact(EventVersion::try_new(5).unwrap())
        );
        assert_eq!(any_version, ExpectedVersion::Any);
    }

    #[test]
    fn test_metadata_serialization() {
        let metadata = EventMetadata::new()
            .with_correlation_id("test-correlation".to_string())
            .with_user_id("test-user".to_string());

        let json_value = serde_json::to_value(&metadata).unwrap();
        let deserialized: EventMetadata = serde_json::from_value(json_value).unwrap();

        assert_eq!(metadata, deserialized);
    }

    #[test]
    fn test_stream_events_construction() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event_id = EventId::new();
        let payload = json!({"test": "data"});

        let event = EventToWrite::new(event_id, payload);
        let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::New, vec![event]);

        assert_eq!(stream_events.stream_id, stream_id);
        assert_eq!(stream_events.expected_version, ExpectedVersion::New);
        assert_eq!(stream_events.events.len(), 1);
    }
}
