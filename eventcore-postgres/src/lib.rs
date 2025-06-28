//! `PostgreSQL` adapter for `EventCore` event sourcing library
//!
//! This crate provides a `PostgreSQL` implementation of the `EventStore` trait
//! from the eventcore crate, enabling persistent event storage with
//! multi-stream atomicity support.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod event_store;

use std::sync::Arc;
use std::time::Duration;

use eventcore::errors::EventStoreError;
use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use thiserror::Error;
use tracing::{debug, info, instrument};

/// Configuration for `PostgreSQL` connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostgresConfig {
    /// Database connection URL
    pub database_url: String,

    /// Maximum number of connections in the pool
    pub max_connections: u32,

    /// Minimum idle connections to maintain
    pub min_connections: u32,

    /// Connection timeout
    pub connect_timeout: Duration,

    /// Maximum lifetime of a connection
    pub max_lifetime: Option<Duration>,

    /// Idle timeout for connections
    pub idle_timeout: Option<Duration>,

    /// Whether to test connections before use
    pub test_before_acquire: bool,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            database_url: "postgres://postgres:postgres@localhost/eventcore".to_string(),
            max_connections: 10,
            min_connections: 1,
            connect_timeout: Duration::from_secs(30),
            max_lifetime: Some(Duration::from_secs(3600)),
            idle_timeout: Some(Duration::from_secs(600)),
            test_before_acquire: true,
        }
    }
}

/// PostgreSQL-specific errors
#[derive(Debug, Error)]
pub enum PostgresError {
    /// Database connection error
    #[error("Database connection error: {0}")]
    Connection(#[from] sqlx::Error),

    /// Pool creation error
    #[error("Failed to create connection pool: {0}")]
    PoolCreation(String),

    /// Migration error
    #[error("Database migration error: {0}")]
    Migration(String),

    /// Serialization error
    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Transaction error
    #[error("Transaction error: {0}")]
    Transaction(String),
}

impl From<PostgresError> for EventStoreError {
    fn from(error: PostgresError) -> Self {
        match error {
            PostgresError::Connection(sqlx_error) => Self::ConnectionFailed(sqlx_error.to_string()),
            PostgresError::PoolCreation(msg) => Self::ConnectionFailed(msg),
            PostgresError::Migration(msg) => Self::Configuration(msg),
            PostgresError::Serialization(err) => Self::SerializationFailed(err.to_string()),
            PostgresError::Transaction(msg) => Self::TransactionRollback(msg),
        }
    }
}

/// `PostgreSQL` event store implementation
#[derive(Debug, Clone)]
pub struct PostgresEventStore {
    pool: Arc<PgPool>,
    config: PostgresConfig,
}

impl PostgresEventStore {
    /// Create a new `PostgreSQL` event store with the given configuration
    pub async fn new(config: PostgresConfig) -> Result<Self, PostgresError> {
        let pool = Self::create_pool(&config).await?;

        Ok(Self {
            pool: Arc::new(pool),
            config,
        })
    }

    /// Create a connection pool from configuration
    async fn create_pool(config: &PostgresConfig) -> Result<PgPool, PostgresError> {
        let connect_options: PgConnectOptions = config
            .database_url
            .parse()
            .map_err(|e| PostgresError::PoolCreation(format!("Invalid database URL: {e}")))?;

        let mut pool_options = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(config.connect_timeout)
            .test_before_acquire(config.test_before_acquire);

        if let Some(max_lifetime) = config.max_lifetime {
            pool_options = pool_options.max_lifetime(max_lifetime);
        }

        if let Some(idle_timeout) = config.idle_timeout {
            pool_options = pool_options.idle_timeout(idle_timeout);
        }

        pool_options
            .connect_with(connect_options)
            .await
            .map_err(|e| PostgresError::PoolCreation(format!("Failed to create pool: {e}")))
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Get the configuration
    pub const fn config(&self) -> &PostgresConfig {
        &self.config
    }

    /// Run database migrations
    #[allow(clippy::unused_async)]
    pub async fn migrate(&self) -> Result<(), PostgresError> {
        // TODO: Implement migrations in Phase 8.3
        info!("Database migrations will be implemented in Phase 8.3");
        Ok(())
    }

    /// Check database connectivity
    #[instrument(skip(self))]
    pub async fn health_check(&self) -> Result<(), PostgresError> {
        sqlx::query("SELECT 1")
            .execute(self.pool.as_ref())
            .await
            .map_err(PostgresError::Connection)?;

        debug!("PostgreSQL health check passed");
        Ok(())
    }
}

// EventStore implementation is now in the event_store module

/// Builder for `PostgreSQL` event store configuration
#[derive(Debug, Default)]
pub struct PostgresConfigBuilder {
    config: PostgresConfig,
}

impl PostgresConfigBuilder {
    /// Create a new configuration builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the database URL
    #[must_use]
    pub fn database_url(mut self, url: impl Into<String>) -> Self {
        self.config.database_url = url.into();
        self
    }

    /// Set the maximum number of connections
    #[must_use]
    pub const fn max_connections(mut self, max: u32) -> Self {
        self.config.max_connections = max;
        self
    }

    /// Set the minimum number of idle connections
    #[must_use]
    pub const fn min_connections(mut self, min: u32) -> Self {
        self.config.min_connections = min;
        self
    }

    /// Set the connection timeout
    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.config.connect_timeout = timeout;
        self
    }

    /// Set the maximum connection lifetime
    #[must_use]
    pub const fn max_lifetime(mut self, lifetime: Option<Duration>) -> Self {
        self.config.max_lifetime = lifetime;
        self
    }

    /// Set the idle timeout
    #[must_use]
    pub const fn idle_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.config.idle_timeout = timeout;
        self
    }

    /// Set whether to test connections before acquisition
    #[must_use]
    pub const fn test_before_acquire(mut self, test: bool) -> Self {
        self.config.test_before_acquire = test;
        self
    }

    /// Build the configuration
    pub fn build(self) -> PostgresConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_postgres_config_default() {
        let config = PostgresConfig::default();
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 1);
        assert_eq!(config.connect_timeout, Duration::from_secs(30));
        assert!(config.test_before_acquire);
    }

    #[test]
    fn test_postgres_config_builder() {
        let config = PostgresConfigBuilder::new()
            .database_url("postgres://user:pass@localhost/test")
            .max_connections(20)
            .min_connections(2)
            .connect_timeout(Duration::from_secs(10))
            .test_before_acquire(false)
            .build();

        assert_eq!(config.database_url, "postgres://user:pass@localhost/test");
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.min_connections, 2);
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
        assert!(!config.test_before_acquire);
    }

    #[test]
    fn test_postgres_error_conversion() {
        let postgres_error = PostgresError::Transaction("test error".to_string());
        let event_store_error: EventStoreError = postgres_error.into();

        matches!(event_store_error, EventStoreError::TransactionRollback(_));
    }
}
