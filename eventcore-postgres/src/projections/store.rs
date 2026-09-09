use std::num::NonZeroU64;

use eventcore_types::{DeliveryPosition, DeliverySourceId, ProjectionSelectionId, ProjectorName};
use sqlx::{
    Acquire, Pool, Postgres, Row, Transaction, pool::PoolConnection, postgres::PgPoolOptions,
    query, query_scalar,
};

use super::{TransactionalProjectionError, migration::run_component_migration};
use crate::PostgresConfig;

const PROJECTION_DESTINATION_COMPONENT: &str = "projection-destination";
const PROJECTION_DESTINATION_VERSION: i64 = 2;
const PROJECTION_DESTINATION_MIGRATION: &str =
    include_str!("../../projection-migrations/0002_projection_destination.sql");
const PROJECTION_LEADER_NAMESPACE: &str = "eventcore:transactional-projection:";

/// Durable progress committed beside a projection's read-model effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionProgress {
    source_id: DeliverySourceId,
    selection_id: ProjectionSelectionId,
    position: DeliveryPosition,
}

impl ProjectionProgress {
    /// Returns the delivery source identity bound to this progress row.
    pub fn source_id(&self) -> &DeliverySourceId {
        &self.source_id
    }

    /// Returns the selection identity bound to this progress row.
    pub fn selection_id(&self) -> &ProjectionSelectionId {
        &self.selection_id
    }

    /// Returns the last transactionally committed delivery position.
    pub fn position(&self) -> DeliveryPosition {
        self.position
    }
}

/// PostgreSQL destination containing a transactional projection's read model and progress.
#[derive(Debug, Clone)]
pub struct PostgresProjectionStore {
    pool: Pool<Postgres>,
}

impl PostgresProjectionStore {
    /// Connects a destination pool with the standard EventCore PostgreSQL configuration.
    pub async fn new<S: Into<String>>(
        connection_string: S,
    ) -> Result<Self, TransactionalProjectionError> {
        let config = PostgresConfig::default();
        let max_connections: std::num::NonZeroU32 = config.max_connections.into();
        let pool = PgPoolOptions::new()
            .max_connections(max_connections.get())
            .acquire_timeout(config.acquire_timeout)
            .idle_timeout(config.idle_timeout)
            .connect(&connection_string.into())
            .await
            .map_err(progress_error)?;
        Ok(Self::from_pool(pool))
    }

    /// Creates a projection destination from an application-owned pool.
    pub fn from_pool(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }

    /// Applies the destination-owned progress migration through the separate projection ledger.
    pub async fn migrate(&self) -> Result<(), TransactionalProjectionError> {
        run_component_migration(
            &self.pool,
            PROJECTION_DESTINATION_COMPONENT,
            PROJECTION_DESTINATION_VERSION,
            PROJECTION_DESTINATION_MIGRATION,
        )
        .await
        .map_err(progress_error)
    }

    /// Reads public durable progress for one projector, if it has committed an event.
    pub async fn progress(
        &self,
        projector: &ProjectorName,
    ) -> Result<Option<ProjectionProgress>, TransactionalProjectionError> {
        let row = query(
            "SELECT source_id, selection_id, last_position \
             FROM eventcore_projection_progress WHERE projector_name = $1",
        )
        .bind(projector.as_ref())
        .fetch_optional(&self.pool)
        .await
        .map_err(progress_error)?;

        row.map(progress_from_row).transpose()
    }

    pub(crate) async fn acquire_leader(
        &self,
        projector: &ProjectorName,
    ) -> Result<ProjectionLeader, TransactionalProjectionError> {
        let mut connection = self.pool.acquire().await.map_err(leadership_error)?;
        connection.close_on_drop();
        let lock_key = projection_lock_key(projector);
        let acquired: bool = query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(lock_key)
            .fetch_one(&mut *connection)
            .await
            .map_err(leadership_error)?;
        if !acquired {
            let _ = connection.close().await;
            return Err(TransactionalProjectionError::LeadershipBusy);
        }

        Ok(ProjectionLeader {
            connection: Some(connection),
            lock_key,
        })
    }
}

pub(crate) struct ProjectionLeader {
    connection: Option<PoolConnection<Postgres>>,
    lock_key: i64,
}

