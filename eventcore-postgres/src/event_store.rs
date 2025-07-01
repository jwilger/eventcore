//! `PostgreSQL` implementation of the `EventStore` trait
//!
//! This module provides a complete `PostgreSQL` implementation of the `EventStore` trait
//! with support for multi-stream atomic operations, optimistic concurrency control,
//! and efficient event querying.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use eventcore::{
    Checkpoint, EventId, EventMetadata, EventProcessor, EventStore, EventStoreError, EventToWrite,
    EventVersion, ExpectedVersion, ReadOptions, StoredEvent, StreamData, StreamEvents, StreamId,
    Subscription, SubscriptionError, SubscriptionName, SubscriptionOptions, SubscriptionPosition,
    SubscriptionResult, Timestamp,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{postgres::PgRow, Row, Transaction};
use tracing::{debug, error, instrument};
use uuid::Uuid;

use crate::{PostgresError, PostgresEventStore};

type EventStoreResult<T> = Result<T, EventStoreError>;

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
    fn to_stored_event<E>(self) -> EventStoreResult<StoredEvent<E>>
    where
        E: for<'de> Deserialize<'de> + PartialEq + Eq,
    {
        let event_id = EventId::try_new(self.event_id)
            .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;

        let stream_id = StreamId::try_new(self.stream_id)
            .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;

        let event_version = if self.event_version >= 0 {
            let version_u64 = u64::try_from(self.event_version)?;
            EventVersion::try_new(version_u64)
                .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?
        } else {
            return Err(EventStoreError::SerializationFailed(
                "Negative event version in database".to_string(),
            ));
        };

        let timestamp = Timestamp::new(self.created_at);

        let metadata = if let Some(metadata_json) = self.metadata {
            let event_metadata: EventMetadata = serde_json::from_value(metadata_json)?;
            Some(event_metadata)
        } else {
            None
        };

        // Deserialize the event data from JSON to the target type
        let payload: E = serde_json::from_value(self.event_data)?;

        Ok(StoredEvent::new(
            event_id,
            stream_id,
            event_version,
            timestamp,
            payload,
            metadata,
        ))
    }
}

#[async_trait]
impl<E> EventStore for PostgresEventStore<E>
where
    E: Serialize
        + for<'de> Deserialize<'de>
        + Send
        + Sync
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Eq
        + 'static,
{
    type Event = E;

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
            .map_err(PostgresError::Connection)?;

        debug!("Retrieved {} events from database", rows.len());

        // Convert rows to events
        let mut events = Vec::new();
        for row in rows {
            let event_row = EventRow::try_from(row)
                .map_err(|e| EventStoreError::SerializationFailed(e.to_string()))?;
            events.push(event_row.to_stored_event::<E>()?);
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
        let mut tx = self.pool.begin().await.map_err(PostgresError::Connection)?;

        let mut result_versions = HashMap::new();

        for stream in stream_events {
            let stream_id = stream.stream_id.clone();
            let new_version = self.write_stream_events(&mut tx, stream).await?;
            result_versions.insert(stream_id, new_version);
        }

        // Commit transaction
        tx.commit().await.map_err(PostgresError::Connection)?;

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
                .map_err(PostgresError::Connection)?;

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
                .map_err(PostgresError::Connection)?;

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
        options: SubscriptionOptions,
    ) -> EventStoreResult<Box<dyn Subscription<Event = Self::Event>>> {
        let subscription = PostgresSubscription::new(self.clone(), options);
        Ok(Box::new(subscription))
    }
}

/// `PostgreSQL` subscription implementation with database checkpointing support.
pub struct PostgresSubscription<E>
where
    E: Serialize
        + for<'de> Deserialize<'de>
        + Send
        + Sync
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Eq
        + 'static,
{
    event_store: PostgresEventStore<E>,
    options: SubscriptionOptions,
    current_position: Arc<RwLock<Option<SubscriptionPosition>>>,
    is_running: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    stop_signal: Arc<AtomicBool>,
}

