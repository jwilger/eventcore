//! `PostgreSQL` adapter for `EventCore` event sourcing library
//!
//! This crate provides a `PostgreSQL` implementation of the `EventStore` trait
//! from the eventcore crate, enabling persistent event storage with
//! multi-stream atomicity support.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::unnecessary_cast)]

pub mod circuit_breaker;
mod event_store;
pub mod monitoring;
pub mod retry;

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use chrono::{DateTime, Utc};
use eventcore::{EventId, EventStoreError, ReadOptions, StoredEvent, StreamId};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use thiserror::Error;
use tracing::{debug, info, instrument};

pub use circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CircuitBreakerError, CircuitBreakerMetrics, CircuitState,
};
pub use monitoring::{AcquisitionTimer, PoolMetrics, PoolMonitor, PoolMonitoringTask};
pub use retry::{RetryError, RetryStrategy};

/// Comprehensive health status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// Whether the database is healthy overall
    pub is_healthy: bool,
    /// Latency of basic connectivity check
    pub basic_latency: Duration,
    /// Connection pool status
    pub pool_status: PoolStatus,
    /// Database schema status
    pub schema_status: SchemaStatus,
    /// Performance metrics
    pub performance_status: PerformanceStatus,
    /// Timestamp of last health check
    pub last_check: DateTime<Utc>,
}

/// Connection pool status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStatus {
    /// Current pool size
    pub size: u32,
    /// Number of idle connections
    pub idle: u32,
    /// Whether the pool is closed
    pub is_closed: bool,
}

/// Database schema status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaStatus {
    /// Whether events table exists
    pub has_events_table: bool,
    /// Whether `event_streams` table exists
    pub has_streams_table: bool,
    /// Whether `subscription_checkpoints` table exists
    pub has_subscriptions_table: bool,
    /// Whether schema is complete for basic operations
    pub is_complete: bool,
}

/// Performance status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceStatus {
    /// Latency of performance test query
    pub query_latency: Duration,
    /// Whether performance is within acceptable thresholds
    pub is_performant: bool,
}

/// Configuration for `PostgreSQL` connection with production-hardening features
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

    /// Query timeout for individual database operations
    /// This is different from connection timeout and applies to query execution
    pub query_timeout: Option<Duration>,

    /// Maximum lifetime of a connection
    pub max_lifetime: Option<Duration>,

    /// Idle timeout for connections
    pub idle_timeout: Option<Duration>,

    /// Whether to test connections before use
    pub test_before_acquire: bool,

    /// Maximum number of retry attempts for failed operations
    pub max_retries: u32,

    /// Base delay between retry attempts
    pub retry_base_delay: Duration,

    /// Maximum delay between retry attempts (for exponential backoff)
    pub retry_max_delay: Duration,

    /// Whether to enable connection recovery on failures
    pub enable_recovery: bool,

    /// Interval for periodic health checks
    pub health_check_interval: Duration,

    /// Batch size for reading events from multiple streams
    /// This controls how many events are fetched in a single query
    pub read_batch_size: usize,
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
            query_timeout: Some(Duration::from_secs(30)), // 30 second default query timeout
            max_lifetime: Some(Duration::from_secs(1800)), // 30 minutes
            idle_timeout: Some(Duration::from_secs(600)), // 10 minutes
            test_before_acquire: false, // Skip validation for performance
            max_retries: 3,      // Retry failed operations up to 3 times
            retry_base_delay: Duration::from_millis(100), // Start with 100ms delay
            retry_max_delay: Duration::from_secs(5), // Maximum 5 second delay
            enable_recovery: true, // Enable automatic connection recovery
            health_check_interval: Duration::from_secs(30), // Check health every 30 seconds
            read_batch_size: 1000, // Default batch size for multi-stream reads
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

