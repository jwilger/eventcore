use std::num::NonZeroU64;

use eventcore_types::{
    BatchSize, DeliveryPosition, DeliverySourceId, DeliveryUpperBound, EventTypeName,
    PersistedEventEnvelope, PersistedEventId, ProjectionSelection, ProjectionSource,
    ProjectionStreamFilter, StreamId, StreamVersion,
};
use serde_json::value::RawValue;
use sqlx::{Pool, Postgres, QueryBuilder, Row, postgres::PgPoolOptions, query_scalar};
use thiserror::Error;
use uuid::Uuid;

use super::migration::run_component_migration;
use crate::PostgresConfig;

const DELIVERY_SOURCE_COMPONENT: &str = "delivery-source";
const DELIVERY_SOURCE_VERSION: i64 = 1;
const DELIVERY_SOURCE_MIGRATION: &str =
    include_str!("../../projection-migrations/0001_delivery_source.sql");

/// Errors returned by [`PostgresProjectionSource`].
#[derive(Debug, Error)]
pub enum PostgresProjectionSourceError {
    /// Connecting the source's PostgreSQL pool failed.
    #[error("failed to connect projection source")]
    ConnectionFailed(#[source] sqlx::Error),
    /// Applying the projection source's component-scoped migration failed.
    #[error("failed to migrate projection source")]
    MigrationFailed(#[source] sqlx::Error),
    /// Reading the committed delivery sequence failed.
    #[error("failed to read projection delivery source")]
    ReadFailed(#[source] sqlx::Error),
    /// A persisted delivery value cannot satisfy the public delivery contract.
    #[error("invalid persisted {field}: {detail}")]
    InvalidPersistedValue {
        /// Name of the persisted field that could not be represented.
        field: &'static str,
        /// Conversion failure detail.
        detail: String,
    },
}

/// PostgreSQL implementation of the strict, lossless projection delivery source.
#[derive(Debug, Clone)]
pub struct PostgresProjectionSource {
    pool: Pool<Postgres>,
    source_id: DeliverySourceId,
}

impl PostgresProjectionSource {
    /// Creates a source with the default PostgreSQL pool configuration.
    pub async fn new<S: Into<String>>(
        connection_string: S,
        source_id: DeliverySourceId,
    ) -> Result<Self, PostgresProjectionSourceError> {
        Self::with_config(connection_string, PostgresConfig::default(), source_id).await
    }

    /// Creates a source with an explicit PostgreSQL pool configuration.
    pub async fn with_config<S: Into<String>>(
        connection_string: S,
        config: PostgresConfig,
        source_id: DeliverySourceId,
    ) -> Result<Self, PostgresProjectionSourceError> {
        let max_connections: std::num::NonZeroU32 = config.max_connections.into();
        let pool = PgPoolOptions::new()
            .max_connections(max_connections.get())
            .acquire_timeout(config.acquire_timeout)
            .idle_timeout(config.idle_timeout)
            .connect(&connection_string.into())
            .await
            .map_err(PostgresProjectionSourceError::ConnectionFailed)?;
        Ok(Self::from_pool(pool, source_id))
    }

    /// Creates a source from an existing pool and a stable application-supplied source identity.
    pub fn from_pool(pool: Pool<Postgres>, source_id: DeliverySourceId) -> Self {
        Self { pool, source_id }
    }

    /// Applies the source-owned delivery migration without using SQLx's shared migration ledger.
    pub async fn migrate(&self) -> Result<(), PostgresProjectionSourceError> {
        run_component_migration(
            &self.pool,
            DELIVERY_SOURCE_COMPONENT,
            DELIVERY_SOURCE_VERSION,
            DELIVERY_SOURCE_MIGRATION,
        )
        .await
    }
}

impl ProjectionSource for PostgresProjectionSource {
    type Error = PostgresProjectionSourceError;

    fn source_id(&self) -> &DeliverySourceId {
        &self.source_id
    }

    async fn high_watermark(&self) -> Result<Option<DeliveryPosition>, Self::Error> {
        let watermark: Option<i64> = query_scalar(
            "SELECT NULLIF(last_position, 0) FROM eventcore_projection_delivery_frontier WHERE singleton = TRUE",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(PostgresProjectionSourceError::ReadFailed)?;

        watermark.map(delivery_position).transpose()
    }

    async fn read_envelopes(
        &self,
        selection: &ProjectionSelection,
        after: Option<DeliveryPosition>,
        through: DeliveryUpperBound,
        limit: BatchSize,
    ) -> Result<Vec<PersistedEventEnvelope>, Self::Error> {
        let mut query = QueryBuilder::<Postgres>::new(
            "SELECT delivery.delivery_position, events.event_id, events.stream_id, \
             events.stream_version, events.event_type, events.event_data::TEXT AS event_data, \
             events.metadata::TEXT AS metadata \
             FROM eventcore_projection_delivery AS delivery \
             INNER JOIN eventcore_events AS events ON events.event_id = delivery.event_id \
             WHERE TRUE",
        );

        if let Some(after) = after {
            let _ = query
                .push(" AND delivery.delivery_position > ")
                .push_bind(i64_from_position(after)?);
        }
        if let DeliveryUpperBound::Inclusive(through) = through {
            let _ = query
                .push(" AND delivery.delivery_position <= ")
                .push_bind(i64_from_position(through)?);
        }

        match selection.stream_filter() {
            ProjectionStreamFilter::All => {}
            ProjectionStreamFilter::Prefix(prefix) => {
                let _ = query
                    .push(" AND LEFT(events.stream_id, char_length(")
                    .push_bind(prefix.as_ref().to_string())
                    .push(")) = ")
                    .push_bind(prefix.as_ref().to_string());
            }
            ProjectionStreamFilter::Pattern(pattern) => {
                let _ = query
                    .push(" AND events.stream_id ~ ")
                    .push_bind(glob_to_anchored_regex(pattern.as_ref()));
            }
        }

        let _ = query.push(" AND events.event_type IN (");
        let mut separated = query.separated(", ");
        for event_type in selection.event_types() {
            let _ = separated.push_bind(event_type.as_ref());
        }
        let _ = separated.push_unseparated(")");
        let _ = query
            .push(" ORDER BY delivery.delivery_position ASC LIMIT ")
            .push_bind(i64::try_from(usize::from(limit)).map_err(|error| {
                PostgresProjectionSourceError::InvalidPersistedValue {
                    field: "batch limit",
                    detail: error.to_string(),
                }
            })?);

        let rows = query
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(PostgresProjectionSourceError::ReadFailed)?;

        rows.into_iter()
            .map(|row| envelope_from_row(row, self.source_id.clone()))
            .collect()
    }
}

fn envelope_from_row(
    row: sqlx::postgres::PgRow,
    source_id: DeliverySourceId,
) -> Result<PersistedEventEnvelope, PostgresProjectionSourceError> {
    let position: i64 = row
        .try_get("delivery_position")
        .map_err(PostgresProjectionSourceError::ReadFailed)?;
    let event_id: Uuid = row
        .try_get("event_id")
        .map_err(PostgresProjectionSourceError::ReadFailed)?;
    let stream_id: String = row
        .try_get("stream_id")
        .map_err(PostgresProjectionSourceError::ReadFailed)?;
    let stream_version: i64 = row
        .try_get("stream_version")
        .map_err(PostgresProjectionSourceError::ReadFailed)?;
    let event_type: String = row
        .try_get("event_type")
        .map_err(PostgresProjectionSourceError::ReadFailed)?;
    let payload: String = row
        .try_get("event_data")
        .map_err(PostgresProjectionSourceError::ReadFailed)?;
    let metadata: String = row
        .try_get("metadata")
        .map_err(PostgresProjectionSourceError::ReadFailed)?;

    let stream_id = StreamId::try_new(stream_id).map_err(|error| {
        PostgresProjectionSourceError::InvalidPersistedValue {
            field: "stream_id",
            detail: error.to_string(),
        }
    })?;
    let stream_version = usize::try_from(stream_version).map_err(|error| {
        PostgresProjectionSourceError::InvalidPersistedValue {
            field: "stream_version",
            detail: error.to_string(),
        }
    })?;
    let event_type = EventTypeName::try_new(event_type).map_err(|error| {
        PostgresProjectionSourceError::InvalidPersistedValue {
            field: "event_type",
            detail: error.to_string(),
        }
    })?;
    let payload = RawValue::from_string(payload).map_err(|error| {
        PostgresProjectionSourceError::InvalidPersistedValue {
            field: "event_data",
            detail: error.to_string(),
        }
    })?;
    let metadata = RawValue::from_string(metadata).map_err(|error| {
        PostgresProjectionSourceError::InvalidPersistedValue {
            field: "metadata",
            detail: error.to_string(),
        }
    })?;
    Ok(PersistedEventEnvelope::new(
        source_id,
        delivery_position(position)?,
        PersistedEventId::new(event_id),
        stream_id,
        StreamVersion::new(stream_version),
        event_type,
        payload,
        metadata,
    ))
}

fn delivery_position(value: i64) -> Result<DeliveryPosition, PostgresProjectionSourceError> {
    let value = u64::try_from(value).map_err(|error| {
        PostgresProjectionSourceError::InvalidPersistedValue {
            field: "delivery_position",
            detail: error.to_string(),
        }
    })?;
    let value = NonZeroU64::new(value).ok_or_else(|| {
        PostgresProjectionSourceError::InvalidPersistedValue {
            field: "delivery_position",
            detail: "must be positive".to_string(),
        }
    })?;
    Ok(DeliveryPosition::new(value))
}

fn i64_from_position(value: DeliveryPosition) -> Result<i64, PostgresProjectionSourceError> {
    i64::try_from(value.get()).map_err(|error| {
        PostgresProjectionSourceError::InvalidPersistedValue {
            field: "delivery_position",
            detail: error.to_string(),
        }
    })
}

fn glob_to_anchored_regex(glob: &str) -> String {
    let mut regex = String::with_capacity(glob.len() + 2);
    regex.push('^');

    let mut chars = glob.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '[' => {
                let mut class = String::new();
                let mut closed = false;
                let negated = matches!(chars.peek(), Some('!'));
                if negated {
                    let _ = chars.next();
                    class.push('^');
                }
                for class_character in chars.by_ref() {
                    if class_character == ']' {
                        closed = true;
                        break;
                    }
                    class.push(class_character);
                }
                if closed {
                    regex.push('[');
                    for (index, class_character) in class.chars().enumerate() {
                        if !negated && index == 0 && class_character == '^' {
                            regex.push_str("\\^");
                        } else if class_character == '\\' {
                            regex.push_str("\\\\");
                        } else {
                            regex.push(class_character);
                        }
                    }
                    regex.push(']');
                } else {
                    regex.push_str("\\[");
                    regex.push_str(&class);
                }
            }
            '\\' | '^' | '$' | '.' | '+' | '(' | ')' | '|' | '{' | '}' => {
                regex.push('\\');
                regex.push(character);
            }
            other => regex.push(other),
        }
    }

    regex.push('$');
    regex
}
