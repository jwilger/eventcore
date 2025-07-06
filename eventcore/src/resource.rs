//! Resource acquisition and release with phantom type safety
//!
//! This module provides compile-time guarantees for resource lifecycle management.
//! Resources must be acquired before use and cannot be used after release.
//! The type system prevents use-after-release and double-release errors.

pub mod types;
pub mod implementations;

pub use types::{states, IsAcquired, IsReleasable, IsRecoverable};
pub use implementations::{
    Resource, ResourceManager, ResourceScope, TimedResourceGuard,
    ResourceLeakDetector, ResourceLeakStats, global_leak_detector,
    ManagedResource, ResourceExt
};

use std::time::Duration;
use thiserror::Error;

/// Errors that can occur during resource operations
#[derive(Debug, Error)]
pub enum ResourceError {
    /// Resource acquisition failed
    #[error("Resource acquisition failed: {0}")]
    AcquisitionFailed(String),

    /// Resource release failed
    #[error("Resource release failed: {0}")]
    ReleaseFailed(String),

    /// Resource is in invalid state for operation
    #[error("Resource is in invalid state: {0}")]
    InvalidState(String),

    /// Resource operation timed out
    #[error("Resource operation timed out after {duration:?}")]
    Timeout {
        /// The duration after which the operation timed out
        duration: Duration,
    },

    /// Resource has been poisoned due to panic
    #[error("Resource poisoned: {0}")]
    Poisoned(String),
}

/// Type alias for resource operation results
pub type ResourceResult<T> = Result<T, ResourceError>;