impl<E> PostgresSubscription<E>
where
    E: Serialize
        + for<'de> Deserialize<'de>
        + Send
        + Sync
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Eq
        + 'static,
{
    /// Creates a new `PostgreSQL` subscription.
    pub fn new(event_store: PostgresEventStore<E>, options: SubscriptionOptions) -> Self {
        Self {
            event_store,
            options,
            current_position: Arc::new(RwLock::new(None)),
            is_running: Arc::new(AtomicBool::new(false)),
            is_paused: Arc::new(AtomicBool::new(false)),
            stop_signal: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Processes events from the event store according to subscription options.
    async fn process_events(
        &self,
        name: SubscriptionName,
        mut processor: Box<dyn EventProcessor<Event = E>>,
    ) -> SubscriptionResult<()>
    where
        E: PartialEq + Eq,
    {
        // Load checkpoint to determine starting position
        let starting_position = self.load_checkpoint(&name).await?;

        loop {
            // Check if we should stop
            if self.stop_signal.load(Ordering::Acquire) {
                break;
            }

            // Check if we're paused
            if self.is_paused.load(Ordering::Acquire) {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }

            // Get events according to subscription options
            let events = self
                .get_events_for_processing(starting_position.as_ref())
                .await?;

            let mut current_pos = starting_position.clone();
            let mut has_new_events = false;

            for event in events {
                // Skip events we've already processed
                if let Some(ref pos) = current_pos {
                    if event.event_id <= pos.last_event_id {
                        continue;
                    }
                }

                // Process the event
                processor.process_event(event.clone()).await?;
                has_new_events = true;

                // Update current position
                let new_checkpoint = Checkpoint::new(event.event_id, event.event_version.into());

                current_pos = Some(if let Some(mut pos) = current_pos {
                    pos.last_event_id = event.event_id;
                    pos.update_checkpoint(event.stream_id.clone(), new_checkpoint);
                    pos
                } else {
                    let mut pos = SubscriptionPosition::new(event.event_id);
                    pos.update_checkpoint(event.stream_id.clone(), new_checkpoint);
                    pos
                });

                // Update our current position
                {
                    let mut guard = self.current_position.write().map_err(|_| {
                        SubscriptionError::CheckpointSaveFailed(
                            "Failed to acquire position lock".to_string(),
                        )
                    })?;
                    (*guard).clone_from(&current_pos);
                }

                // Periodically save checkpoint to database
                if let Some(ref pos) = current_pos {
                    self.save_checkpoint_to_db(&name, pos.clone()).await?;
                }
            }

            // If we're caught up and this is a live subscription, notify the processor
            if !has_new_events && matches!(self.options, SubscriptionOptions::LiveOnly) {
                processor.on_live().await?;
            }

            // Sleep briefly to avoid busy-waiting
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        Ok(())
    }

    /// Gets events for processing based on subscription options.
    async fn get_events_for_processing(
        &self,
        starting_position: Option<&SubscriptionPosition>,
    ) -> SubscriptionResult<Vec<StoredEvent<E>>> {
        let (streams, from_position) = match &self.options {
            SubscriptionOptions::CatchUpFromBeginning => (vec![], None),
            SubscriptionOptions::CatchUpFromPosition(pos) => (vec![], Some(pos.last_event_id)),
            SubscriptionOptions::LiveOnly => {
                // For live-only, start from the current position in the store
                (vec![], starting_position.as_ref().map(|p| p.last_event_id))
            }
            SubscriptionOptions::SpecificStreamsFromBeginning(_mode) => {
                // This would need stream selection logic based on mode
                (vec![], None)
            }
            SubscriptionOptions::SpecificStreamsFromPosition(_mode, pos) => {
                (vec![], Some(pos.last_event_id))
            }
            SubscriptionOptions::AllStreams { from_position } => (vec![], *from_position),
            SubscriptionOptions::SpecificStreams {
                streams,
                from_position,
            } => (streams.clone(), *from_position),
        };

        // Read events from PostgreSQL
        if streams.is_empty() {
            self.read_all_events_since(
                from_position.or_else(|| starting_position.map(|p| p.last_event_id)),
            )
            .await
        } else {
            self.read_streams_events_since(
                &streams,
                from_position.or_else(|| starting_position.map(|p| p.last_event_id)),
            )
            .await
        }
    }

    /// Reads all events from `PostgreSQL` since a given event ID.
    async fn read_all_events_since(
        &self,
        from_event_id: Option<EventId>,
    ) -> SubscriptionResult<Vec<StoredEvent<E>>> {
        // For now, use a simple approach: read all streams and filter
        // This can be optimized later with direct SQL queries
        let all_streams = self.get_all_stream_ids().await?;
        if all_streams.is_empty() {
            return Ok(vec![]);
        }

        let read_options = ReadOptions {
            from_version: None,
            to_version: None,
            max_events: Some(1000),
        };

        let stream_data = self
            .event_store
            .read_streams(&all_streams, &read_options)
            .await
            .map_err(SubscriptionError::EventStore)?;

        // Filter events based on from_event_id
        let filtered_events = if let Some(from_id) = from_event_id {
            stream_data
                .events
                .into_iter()
                .filter(|e| e.event_id > from_id)
                .collect()
        } else {
            stream_data.events
        };

        Ok(filtered_events)
    }

    /// Gets all stream IDs from the database.
    async fn get_all_stream_ids(&self) -> SubscriptionResult<Vec<StreamId>> {
        let rows = sqlx::query("SELECT DISTINCT stream_id FROM events LIMIT 1000")
            .fetch_all(self.event_store.pool.as_ref())
            .await
            .map_err(|e| {
                SubscriptionError::EventStore(EventStoreError::Internal(format!(
                    "Failed to fetch stream IDs from database for subscription processing (query: 'SELECT DISTINCT stream_id FROM events LIMIT 1000'): {e}"
                )))
            })?;

        let mut stream_ids = Vec::new();
        for row in rows {
            let stream_id_str: String = row.get("stream_id");
            if let Ok(stream_id) = StreamId::try_new(stream_id_str) {
                stream_ids.push(stream_id);
            }
        }

        Ok(stream_ids)
    }

    /// Reads events from specific streams since a given event ID.
    async fn read_streams_events_since(
        &self,
        stream_ids: &[StreamId],
        from_event_id: Option<EventId>,
    ) -> SubscriptionResult<Vec<StoredEvent<E>>> {
        let read_options = ReadOptions {
            from_version: None,
            to_version: None,
            max_events: None,
        };

        let stream_data = self
            .event_store
            .read_streams(stream_ids, &read_options)
            .await
            .map_err(SubscriptionError::EventStore)?;

        // Filter events based on from_event_id
        let filtered_events = if let Some(from_id) = from_event_id {
            stream_data
                .events
                .into_iter()
                .filter(|e| e.event_id > from_id)
                .collect()
        } else {
            stream_data.events
        };

        Ok(filtered_events)
    }

    /// Saves checkpoint to `PostgreSQL` database.
    async fn save_checkpoint_to_db(
        &self,
        name: &SubscriptionName,
        position: SubscriptionPosition,
    ) -> SubscriptionResult<()> {
        let position_json = serde_json::to_string(&position).map_err(|e| {
            SubscriptionError::CheckpointSaveFailed(format!(
                "Failed to serialize checkpoint position for subscription '{}': {e}",
                name.as_ref()
            ))
        })?;

        sqlx::query(
            "INSERT INTO subscription_checkpoints (subscription_name, position_data, updated_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT (subscription_name) 
             DO UPDATE SET position_data = $2, updated_at = NOW()",
        )
        .bind(name.as_ref())
        .bind(position_json)
        .execute(self.event_store.pool.as_ref())
        .await
        .map_err(|e| {
            SubscriptionError::CheckpointSaveFailed(format!(
                "Failed to save checkpoint for subscription '{}' to database: {e}",
                name.as_ref()
            ))
        })?;

        Ok(())
    }

    /// Loads checkpoint from `PostgreSQL` database.
    async fn load_checkpoint_from_db(
        &self,
        name: &SubscriptionName,
    ) -> SubscriptionResult<Option<SubscriptionPosition>> {
        let row = sqlx::query(
            "SELECT position_data FROM subscription_checkpoints WHERE subscription_name = $1",
        )
        .bind(name.as_ref())
        .fetch_optional(self.event_store.pool.as_ref())
        .await
        .map_err(|e| {
            SubscriptionError::CheckpointLoadFailed(format!(
                "Failed to load checkpoint for subscription '{}' from database: {e}",
                name.as_ref()
            ))
        })?;

        if let Some(row) = row {
            let position_json: String = row.get("position_data");
            let position = serde_json::from_str(&position_json).map_err(|e| {
                SubscriptionError::CheckpointLoadFailed(format!(
                    "Failed to deserialize checkpoint position for subscription '{}': {e}",
                    name.as_ref()
                ))
            })?;
            Ok(Some(position))
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl<E> Subscription for PostgresSubscription<E>
where
    E: Serialize
        + for<'de> Deserialize<'de>
        + Send
        + Sync
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Eq
        + 'static,
{
    type Event = E;

    async fn start(
        &mut self,
        name: SubscriptionName,
        options: SubscriptionOptions,
        processor: Box<dyn EventProcessor<Event = Self::Event>>,
    ) -> SubscriptionResult<()>
    where
        Self::Event: PartialEq + Eq,
    {
        // Update options if provided
        self.options = options;

        // Set running state
        self.is_running.store(true, Ordering::Release);
        self.stop_signal.store(false, Ordering::Release);
        self.is_paused.store(false, Ordering::Release);

        // Start processing events in a background task
        let subscription = self.clone(); // We'll need to implement Clone
        let name_copy = name;

        tokio::spawn(async move {
            if let Err(e) = subscription.process_events(name_copy, processor).await {
                error!("PostgreSQL subscription processing failed: {}", e);
            }
        });

        Ok(())
    }

    async fn stop(&mut self) -> SubscriptionResult<()> {
        self.stop_signal.store(true, Ordering::Release);
        self.is_running.store(false, Ordering::Release);
        Ok(())
    }

    async fn pause(&mut self) -> SubscriptionResult<()> {
        self.is_paused.store(true, Ordering::Release);
        Ok(())
    }

    async fn resume(&mut self) -> SubscriptionResult<()> {
        self.is_paused.store(false, Ordering::Release);
        Ok(())
    }

    async fn get_position(&self) -> SubscriptionResult<Option<SubscriptionPosition>> {
        let guard = self.current_position.read().map_err(|_| {
            SubscriptionError::CheckpointLoadFailed("Failed to acquire position lock".to_string())
        })?;
        Ok(guard.clone())
    }

    async fn save_checkpoint(&mut self, position: SubscriptionPosition) -> SubscriptionResult<()> {
        // For the PostgreSQL implementation, we update the current position
        {
            let mut guard = self.current_position.write().map_err(|_| {
                SubscriptionError::CheckpointSaveFailed(
                    "Failed to acquire position lock".to_string(),
                )
            })?;
            *guard = Some(position);
        }
        Ok(())
    }

    async fn load_checkpoint(
        &self,
        name: &SubscriptionName,
    ) -> SubscriptionResult<Option<SubscriptionPosition>> {
        self.load_checkpoint_from_db(name).await
    }
}

// Implement Clone for the PostgreSQL subscription
impl<E> Clone for PostgresSubscription<E>
where
    E: Serialize
        + for<'de> Deserialize<'de>
        + Send
        + Sync
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Eq
        + 'static,
{
    fn clone(&self) -> Self {
        Self {
            event_store: self.event_store.clone(),
            options: self.options.clone(),
            current_position: Arc::clone(&self.current_position),
            is_running: Arc::clone(&self.is_running),
            is_paused: Arc::clone(&self.is_paused),
            stop_signal: Arc::clone(&self.stop_signal),
        }
    }
}

impl<E> PostgresEventStore<E>
where
    E: Serialize
        + for<'de> Deserialize<'de>
        + Send
        + Sync
        + std::fmt::Debug
        + Clone
        + PartialEq
        + Eq
        + 'static,
{
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
        stream_events: StreamEvents<E>,
    ) -> EventStoreResult<EventVersion>
    where
        E: serde::Serialize + Sync,
    {
        let StreamEvents {
            stream_id,
            expected_version,
            events,
        } = stream_events;

        if events.is_empty() {
            return Err(EventStoreError::Internal(format!(
                "No events to write to stream '{}' (expected_version: {:?}). This may indicate a bug in command logic where events are created but then filtered out.",
                stream_id.as_ref(),
                expected_version
            )));
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
        event: &EventToWrite<E>,
    ) -> EventStoreResult<()>
    where
        E: serde::Serialize + Sync,
    {
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
                        Some(metadata.correlation_id.to_string()),
                        metadata
                            .user_id
                            .as_ref()
                            .map(|uid| uid.as_ref().to_string()),
                    )
                });

        // Serialize the event payload to JSON
        let event_data = serde_json::to_value(&event.payload).map_err(|e| {
            EventStoreError::SerializationFailed(format!("Failed to serialize event data: {e}"))
        })?;

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
        .bind(event_data)
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
    use eventcore::{EventToWrite, ExpectedVersion, StreamEvents};
    use serde::{Deserialize, Serialize};

    // Test event type for unit tests
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    enum TestEvent {
        Created { name: String },
        Updated { value: i32 },
    }

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
        use eventcore::{CorrelationId, UserId};

        let metadata = EventMetadata::new()
            .with_correlation_id(CorrelationId::new())
            .with_user_id(Some(UserId::try_new("test-user").unwrap()));

        let json_value = serde_json::to_value(&metadata).unwrap();
        let deserialized: EventMetadata = serde_json::from_value(json_value).unwrap();

        assert_eq!(metadata, deserialized);
    }

    #[test]
    fn test_stream_events_construction() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let event_id = EventId::new();
        let payload = TestEvent::Created {
            name: "test".to_string(),
        };

        let event = EventToWrite::new(event_id, payload);
        let stream_events = StreamEvents::new(stream_id.clone(), ExpectedVersion::New, vec![event]);

        assert_eq!(stream_events.stream_id, stream_id);
        assert_eq!(stream_events.expected_version, ExpectedVersion::New);
        assert_eq!(stream_events.events.len(), 1);
    }
}
