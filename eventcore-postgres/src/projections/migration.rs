use sqlx::{Pool, Postgres, query, query_scalar, raw_sql};

use super::PostgresProjectionSourceError;

const PROJECTION_MIGRATION_LOCK: i64 = 3_720_066_808_599_939_109;

pub(crate) async fn run_component_migration(
    pool: &Pool<Postgres>,
    component: &str,
    version: i64,
    migration_sql: &str,
) -> Result<(), PostgresProjectionSourceError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(PostgresProjectionSourceError::MigrationFailed)?;

    let _ = query("SELECT pg_advisory_xact_lock($1)")
        .bind(PROJECTION_MIGRATION_LOCK)
        .execute(&mut *transaction)
        .await
        .map_err(PostgresProjectionSourceError::MigrationFailed)?;
    let _ = query(
        "CREATE TABLE IF NOT EXISTS eventcore_projection_schema_versions (\
             component TEXT NOT NULL, \
             version BIGINT NOT NULL, \
             applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), \
             PRIMARY KEY (component, version)\
         )",
    )
    .execute(&mut *transaction)
    .await
    .map_err(PostgresProjectionSourceError::MigrationFailed)?;

    let already_applied: bool = query_scalar(
        "SELECT EXISTS (\
             SELECT 1 FROM eventcore_projection_schema_versions \
             WHERE component = $1 AND version = $2\
         )",
    )
    .bind(component)
    .bind(version)
    .fetch_one(&mut *transaction)
    .await
    .map_err(PostgresProjectionSourceError::MigrationFailed)?;

    if !already_applied {
        let _ = raw_sql(migration_sql)
            .execute(&mut *transaction)
            .await
            .map_err(PostgresProjectionSourceError::MigrationFailed)?;
        let _ = query(
            "INSERT INTO eventcore_projection_schema_versions (component, version) VALUES ($1, $2)",
        )
        .bind(component)
        .bind(version)
        .execute(&mut *transaction)
        .await
        .map_err(PostgresProjectionSourceError::MigrationFailed)?;
    }

    transaction
        .commit()
        .await
        .map_err(PostgresProjectionSourceError::MigrationFailed)
}
