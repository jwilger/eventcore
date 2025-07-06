//! Concrete resource implementations
//!
//! This module contains specific implementations of resource management
//! for common types like database connections and mutex locks.

/// Database connection resource implementation
#[cfg(feature = "postgres")]
pub mod database {
    use crate::resource::{async_trait, states, Resource, ResourceError, ResourceManager, ResourceResult};
    use sqlx::{PgPool, Postgres, Transaction};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// A database connection pool wrapped in resource management
    pub type DatabasePool = Resource<Arc<PgPool>, states::Acquired>;

    /// A database transaction wrapped in resource management
    pub type DatabaseTransaction<'a> = Resource<Transaction<'a, Postgres>, states::Acquired>;

    /// Database connection wrapped in resource management  
    pub type DatabaseConnection = Resource<sqlx::pool::PoolConnection<Postgres>, states::Acquired>;

    /// Database pool resource manager with health monitoring
    pub struct DatabaseResourceManager {
        pool: Arc<PgPool>,
        last_health_check: std::sync::Mutex<Instant>,
        health_check_interval: Duration,
    }

    impl DatabaseResourceManager {
        /// Create a new database resource manager
        pub fn new(pool: Arc<PgPool>) -> Self {
            Self {
                pool,
                last_health_check: std::sync::Mutex::new(Instant::now()),
                health_check_interval: Duration::from_secs(30),
            }
        }

        /// Create a new database resource manager with custom health check interval
        pub fn new_with_health_interval(
            pool: Arc<PgPool>,
            health_check_interval: Duration,
        ) -> Self {
            Self {
                pool,
                last_health_check: std::sync::Mutex::new(Instant::now()),
                health_check_interval,
            }
        }

        /// Check if health check is needed
        fn needs_health_check(&self) -> bool {
            self.last_health_check.lock().map_or(true, |last_check| {
                last_check.elapsed() > self.health_check_interval
            })
        }

        /// Perform health check and update timestamp
        async fn perform_health_check(&self) -> ResourceResult<()> {
            // Basic connectivity check
            sqlx::query("SELECT 1")
                .execute(self.pool.as_ref())
                .await
                .map_err(|e| {
                    ResourceError::AcquisitionFailed(format!("Health check failed: {e}"))
                })?;

            // Update health check timestamp
            if let Ok(mut last_check) = self.last_health_check.lock() {
                *last_check = Instant::now();
            }

            Ok(())
        }
    }

    #[async_trait]
    impl ResourceManager<Arc<PgPool>> for DatabaseResourceManager {
        async fn acquire() -> ResourceResult<Resource<Arc<PgPool>, states::Acquired>> {
            // Static method - cannot access instance data
            Err(ResourceError::AcquisitionFailed(
                "DatabaseResourceManager::acquire requires instance method".to_string(),
            ))
        }
    }

    impl DatabaseResourceManager {
        /// Acquire a database pool resource with health checking
        pub async fn acquire_pool(&self) -> ResourceResult<DatabasePool> {
            // Perform health check if needed
            if self.needs_health_check() {
                self.perform_health_check().await?;
            }

            // Verify pool is not closed
            if self.pool.is_closed() {
                return Err(ResourceError::AcquisitionFailed(
                    "Connection pool is closed".to_string(),
                ));
            }

            Ok(Resource::new(Arc::clone(&self.pool)))
        }

        /// Acquire a single database connection resource
        pub async fn acquire_connection(&self) -> ResourceResult<DatabaseConnection> {
            // Perform health check if needed
            if self.needs_health_check() {
                self.perform_health_check().await?;
            }

            // Acquire a connection from the pool
            let connection = self.pool.acquire().await.map_err(|e| {
                ResourceError::AcquisitionFailed(format!("Failed to acquire connection: {e}"))
            })?;

            Ok(Resource::new(connection))
        }

        /// Begin a transaction with resource management
        pub async fn begin_transaction(&self) -> ResourceResult<DatabaseTransaction<'_>> {
            // Perform health check if needed
            if self.needs_health_check() {
                self.perform_health_check().await?;
            }

            // Begin transaction
            let transaction = self.pool.begin().await.map_err(|e| {
                ResourceError::AcquisitionFailed(format!("Failed to begin transaction: {e}"))
            })?;