impl ProjectionLeader {
    pub(crate) async fn begin(
        &mut self,
    ) -> Result<Transaction<'_, Postgres>, TransactionalProjectionError> {
        self.connection_mut()?
            .begin()
            .await
            .map_err(leadership_error)
    }

    pub(crate) async fn load_progress(
        transaction: &mut Transaction<'_, Postgres>,
        projector: &ProjectorName,
    ) -> Result<Option<ProjectionProgress>, TransactionalProjectionError> {
        let row = query(
            "SELECT source_id, selection_id, last_position \
             FROM eventcore_projection_progress WHERE projector_name = $1 FOR UPDATE",
        )
        .bind(projector.as_ref())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(progress_error)?;
        row.map(progress_from_row).transpose()
    }

    pub(crate) async fn advance_progress(
        transaction: &mut Transaction<'_, Postgres>,
        projector: &ProjectorName,
        source_id: &DeliverySourceId,
        selection_id: &ProjectionSelectionId,
        position: DeliveryPosition,
    ) -> Result<(), TransactionalProjectionError> {
        let position = i64::try_from(position.get()).map_err(|error| {
            TransactionalProjectionError::ProgressStore {
                source: Box::new(error),
            }
        })?;
        let _ = query(
            "INSERT INTO eventcore_projection_progress \
             (projector_name, source_id, selection_id, last_position) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (projector_name) DO UPDATE SET \
             source_id = EXCLUDED.source_id, selection_id = EXCLUDED.selection_id, \
             last_position = EXCLUDED.last_position, updated_at = NOW()",
        )
        .bind(projector.as_ref())
        .bind(source_id.as_ref())
        .bind(selection_id.as_ref())
        .bind(position)
        .execute(&mut **transaction)
        .await
        .map_err(progress_error)?;
        Ok(())
    }

    pub(crate) async fn release(mut self) -> Result<(), TransactionalProjectionError> {
        let Some(mut connection) = self.connection.take() else {
            return Ok(());
        };
        let _: bool = query_scalar("SELECT pg_advisory_unlock($1)")
            .bind(self.lock_key)
            .fetch_one(&mut *connection)
            .await
            .map_err(leadership_error)?;
        connection.close().await.map_err(leadership_error)
    }

    fn connection_mut(
        &mut self,
    ) -> Result<&mut PoolConnection<Postgres>, TransactionalProjectionError> {
        self.connection
            .as_mut()
            .ok_or_else(|| TransactionalProjectionError::LeadershipLost {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "projection leader connection is unavailable",
                )),
            })
    }
}

fn progress_from_row(
    row: sqlx::postgres::PgRow,
) -> Result<ProjectionProgress, TransactionalProjectionError> {
    let source_id: String = row.try_get("source_id").map_err(progress_error)?;
    let selection_id: String = row.try_get("selection_id").map_err(progress_error)?;
    let position: i64 = row.try_get("last_position").map_err(progress_error)?;
    let source_id = DeliverySourceId::try_new(source_id).map_err(|error| {
        TransactionalProjectionError::ProgressStore {
            source: Box::new(error),
        }
    })?;
    let selection_id = ProjectionSelectionId::try_new(selection_id).map_err(|error| {
        TransactionalProjectionError::ProgressStore {
            source: Box::new(error),
        }
    })?;
    let position = u64::try_from(position)
        .ok()
        .and_then(NonZeroU64::new)
        .map(DeliveryPosition::new)
        .ok_or_else(|| TransactionalProjectionError::ProgressStore {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "projection progress position must be positive",
            )),
        })?;

    Ok(ProjectionProgress {
        source_id,
        selection_id,
        position,
    })
}

fn progress_error(error: sqlx::Error) -> TransactionalProjectionError {
    TransactionalProjectionError::ProgressStore {
        source: Box::new(error),
    }
}

fn leadership_error(error: sqlx::Error) -> TransactionalProjectionError {
    TransactionalProjectionError::LeadershipLost {
        source: Box::new(error),
    }
}

fn projection_lock_key(projector: &ProjectorName) -> i64 {
    stable_fnv1a(&format!("{PROJECTION_LEADER_NAMESPACE}{projector}"))
}

fn stable_fnv1a(value: &str) -> i64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash as i64
}
