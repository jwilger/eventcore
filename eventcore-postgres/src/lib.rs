use std::time::Duration;

use sqlx::query;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use thiserror::Error;

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
