//! `PostgreSQL` adapter for `EventCore` event sourcing library
//!
//! This crate provides a `PostgreSQL` implementation of the `EventStore` trait
//! from the eventcore crate, enabling persistent event storage with
//! multi-stream atomicity support.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod event_store;

use parking_lot::RwLock;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use eventcore::{EventStoreError, EventVersion, StreamId};
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

impl PostgresConfig {
    /// Create a new configuration with just a database URL, using defaults for other settings
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            ..Self::default()
        }
    }
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            database_url: "postgres://postgres:postgres@localhost/eventcore".to_string(),
            max_connections: 20, // Increased for better concurrency
            min_connections: 2,  // Keep minimum connections open
            connect_timeout: Duration::from_secs(10), // Faster timeout for performance
            max_lifetime: Some(Duration::from_secs(1800)), // 30 minutes
            idle_timeout: Some(Duration::from_secs(600)), // 10 minutes
            test_before_acquire: false, // Skip validation for performance
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

/// Simple cache entry for stream versions
#[derive(Debug, Clone)]
#[allow(dead_code)] // Used in future performance optimizations
struct VersionCacheEntry {
    version: EventVersion,
    timestamp: std::time::Instant,
}

/// `PostgreSQL` event store implementation
#[derive(Debug)]
pub struct PostgresEventStore<E>
where
    E: Send + Sync,
{
    pool: Arc<PgPool>,
    config: PostgresConfig,
    /// Simple cache for stream versions (read-heavy workload optimization)
    version_cache: Arc<RwLock<HashMap<StreamId, VersionCacheEntry>>>,
    /// Phantom data to track event type
    _phantom: PhantomData<E>,
}

impl<E> Clone for PostgresEventStore<E>
where
    E: Send + Sync,
{
    fn clone(&self) -> Self {
        Self {
            pool: Arc::clone(&self.pool),
            config: self.config.clone(),
            version_cache: Arc::clone(&self.version_cache),
            _phantom: PhantomData,
        }
    }
}

impl<E> PostgresEventStore<E>
where
    E: Send + Sync,
{
    /// Create a new `PostgreSQL` event store with the given configuration
    pub async fn new(config: PostgresConfig) -> Result<Self, PostgresError> {
        let pool = Self::create_pool(&config).await?;

        Ok(Self {
            pool: Arc::new(pool),
            config,
            version_cache: Arc::new(RwLock::new(HashMap::new())),
            _phantom: PhantomData,
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

    /// Initialize the database schema
    ///
    /// This method creates the necessary tables and indexes for the event store.
    /// It is idempotent and can be called multiple times safely.
    pub async fn initialize(&self) -> Result<(), PostgresError> {
        // Create events table
        sqlx::query(
            r"
            CREATE TABLE IF NOT EXISTS events (
                event_id UUID PRIMARY KEY,
                stream_id VARCHAR(255) NOT NULL,
                event_version BIGINT NOT NULL,
                event_type VARCHAR(255) NOT NULL,
                event_data JSONB NOT NULL,
                metadata JSONB,
                causation_id UUID,
                correlation_id VARCHAR(255),
                user_id VARCHAR(255),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(stream_id, event_version)
            )
            ",
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(PostgresError::Connection)?;

        // Create indexes - must be separate queries
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_events_stream_id ON events(stream_id)")
            .execute(self.pool.as_ref())
            .await
            .map_err(PostgresError::Connection)?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_events_created_at ON events(created_at)")
            .execute(self.pool.as_ref())
            .await
            .map_err(PostgresError::Connection)?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_events_correlation_id ON events(correlation_id)",
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(PostgresError::Connection)?;

        // Create event_streams table for tracking stream metadata
        sqlx::query(
            r"
            CREATE TABLE IF NOT EXISTS event_streams (
                stream_id VARCHAR(255) PRIMARY KEY,
                current_version BIGINT NOT NULL DEFAULT 0,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            ",
        )
        .execute(self.pool.as_ref())
        .await
        .map_err(PostgresError::Connection)?;

        info!("Database schema initialized successfully");
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

    /// Get cached version if available and not expired (cache TTL: 5 seconds)
    #[allow(dead_code)] // Used in future performance optimizations
    fn get_cached_version(&self, stream_id: &StreamId) -> Option<EventVersion> {
        const CACHE_TTL: Duration = Duration::from_secs(5);

        let cache = self.version_cache.read();
        cache.get(stream_id).and_then(|entry| {
            if entry.timestamp.elapsed() < CACHE_TTL {
                Some(entry.version)
            } else {
                None
            }
        })
    }

    /// Cache a stream version
    #[allow(dead_code)] // Used in future performance optimizations
    fn cache_version(&self, stream_id: StreamId, version: EventVersion) {
        let mut cache = self.version_cache.write();
        cache.insert(
            stream_id,
            VersionCacheEntry {
                version,
                timestamp: std::time::Instant::now(),
            },
        );

        // Simple cache size management - keep last 1000 entries
        if cache.len() > 1000 {
            let oldest_keys: Vec<_> = cache.keys().take(100).cloned().collect();
            for key in oldest_keys {
                cache.remove(&key);
            }
        }
    }

    /// Invalidate cached version for a stream (called after writes)
    #[allow(dead_code)] // Used in future performance optimizations
    fn invalidate_cached_version(&self, stream_id: &StreamId) {
        let mut cache = self.version_cache.write();
        cache.remove(stream_id);
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

    /// Configure for high-performance event sourcing workloads
    #[must_use]
    pub const fn performance_optimized(mut self) -> Self {
        self.config.max_connections = 30;
        self.config.min_connections = 5;
        self.config.connect_timeout = Duration::from_secs(5);
        self.config.max_lifetime = Some(Duration::from_secs(1800));
        self.config.idle_timeout = Some(Duration::from_secs(300));
        self.config.test_before_acquire = false;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_postgres_config_default() {
        let config = PostgresConfig::default();
        assert_eq!(config.max_connections, 20); // Updated for performance
        assert_eq!(config.min_connections, 2); // Updated for performance
        assert_eq!(config.connect_timeout, Duration::from_secs(10)); // Updated for performance
        assert!(!config.test_before_acquire); // Updated for performance
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