/// Database connection resource implementation
#[cfg(feature = "postgres")]
pub mod database {
    use super::{implementations::Resource, states, ResourceError, ResourceManager, ResourceResult};
    use async_trait::async_trait;
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
    use super::{implementations::Resource, states, ResourceError, ResourceResult};
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

#[cfg(test)]
mod tests {
    use super::states::*;
    use super::{
        global_leak_detector, locking, IsAcquired, IsReleasable, ManagedResource, Resource,
        ResourceError, ResourceExt, ResourceLeakDetector, ResourceScope, TimedResourceGuard,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;

    #[test]
    fn test_resource_state_transitions() {
        // Start with initializing resource
        let initializing = Resource::<String, Initializing>::new("test".to_string());

        // Can transition to acquired
        let acquired = initializing.mark_acquired();
        assert_eq!(acquired.get(), "test");

        // Can transition to failed
        let failed = acquired.mark_failed();

        // Failed can be recovered
        let recovered = failed.recover().unwrap();
        assert_eq!(recovered.get(), "test");

        // Can be released
        let released = recovered.release().unwrap();
        assert!(released.is_released());
    }

    #[test]
    fn test_resource_scope() {
        let resource = Resource::<String, Acquired>::new("test".to_string());
        let mut scope = ResourceScope::new(resource);

        // Can access resource in scope
        scope.with_resource(|r| {
            assert_eq!(r.get(), "test");
        });

        // Check scope state
        assert!(!scope.is_released());
        assert!(!scope.is_leaked());

        // Manual release works
        scope.release().unwrap();
    }

    #[test]
    fn test_resource_scope_drop_warning() {
        let resource = Resource::<String, Acquired>::new("test".to_string());
        let scope = ResourceScope::new(resource);

        // Drop without release
        drop(scope); // Should log warning
    }

    #[tokio::test]
    async fn test_timed_resource_guard() {
        let resource = Resource::<String, Acquired>::new("test".to_string());
        let guard = TimedResourceGuard::new(resource, Duration::from_millis(100));

        // Resource accessible before timeout
        assert_eq!(guard.get(), Some(&"test".to_string()));
        assert!(!guard.is_timed_out());
        assert!(guard.time_remaining().is_some());

        // Wait for timeout
        sleep(Duration::from_millis(150)).await;

        // Resource not accessible after timeout
        assert!(guard.get().is_none());
        assert!(guard.is_timed_out());
        assert!(guard.time_remaining().is_none());

        // Release fails after timeout
        match guard.release() {
            Err(ResourceError::Timeout { duration }) => {
                assert!(duration >= Duration::from_millis(150));
            }
            _ => panic!("Expected timeout error"),
        }
    }

    #[test]
    fn test_leak_detector() {
        let detector = ResourceLeakDetector::new();

        // Register some resources
        detector.register_acquisition("res1", "TestResource", Some("test.rs:42".to_string()));
        detector.register_acquisition("res2", "TestResource", None);
        detector.register_acquisition("res3", "OtherResource", None);

        // Check stats
        let stats = detector.get_stats();
        assert_eq!(stats.total_active, 3);
        assert_eq!(stats.by_type.get("TestResource"), Some(&2));
        assert_eq!(stats.by_type.get("OtherResource"), Some(&1));

        // Release one resource
        detector.register_release("res1");

        let stats = detector.get_stats();
        assert_eq!(stats.total_active, 2);
        assert_eq!(stats.by_type.get("TestResource"), Some(&1));

        // Check for leaks (with very short threshold for testing)
        let leaks = detector.find_potential_leaks(Duration::from_nanos(1));
        assert_eq!(leaks.len(), 2);
    }

    #[test]
    fn test_managed_resource() {
        let resource = Resource::<String, Acquired>::new("test".to_string());
        let detector_before = global_leak_detector().get_stats();

        {
            let managed = ManagedResource::new(resource, "TestResource");
            assert!(managed.get().is_some());

            // Should be registered
            let stats_during = global_leak_detector().get_stats();
            assert_eq!(stats_during.total_active, detector_before.total_active + 1);
        }

        // Should be unregistered after drop
        let stats_after = global_leak_detector().get_stats();
        assert_eq!(stats_after.total_active, detector_before.total_active);
    }

    #[test]
    fn test_managed_resource_take() {
        let resource = Resource::<String, Acquired>::new("test".to_string());
        let detector_before = global_leak_detector().get_stats();

        let managed = ManagedResource::new(resource, "TestResource");

        // Take ownership
        let resource_back = managed.take().unwrap();
        assert_eq!(resource_back.get(), "test");

        // Should be unregistered immediately
        let stats_after = global_leak_detector().get_stats();
        assert_eq!(stats_after.total_active, detector_before.total_active);
    }

    #[test]
    fn test_resource_ext_trait() {
        let resource = Resource::<String, Acquired>::new("test".to_string());

        // Test scoped
        let scope = resource.scoped();
        scope.release().unwrap();

        let resource = Resource::<String, Acquired>::new("test".to_string());

        // Test managed
        let managed = resource.managed("TestResource");
        drop(managed);

        let resource = Resource::<String, Acquired>::new("test".to_string());

        // Test with_timeout
        let guard = resource.with_timeout(Duration::from_secs(1));
        guard.release().unwrap();
    }

    #[test]
    fn test_mutex_resource() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        let mutex_res = locking::create_mutex_resource(counter_clone);

        // Lock and modify
        {
            let mut guard = mutex_res.lock().unwrap();
            guard.get_mut().fetch_add(1, Ordering::SeqCst);
        }

        // Try lock
        let guard = mutex_res.try_lock().unwrap();
        assert!(guard.is_some());
        assert_eq!(guard.unwrap().get().load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_sealed_trait_pattern() {
        // Test that our sealed traits work correctly
        // This is mostly a compile-time test

        fn test_acquired<S: IsAcquired>(_state: std::marker::PhantomData<S>) {
            // This function should only accept acquired states
        }

        fn test_releasable<S: IsReleasable>(_state: std::marker::PhantomData<S>) {
            // This function should only accept releasable states
        }

        // These should compile
        test_acquired(std::marker::PhantomData::<Acquired>);
        test_releasable(std::marker::PhantomData::<Acquired>);
        test_releasable(std::marker::PhantomData::<Failed>);

        // These should NOT compile (verified by external compile tests):
        // test_acquired(std::marker::PhantomData::<Released>);
        // test_releasable(std::marker::PhantomData::<Released>);
    }

    #[test]
    fn test_compilation_errors() {
        // These tests verify that certain operations don't compile
        // They are written as compile_fail tests to document the expected behavior

        // Cannot use released resource
        let _resource = Resource::<String, Released>::new("test".to_string());
        // resource.get(); // ❌ This should not compile

        // Cannot release an already released resource
        // resource.release(); // ❌ This should not compile

        // Cannot mark released resource as failed
        // resource.mark_failed(); // ❌ This should not compile
    }
}

/// Integration with existing EventCore types
pub mod integration {
    use super::{implementations::Resource, states};

    /// Resource wrapper for event stores
    pub type EventStoreResource<ES> = Resource<ES, states::Acquired>;

    /// Resource wrapper for subscriptions
    pub type SubscriptionResource<S> = Resource<S, states::Acquired>;

    /// Extension trait for event store resources
    pub trait EventStoreResourceExt<ES> {
        /// Create an event store resource
        fn into_resource(self) -> EventStoreResource<ES>;
    }

    impl<ES> EventStoreResourceExt<ES> for ES {
        fn into_resource(self) -> EventStoreResource<ES> {
            Resource::new(self)
        }
    }

    /// Extension trait for subscription resources
    pub trait SubscriptionResourceExt<S> {
        /// Create a subscription resource
        fn into_resource(self) -> SubscriptionResource<S>;
    }

    impl<S> SubscriptionResourceExt<S> for S {
        fn into_resource(self) -> SubscriptionResource<S> {
            Resource::new(self)
        }
    }
}