            Ok(Resource::new(transaction))
        }
    }

    /// Extension methods for database pool resources
    impl DatabasePool {
        /// Execute a query using the resource
        ///
        /// Only available when the resource is acquired
        pub async fn execute_query(
            &self,
            query: &str,
        ) -> ResourceResult<sqlx::postgres::PgQueryResult> {
            sqlx::query(query)
                .execute(self.get().as_ref())
                .await
                .map_err(|e| ResourceError::InvalidState(format!("Query execution failed: {e}")))
        }

        /// Fetch one row from a query
        ///
        /// Only available when the resource is acquired
        pub async fn fetch_one(&self, query: &str) -> ResourceResult<sqlx::postgres::PgRow> {
            sqlx::query(query)
                .fetch_one(self.get().as_ref())
                .await
                .map_err(|e| ResourceError::InvalidState(format!("Query fetch failed: {e}")))
        }

        /// Get the connection pool
        ///
        /// Only available when the resource is acquired
        pub const fn pool(&self) -> &Arc<PgPool> {
            self.get()
        }

        /// Check pool health
        ///
        /// Only available when the resource is acquired
        pub fn pool_stats(&self) -> PoolStats {
            let pool = self.get();
            PoolStats {
                size: pool.size(),
                idle: u32::try_from(pool.num_idle()).unwrap_or(u32::MAX),
                is_closed: pool.is_closed(),
            }
        }
    }

    /// Extension methods for database connection resources
    impl DatabaseConnection {
        /// Execute a query using the connection
        ///
        /// Only available when the resource is acquired
        pub async fn execute_query(
            &mut self,
            query: &str,
        ) -> ResourceResult<sqlx::postgres::PgQueryResult> {
            sqlx::query(query)
                .execute(&mut **self.get_mut())
                .await
                .map_err(|e| ResourceError::InvalidState(format!("Query execution failed: {e}")))
        }

        // Note: To begin a transaction, use DatabasePool::begin_transaction() instead.
        // Pool connections should not create their own transactions as this can lead
        // to lifetime issues and is not the recommended SQLx pattern.
    }

    /// Extension methods for database transaction resources
    impl DatabaseTransaction<'_> {
        /// Execute a query within the transaction
        ///
        /// Only available when the resource is acquired
        pub async fn execute_query(
            &mut self,
            query: &str,
        ) -> ResourceResult<sqlx::postgres::PgQueryResult> {
            sqlx::query(query)
                .execute(&mut **self.get_mut())
                .await
                .map_err(|e| ResourceError::InvalidState(format!("Transaction query failed: {e}")))
        }

        /// Commit the transaction
        ///
        /// Only available when the resource is acquired
        /// Consumes the transaction and returns a released resource
        pub async fn commit(self) -> ResourceResult<Resource<(), states::Released>> {
            self.into_inner().commit().await.map_err(|e| {
                ResourceError::ReleaseFailed(format!("Transaction commit failed: {e}"))
            })?;

            Ok(Resource::new(()))
        }

        /// Rollback the transaction
        ///
        /// Only available when the resource is acquired
        /// Consumes the transaction and returns a released resource
        pub async fn rollback(self) -> ResourceResult<Resource<(), states::Released>> {
            self.into_inner().rollback().await.map_err(|e| {
                ResourceError::ReleaseFailed(format!("Transaction rollback failed: {e}"))
            })?;

            Ok(Resource::new(()))
        }
    }

    /// Pool statistics for monitoring
    #[derive(Debug, Clone)]
    pub struct PoolStats {
        /// Current pool size
        pub size: u32,
        /// Number of idle connections
        pub idle: u32,
        /// Whether the pool is closed
        pub is_closed: bool,
    }

    /// Factory for creating database resource managers
    pub struct DatabaseResourceFactory;

    impl DatabaseResourceFactory {
        /// Create a resource manager from an existing pool
        pub fn from_pool(pool: Arc<PgPool>) -> DatabaseResourceManager {
            DatabaseResourceManager::new(pool)
        }

        /// Create a resource manager with custom health check interval
        pub fn from_pool_with_health_interval(
            pool: Arc<PgPool>,
            health_check_interval: Duration,
        ) -> DatabaseResourceManager {
            DatabaseResourceManager::new_with_health_interval(pool, health_check_interval)
        }
    }
}

/// Lock resource implementation with phantom types
pub mod locking {
    use crate::resource::{states, Resource, ResourceError, ResourceResult};
    use std::marker::PhantomData;
    use std::sync::{Arc, Mutex, MutexGuard};

    /// A mutex lock wrapped in resource management
    pub type MutexResource<T> = Resource<Arc<Mutex<T>>, states::Acquired>;

    /// A mutex guard wrapped in resource management
    ///
    /// This ensures the guard is only used while the lock is held
    pub struct MutexGuardResource<'a, T> {
        guard: MutexGuard<'a, T>,
        _phantom: PhantomData<states::Acquired>,
    }

    impl<'a, T> MutexGuardResource<'a, T> {
        /// Create a new mutex guard resource
        const fn new(guard: MutexGuard<'a, T>) -> Self {
            Self {
                guard,
                _phantom: PhantomData,
            }
        }

        /// Access the protected data
        pub fn get(&self) -> &T {
            &self.guard
        }

        /// Access the protected data mutably
        pub fn get_mut(&mut self) -> &mut T {
            &mut self.guard
        }
    }

    /// Extension methods for mutex resources
    impl<T> MutexResource<T> {
        /// Acquire the mutex lock
        ///
        /// Returns a guard resource that enforces lock is held
        pub fn lock(&self) -> ResourceResult<MutexGuardResource<'_, T>> {
            let guard = self
                .get()
                .lock()
                .map_err(|e| ResourceError::Poisoned(format!("Mutex poisoned: {e}")))?;
            Ok(MutexGuardResource::new(guard))
        }

        /// Try to acquire the mutex lock without blocking
        ///
        /// Returns None if the lock is currently held
        pub fn try_lock(&self) -> ResourceResult<Option<MutexGuardResource<'_, T>>> {
            match self.get().try_lock() {
                Ok(guard) => Ok(Some(MutexGuardResource::new(guard))),
                Err(std::sync::TryLockError::WouldBlock) => Ok(None),
                Err(std::sync::TryLockError::Poisoned(e)) => {
                    Err(ResourceError::Poisoned(format!("Mutex poisoned: {e}")))
                }
            }
        }
    }

    /// Create a mutex resource
    pub fn create_mutex_resource<T>(data: T) -> MutexResource<T> {
        Resource::new(Arc::new(Mutex::new(data)))
    }
}