#[allow(clippy::fallible_impl_from)] // We use expect() for critical internal errors that should never fail
impl From<PostgresError> for EventStoreError {
    fn from(error: PostgresError) -> Self {
        match error {
            PostgresError::Connection(sqlx_error) => {
                // Handle specific sqlx errors
                use sqlx::Error::{
                    Configuration, Database, Io, PoolClosed, PoolTimedOut, Protocol, RowNotFound,
                    Tls,
                };
                match &sqlx_error {
                    Configuration(_) => Self::Configuration(sqlx_error.to_string()),
                    Database(db_err) => {
                        // Check for specific PostgreSQL error codes
                        if let Some(code) = db_err.code() {
                            match code.as_ref() {
                                "23505" => {
                                    // PostgreSQL unique violation
                                    return Self::ConnectionFailed(format!(
                                        "Unique constraint violation: {db_err}"
                                    ));
                                }
                                "40001" => {
                                    // Serialization failure - transaction conflict in SERIALIZABLE isolation
                                    // Convert to VersionConflict which will be converted to ConcurrencyConflict by the executor
                                    // We can't determine the specific stream, so create a generic conflict
                                    use eventcore::{EventVersion, StreamId};
                                    return Self::VersionConflict {
                                        stream: StreamId::try_new("serialization-conflict")
                                            .unwrap(),
                                        expected: EventVersion::initial(),
                                        current: EventVersion::try_new(1).unwrap(),
                                    };
                                }
                                _ => {}
                            }
                        }

                        // Also check the error message for serialization failures that might not have the code
                        let error_msg = db_err.to_string().to_lowercase();
                        if error_msg.contains("could not serialize access due to concurrent update")
                            || error_msg.contains("serialization failure")
                        {
                            use eventcore::{EventVersion, StreamId};
                            return Self::VersionConflict {
                                stream: StreamId::try_new("serialization-conflict").unwrap(),
                                expected: EventVersion::initial(),
                                current: EventVersion::try_new(1).unwrap(),
                            };
                        }

                        Self::ConnectionFailed(db_err.to_string())
                    }
                    Io(_) | Tls(_) | Protocol(_) | PoolTimedOut | PoolClosed => {
                        Self::ConnectionFailed(sqlx_error.to_string())
                    }
                    RowNotFound => Self::ConnectionFailed(format!("Row not found: {sqlx_error}")),
                    _ => Self::Internal(sqlx_error.to_string()),
                }
            }
            PostgresError::PoolCreation(msg) => Self::ConnectionFailed(msg),
            PostgresError::Migration(msg) => Self::Configuration(msg),
            PostgresError::Serialization(err) => Self::SerializationFailed(err.to_string()),
            PostgresError::Transaction(msg) => Self::TransactionRollback(msg),
        }
    }
}

/// `PostgreSQL` event store implementation
#[derive(Debug)]
pub struct PostgresEventStore<E>
where
    E: Send + Sync,
{
    pool: Arc<PgPool>,
    config: PostgresConfig,
    retry_strategy: RetryStrategy,
    monitor: Arc<monitoring::PoolMonitor>,
    /// Phantom data to track event type
    _phantom: PhantomData<E>,
}

impl<E> Clone for PostgresEventStore<E>
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
            pool: Arc::clone(&self.pool),
            config: self.config.clone(),
            retry_strategy: self.retry_strategy.clone(),
            monitor: Arc::clone(&self.monitor),
            _phantom: PhantomData,
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
    /// Create a new `PostgreSQL` event store with the given configuration
    pub async fn new(config: PostgresConfig) -> Result<Self, PostgresError> {
        let pool = Self::create_pool(&config).await?;

        let retry_strategy = RetryStrategy {
            max_attempts: config.max_retries,
            base_delay: config.retry_base_delay,
            max_delay: config.retry_max_delay,
            ..RetryStrategy::default()
        };

        let monitor = Arc::new(monitoring::PoolMonitor::new(config.max_connections));

        Ok(Self {
            pool: Arc::new(pool),
            config,
            retry_strategy,
            monitor,
            _phantom: PhantomData,
        })
    }

    /// Create a new `PostgreSQL` event store with custom retry strategy
    pub async fn new_with_retry_strategy(
        config: PostgresConfig,
        retry_strategy: RetryStrategy,
    ) -> Result<Self, PostgresError> {
        let pool = Self::create_pool(&config).await?;
        let monitor = Arc::new(monitoring::PoolMonitor::new(config.max_connections));

        Ok(Self {
            pool: Arc::new(pool),
            config,
            retry_strategy,
            monitor,
            _phantom: PhantomData,
        })
    }

    /// Create a connection pool from configuration
    async fn create_pool(config: &PostgresConfig) -> Result<PgPool, PostgresError> {
        let mut connect_options: PgConnectOptions = config
            .database_url
            .parse()
            .map_err(|e| PostgresError::PoolCreation(format!("Invalid database URL: {e}")))?;

        // Apply query timeout if configured
        if let Some(query_timeout) = config.query_timeout {
            // Convert Duration to postgres statement timeout format (milliseconds)
            // Safe to cast as we're unlikely to have timeouts > u64::MAX milliseconds
            #[allow(clippy::cast_possible_truncation)]
            let timeout_ms = query_timeout.as_millis() as u64;
            connect_options =
                connect_options.options([("statement_timeout", &timeout_ms.to_string())]);
        }

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

    /// Check if schema tables exist
    async fn schema_exists(&self) -> Result<bool, PostgresError> {
        let count = sqlx::query_scalar::<_, i64>(
            r"
            SELECT COUNT(*) FROM information_schema.tables 
            WHERE table_name IN ('events', 'event_streams')
            AND table_schema = 'public'
            ",
        )
        .fetch_one(self.pool.as_ref())
        .await
        .map_err(PostgresError::Connection)?;

        Ok(count >= 2)
    }

    /// Try to acquire schema initialization lock
    async fn try_acquire_schema_lock(&self, lock_id: i64) -> Result<bool, PostgresError> {
        sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
            .bind(lock_id)
            .fetch_one(self.pool.as_ref())
            .await
            .map_err(PostgresError::Connection)
    }

    /// Wait for schema initialization by another process
    async fn wait_for_schema_initialization(&self, lock_id: i64) -> Result<bool, PostgresError> {
        // Try to acquire the lock again with a longer wait
        for _ in 0..50 {
            // Try for up to 5 seconds
            tokio::time::sleep(Duration::from_millis(100)).await;

            if self.try_acquire_schema_lock(lock_id).await? {
                // We got the lock
                return Ok(true);
            }

            // Check if tables were created while we were waiting
            if self.schema_exists().await? {
                debug!("Database schema initialized by another process while waiting");
                return Ok(false);
            }
        }

        // Final check
        if self.try_acquire_schema_lock(lock_id).await? {
            Ok(true)
        } else if self.schema_exists().await? {
            debug!("Database schema initialized by another process");
            Ok(false)
        } else {
            Err(PostgresError::Migration(
                "Failed to acquire schema initialization lock after multiple attempts".to_string(),
            ))
        }
    }

    /// Initialize the database schema
    ///
    /// This method creates the necessary tables and indexes for the event store.
    /// It is idempotent and can be called multiple times safely.
    /// Uses `PostgreSQL` advisory locks to prevent concurrent initialization conflicts.
    #[allow(clippy::too_many_lines)]
    pub async fn initialize(&self) -> Result<(), PostgresError> {
        // Use PostgreSQL advisory lock to prevent concurrent schema initialization
        // Lock ID 123456789 is arbitrary but consistent for schema initialization
        const SCHEMA_LOCK_ID: i64 = 123_456_789;

        // Try to acquire advisory lock
        let mut lock_acquired = self.try_acquire_schema_lock(SCHEMA_LOCK_ID).await?;

        if !lock_acquired {
            // Another process is initializing the schema, wait briefly and check if schema exists
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Check if tables exist - if they do, initialization was completed by another process
            if self.schema_exists().await? {
                debug!("Database schema already initialized by another process");
                return Ok(());
            }

            // Wait for schema initialization or acquire lock
            lock_acquired = self.wait_for_schema_initialization(SCHEMA_LOCK_ID).await?;
            if !lock_acquired {
                // Schema was initialized by another process
                return Ok(());
            }
        }

        // We have the lock, proceed with initialization
        let result = async {
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

            // Create optimized index for multi-stream reads using ANY($1) pattern
            // This composite index supports the common multi-stream read pattern
            sqlx::query(
                "CREATE INDEX IF NOT EXISTS idx_events_multistream_any ON events(stream_id, event_id)",
            )
            .execute(self.pool.as_ref())
            .await
            .map_err(PostgresError::Connection)?;

            // Enable pgcrypto extension for random bytes generation
            sqlx::query("CREATE EXTENSION IF NOT EXISTS pgcrypto")
                .execute(self.pool.as_ref())
                .await
                .map_err(PostgresError::Connection)?;

            // Create function to generate UUIDv7
            sqlx::query(
                r"
                CREATE OR REPLACE FUNCTION gen_uuidv7() RETURNS UUID AS $$
                DECLARE
                    unix_ts_ms BIGINT;
                    uuid_bytes BYTEA;
                BEGIN
                    -- Get current timestamp in milliseconds since Unix epoch
                    unix_ts_ms := (extract(epoch from clock_timestamp()) * 1000)::BIGINT;
                    
                    -- Create a 16-byte UUID:
                    -- Bytes 0-5: 48-bit big-endian timestamp (milliseconds since Unix epoch)
                    -- Bytes 6-7: 4-bit version (0111) + 12 random bits  
                    -- Bytes 8-9: 2-bit variant (10) + 14 random bits
                    -- Bytes 10-15: 48 random bits
                    
                    -- Build the UUID byte by byte
                    uuid_bytes := 
                        -- Timestamp: 48 bits, big-endian
                        substring(int8send(unix_ts_ms) from 3 for 6) ||
                        -- Version (0111 = 7) + random: 16 bits
                        -- First byte: 0111RRRR (where R is random)
                        set_byte(gen_random_bytes(1), 0, (get_byte(gen_random_bytes(1), 0) & 15) | 112) ||
                        -- Second byte: all random
                        gen_random_bytes(1) ||
                        -- Variant (10) + random: 16 bits  
                        -- First byte: 10RRRRRR (where R is random)
                        set_byte(gen_random_bytes(1), 0, (get_byte(gen_random_bytes(1), 0) & 63) | 128) ||
                        -- Remaining 7 bytes: all random
                        gen_random_bytes(7);
                    
                    RETURN encode(uuid_bytes, 'hex')::UUID;
                END;
                $$ LANGUAGE plpgsql VOLATILE;
                ",
            )
            .execute(self.pool.as_ref())
            .await
            .map_err(PostgresError::Connection)?;

            // Create function for atomic version checking
            sqlx::query(
                r"
                CREATE OR REPLACE FUNCTION check_event_version() RETURNS TRIGGER AS $$
                DECLARE
                    current_max_version BIGINT;
                    expected_version BIGINT;
                BEGIN
                    -- Lock the stream for this transaction to ensure sequential versioning
                    -- This prevents gaps when multiple events are inserted in parallel
                    PERFORM pg_advisory_xact_lock(hashtext(NEW.stream_id));
                    
                    -- Get the current maximum version for this stream
                    -- Returns NULL if no events exist, which we'll treat as 0
                    SELECT COALESCE(MAX(event_version), 0) INTO current_max_version
                    FROM events
                    WHERE stream_id = NEW.stream_id;
                    
                    -- Version checking logic:
                    -- The unique constraint on (stream_id, event_version) prevents duplicates
                    -- but we need to ensure no gaps and handle ExpectedVersion::New properly
                    
                    -- For version 1, this is ExpectedVersion::New - stream MUST be empty
                    IF NEW.event_version = 1 THEN
                        IF current_max_version != 0 THEN
                            RAISE EXCEPTION 'Version conflict for stream %: cannot insert version 1 when stream already has events', 
                                NEW.stream_id
                                USING ERRCODE = '40001'; -- Use serialization_failure error code
                        END IF;
                    ELSE
                        -- For versions > 1, ensure no gaps in the sequence
                        expected_version := current_max_version + 1;
                        IF NEW.event_version != expected_version THEN
                            RAISE EXCEPTION 'Version gap detected for stream %: expected version %, got %', 
                                NEW.stream_id, expected_version, NEW.event_version
                                USING ERRCODE = '40001'; -- Use serialization_failure error code
                        END IF;
                    END IF;
                    
                    -- Generate event_id using UUIDv7 (always required since we never pass one)
                    NEW.event_id := gen_uuidv7();
                    
                    RETURN NEW;
                END;
                $$ LANGUAGE plpgsql
                ",
            )
            .execute(self.pool.as_ref())
            .await
            .map_err(PostgresError::Connection)?;

            // Drop existing trigger if it exists
            sqlx::query("DROP TRIGGER IF EXISTS enforce_event_version ON events")
                .execute(self.pool.as_ref())
                .await
                .map_err(PostgresError::Connection)?;

            // Create trigger to enforce sequential versioning
            sqlx::query(
                r"
                CREATE TRIGGER enforce_event_version
                    BEFORE INSERT ON events
                    FOR EACH ROW
                    EXECUTE FUNCTION check_event_version()
                ",
            )
            .execute(self.pool.as_ref())
            .await
            .map_err(PostgresError::Connection)?;

            // Drop the foreign key constraint since we no longer use event_streams table
            sqlx::query("ALTER TABLE events DROP CONSTRAINT IF EXISTS fk_events_stream_id")
                .execute(self.pool.as_ref())
                .await
                .map_err(PostgresError::Connection)?;

            info!("Database schema initialized successfully");
            Ok::<(), PostgresError>(())
        }
        .await;

        // Always release the advisory lock, regardless of success or failure
        let _unlock_result = sqlx::query("SELECT pg_advisory_unlock($1)")
            .bind(SCHEMA_LOCK_ID)
            .execute(self.pool.as_ref())
            .await;

        result
    }

    /// Check database connectivity with comprehensive health checks
    #[instrument(skip(self))]
    pub async fn health_check(&self) -> Result<HealthStatus, PostgresError> {
        let start = std::time::Instant::now();

        // Basic connectivity check
        sqlx::query("SELECT 1")
            .execute(self.pool.as_ref())
            .await
            .map_err(PostgresError::Connection)?;

        let basic_latency = start.elapsed();

        // Advanced health checks
        let pool_status = self.get_pool_status()?;
        let schema_status = self.verify_schema().await?;
        let performance_status = self.check_performance().await?;

        let status = HealthStatus {
            is_healthy: true,
            basic_latency,
            pool_status,
            schema_status,
            performance_status,
            last_check: chrono::Utc::now(),
        };

        debug!("PostgreSQL health check passed: {:?}", status);
        Ok(status)
    }

    /// Get detailed connection pool status
    fn get_pool_status(&self) -> Result<PoolStatus, PostgresError> {
        let pool = self.pool.as_ref();

        Ok(PoolStatus {
            #[allow(clippy::cast_possible_truncation)]
            size: pool.size() as u32,
            #[allow(clippy::cast_possible_truncation)]
            idle: pool.num_idle() as u32,
            is_closed: pool.is_closed(),
        })
    }

    /// Verify that required database schema exists
    async fn verify_schema(&self) -> Result<SchemaStatus, PostgresError> {
        let has_events_table = self.table_exists("events").await?;
        // Note: event_streams table is no longer used as of migration 007
        let has_streams_table = false;
        let has_subscriptions_table = self
            .table_exists("subscription_checkpoints")
            .await
            .unwrap_or(false);

        Ok(SchemaStatus {
            has_events_table,
            has_streams_table,
            has_subscriptions_table,
            is_complete: has_events_table,
        })
    }

    /// Check if a specific table exists
    async fn table_exists(&self, table_name: &str) -> Result<bool, PostgresError> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name = $1 AND table_schema = 'public')"
        )
        .bind(table_name)
        .fetch_one(self.pool.as_ref())
        .await
        .map_err(PostgresError::Connection)?;

        Ok(exists)
    }

    /// Check database performance characteristics
    async fn check_performance(&self) -> Result<PerformanceStatus, PostgresError> {
        let start = std::time::Instant::now();

        // Test a simple query that exercises indexes
        let _count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE created_at > NOW() - INTERVAL '1 minute'",
        )
        .fetch_one(self.pool.as_ref())
        .await
        .unwrap_or(0); // Gracefully handle if events table doesn't exist yet

        let query_latency = start.elapsed();

        Ok(PerformanceStatus {
            query_latency,
            is_performant: query_latency < Duration::from_millis(100), // 100ms threshold
        })
    }

    /// Attempt to recover from connection issues
    pub async fn recover_connection(&self) -> Result<(), PostgresError> {
        debug!("Attempting connection recovery");

        // Close any potentially stale connections
        if !self.pool.is_closed() {
            self.pool.close().await;
        }

        // Test if we can still connect with a new pool
        let test_pool = Self::create_pool(&self.config).await?;

        // Verify basic connectivity
        sqlx::query("SELECT 1")
            .execute(&test_pool)
            .await
            .map_err(PostgresError::Connection)?;

        test_pool.close().await;

        info!("Connection recovery completed successfully");
        Ok(())
    }

    /// Get current pool metrics
    pub fn get_pool_metrics(&self) -> monitoring::PoolMetrics {
        let pool_status = PoolStatus {
            #[allow(clippy::cast_possible_truncation)]
            size: self.pool.size() as u32,
            #[allow(clippy::cast_possible_truncation)]
            idle: self.pool.num_idle() as u32,
            is_closed: self.pool.is_closed(),
        };
        self.monitor.get_metrics(&pool_status)
    }

    /// Get pool monitor for advanced monitoring setup
    pub fn monitor(&self) -> Arc<monitoring::PoolMonitor> {
        Arc::clone(&self.monitor)
    }

    /// Start background pool monitoring task
    pub fn start_pool_monitoring(
        &self,
    ) -> (
        tokio::task::JoinHandle<()>,
        tokio::sync::watch::Sender<bool>,
    ) {
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let monitor = Arc::clone(&self.monitor);
        let pool_ref = Arc::clone(&self.pool);
        let interval = self.config.health_check_interval;

        let task = tokio::spawn(async move {
            let monitoring_task = monitoring::PoolMonitoringTask::new(monitor, interval, stop_rx);

            monitoring_task
                .run(move || PoolStatus {
                    #[allow(clippy::cast_possible_truncation)]
                    size: pool_ref.size() as u32,
                    #[allow(clippy::cast_possible_truncation)]
                    idle: pool_ref.num_idle() as u32,
                    is_closed: pool_ref.is_closed(),
                })
                .await;
        });

        (task, stop_tx)
    }

    /// Read events from multiple streams with pagination support.
    ///
    /// This method is designed for efficiently processing large result sets
    /// without loading all events into memory at once.
    ///
    /// # Arguments
    /// * `stream_ids` - The streams to read from
    /// * `options` - Read options (version filters, max events)
    /// * `continuation_token` - Optional continuation token from previous page
    ///
    /// # Returns
    /// A tuple containing:
    /// * Vector of events for this page
    /// * Optional continuation token for the next page (None if no more results)
    ///
    /// # Example
    /// ```no_run
    /// # use eventcore_postgres::PostgresEventStore;
    /// # use eventcore::{EventId, ReadOptions, StreamId};
    /// # async fn example<E>(store: &PostgresEventStore<E>) -> Result<(), Box<dyn std::error::Error>>
    /// # where E: serde::Serialize + for<'de> serde::de::Deserialize<'de> + Send + Sync + Clone + std::fmt::Debug + PartialEq + Eq + 'static
    /// # {
    /// let stream_ids = vec![StreamId::try_new("stream-1")?, StreamId::try_new("stream-2")?];
    /// let options = ReadOptions::default();
    ///
    /// let mut continuation = None;
    /// loop {
    ///     let (events, next_token) = store.read_paginated(&stream_ids, &options, continuation).await?;
    ///     
    ///     // Process events for this page
    ///     for event in events {
    ///         println!("Processing event: {:?}", event.event_id);
    ///     }
    ///     
    ///     // Check if there are more pages
    ///     match next_token {
    ///         Some(token) => continuation = Some(token),
    ///         None => break, // No more pages
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read_paginated(
        &self,
        stream_ids: &[StreamId],
        options: &ReadOptions,
        continuation_token: Option<EventId>,
    ) -> Result<(Vec<StoredEvent<E>>, Option<EventId>), EventStoreError>
    where
        E: Serialize
            + for<'de> Deserialize<'de>
            + Send
            + Sync
            + Clone
            + std::fmt::Debug
            + PartialEq
            + Eq
            + 'static,
    {
        // Delegate to the implementation
        self.read_streams_paginated_impl(stream_ids, options, continuation_token)
            .await
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

    /// Set the query timeout
    #[must_use]
    pub const fn query_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.config.query_timeout = timeout;
        self
    }

    /// Set the health check interval
    #[must_use]
    pub const fn health_check_interval(mut self, interval: Duration) -> Self {
        self.config.health_check_interval = interval;
        self
    }

    /// Set the maximum retries
    #[must_use]
    pub const fn max_retries(mut self, retries: u32) -> Self {
        self.config.max_retries = retries;
        self
    }

    /// Set the retry base delay
    #[must_use]
    pub const fn retry_base_delay(mut self, delay: Duration) -> Self {
        self.config.retry_base_delay = delay;
        self
    }

    /// Set the retry max delay
    #[must_use]
    pub const fn retry_max_delay(mut self, delay: Duration) -> Self {
        self.config.retry_max_delay = delay;
        self
    }

    /// Enable or disable connection recovery
    #[must_use]
    pub const fn enable_recovery(mut self, enable: bool) -> Self {
        self.config.enable_recovery = enable;
        self
    }

    /// Set the read batch size for multi-stream queries
    #[must_use]
    pub const fn read_batch_size(mut self, size: usize) -> Self {
        self.config.read_batch_size = size;
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
        self.config.query_timeout = Some(Duration::from_secs(10)); // Fast query timeout
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
        assert_eq!(config.read_batch_size, 1000); // Default batch size
    }

    #[test]
    fn test_postgres_config_builder() {
        let config = PostgresConfigBuilder::new()
            .database_url("postgres://user:pass@localhost/test")
            .max_connections(20)
            .min_connections(2)
            .connect_timeout(Duration::from_secs(10))
            .test_before_acquire(false)
            .read_batch_size(2000)
            .build();

        assert_eq!(config.database_url, "postgres://user:pass@localhost/test");
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.min_connections, 2);
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
        assert!(!config.test_before_acquire);
        assert_eq!(config.read_batch_size, 2000);
    }

    #[test]
    fn test_postgres_error_conversion() {
        let postgres_error = PostgresError::Transaction("test error".to_string());
        let event_store_error: EventStoreError = postgres_error.into();

        matches!(event_store_error, EventStoreError::TransactionRollback(_));
    }

    #[test]
    fn test_postgres_config_new() {
        let config = PostgresConfig::new("postgres://custom/db");
        assert_eq!(config.database_url, "postgres://custom/db");
        // Should use defaults for other fields
        assert_eq!(config.max_connections, 20);
        assert_eq!(config.min_connections, 2);
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_postgres_config_all_fields() {
        let config = PostgresConfig {
            database_url: "postgres://test/db".to_string(),
            max_connections: 50,
            min_connections: 10,
            connect_timeout: Duration::from_secs(5),
            query_timeout: Some(Duration::from_secs(60)),
            max_lifetime: Some(Duration::from_secs(3600)),
            idle_timeout: Some(Duration::from_secs(300)),
            test_before_acquire: true,
            max_retries: 5,
            retry_base_delay: Duration::from_millis(200),
            retry_max_delay: Duration::from_secs(10),
            enable_recovery: false,
            health_check_interval: Duration::from_secs(60),
            read_batch_size: 500,
        };

        assert_eq!(config.max_connections, 50);
        assert_eq!(config.min_connections, 10);
        assert_eq!(config.connect_timeout, Duration::from_secs(5));
        assert_eq!(config.query_timeout, Some(Duration::from_secs(60)));
        assert_eq!(config.max_lifetime, Some(Duration::from_secs(3600)));
        assert_eq!(config.idle_timeout, Some(Duration::from_secs(300)));
        assert!(config.test_before_acquire);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.retry_base_delay, Duration::from_millis(200));
        assert_eq!(config.retry_max_delay, Duration::from_secs(10));
        assert!(!config.enable_recovery);
        assert_eq!(config.health_check_interval, Duration::from_secs(60));
    }

    #[test]
    fn test_postgres_config_builder_all_methods() {
        let config = PostgresConfigBuilder::new()
            .database_url("postgres://builder/test")
            .max_connections(25)
            .min_connections(3)
            .connect_timeout(Duration::from_secs(8))
            .max_lifetime(Some(Duration::from_secs(1200)))
            .idle_timeout(Some(Duration::from_secs(240)))
            .test_before_acquire(true)
            .query_timeout(Some(Duration::from_secs(45)))
            .build();

        assert_eq!(config.database_url, "postgres://builder/test");
        assert_eq!(config.max_connections, 25);
        assert_eq!(config.min_connections, 3);
        assert_eq!(config.connect_timeout, Duration::from_secs(8));
        assert_eq!(config.max_lifetime, Some(Duration::from_secs(1200)));
        assert_eq!(config.idle_timeout, Some(Duration::from_secs(240)));
        assert!(config.test_before_acquire);
        assert_eq!(config.query_timeout, Some(Duration::from_secs(45)));
    }

    #[test]
    fn test_postgres_config_builder_performance_optimized() {
        let config = PostgresConfigBuilder::new()
            .database_url("postgres://perf/test")
            .performance_optimized()
            .build();

        assert_eq!(config.database_url, "postgres://perf/test");
        assert_eq!(config.max_connections, 30);
        assert_eq!(config.min_connections, 5);
        assert_eq!(config.connect_timeout, Duration::from_secs(5));
        assert_eq!(config.query_timeout, Some(Duration::from_secs(10)));
        assert_eq!(config.max_lifetime, Some(Duration::from_secs(1800)));
        assert_eq!(config.idle_timeout, Some(Duration::from_secs(300)));
        assert!(!config.test_before_acquire);
    }

    #[test]
    fn test_pool_status_fields() {
        let status = PoolStatus {
            size: 10,
            idle: 3,
            is_closed: false,
        };

        assert_eq!(status.size, 10);
        assert_eq!(status.idle, 3);
        assert!(!status.is_closed);
    }

    #[test]
    fn test_health_status_fields() {
        let health = HealthStatus {
            is_healthy: true,
            basic_latency: Duration::from_millis(5),
            pool_status: PoolStatus {
                size: 20,
                idle: 15,
                is_closed: false,
            },
            schema_status: SchemaStatus {
                has_events_table: true,
                has_streams_table: true,
                has_subscriptions_table: false,
                is_complete: true,
            },
            performance_status: PerformanceStatus {
                query_latency: Duration::from_millis(10),
                is_performant: true,
            },
            last_check: chrono::Utc::now(),
        };

        assert!(health.is_healthy);
        assert_eq!(health.basic_latency, Duration::from_millis(5));
        assert_eq!(health.pool_status.size, 20);
        assert!(health.schema_status.is_complete);
        assert!(health.performance_status.is_performant);
    }
}
