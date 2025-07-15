//! Resource lifecycle management example
//!
//! This example demonstrates how to use `EventCore`'s phantom type resource management
//! system to ensure safe resource acquisition and release with compile-time guarantees.

use eventcore::resource::{
    global_leak_detector, locking::create_mutex_resource, ResourceExt, ResourceResult,
};
use std::time::Duration;

#[cfg(feature = "postgres")]
use eventcore::resource::{database::DatabaseResourceFactory, ResourceError};
#[cfg(feature = "postgres")]
use std::sync::Arc;
#[cfg(feature = "postgres")]
use tokio::time::sleep;

/// Example data structure for demonstration
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct UserAccount {
    id: u64,
    username: String,
    balance: f64,
}

/// Example showing basic resource acquisition and release
#[cfg(feature = "postgres")]
async fn basic_resource_example() -> ResourceResult<()> {
    println!("=== Basic Resource Example ===");

    // Create a mock PostgreSQL pool for demonstration
    // In real usage, you'd create this from your database configuration
    let pool = create_mock_pool().await?;
    let resource_manager = DatabaseResourceFactory::from_pool(Arc::new(pool));

    // Acquire a database pool resource
    let pool_resource = resource_manager.acquire_pool().await?;
    println!("✅ Database pool acquired");

    // Use the resource (only possible when acquired)
    let result = pool_resource
        .execute_query("SELECT COUNT(*) FROM users")
        .await?;
    println!(
        "📊 Query executed: {:?} rows affected",
        result.rows_affected()
    );

    // Check pool statistics
    let stats = pool_resource.pool_stats();
    println!("📈 Pool stats: {} total, {} idle", stats.size, stats.idle);

    // Explicitly release the resource
    pool_resource.release()?;
    println!("🔓 Resource released");

    Ok(())
}

/// Example showing automatic cleanup with scoped resources
#[cfg(feature = "postgres")]
async fn scoped_resource_example() -> ResourceResult<()> {
    println!("\n=== Scoped Resource Example ===");

    let pool = create_mock_pool().await?;
    let resource_manager = DatabaseResourceFactory::from_pool(Arc::new(pool));

    // Use scoped resource for automatic cleanup
    {
        let pool_resource = resource_manager.acquire_pool().await?;
        let mut scoped_resource = pool_resource.scoped();

        println!("✅ Database pool acquired and scoped");

        // Use the resource within the scope
        scoped_resource.with_resource(|_resource| {
            println!("🔧 Using resource within scope");
            // Resource is guaranteed to be available here
        });

        // Resource will be automatically released when scope ends
    } // <- Automatic cleanup happens here

    println!("🔓 Scoped resource automatically cleaned up");

    Ok(())
}

/// Example showing timeout-based resource management
#[cfg(feature = "postgres")]
async fn timed_resource_example() -> ResourceResult<()> {
    println!("\n=== Timed Resource Example ===");

    let pool = create_mock_pool().await?;
    let resource_manager = DatabaseResourceFactory::from_pool(Arc::new(pool));

    // Acquire resource with timeout
    let pool_resource = resource_manager.acquire_pool().await?;
    let timed_guard = pool_resource.with_timeout(Duration::from_secs(2));

    println!("✅ Resource acquired with 2-second timeout");

    // Use the resource while it's valid
    if let Some(_resource) = timed_guard.get() {
        println!("🔧 Using timed resource");
        // Use the resource for some operation
        println!("🔧 Resource in use within timeout");
    }

    // Wait to demonstrate timeout
    println!("⏳ Waiting 1 second...");
    sleep(Duration::from_secs(1)).await;

    if let Some(time_remaining) = timed_guard.time_remaining() {
        println!("⏰ Time remaining: {time_remaining:?}");
    }

    // Release before timeout
    timed_guard.release()?;
    println!("🔓 Timed resource released before timeout");

    Ok(())
}

/// Example showing managed resources with leak detection
#[cfg(feature = "postgres")]
async fn managed_resource_example() -> ResourceResult<()> {
    println!("\n=== Managed Resource Example ===");

    let pool = create_mock_pool().await?;
    let resource_manager = DatabaseResourceFactory::from_pool(Arc::new(pool));

    // Get initial leak statistics
    let initial_stats = global_leak_detector().get_stats();
    println!(
        "📊 Initial active resources: {}",
        initial_stats.total_active
    );

    // Create managed resources for automatic leak detection
    {
        let pool_resource = resource_manager.acquire_pool().await?;
        let _managed1 = pool_resource.managed("DatabasePool");

        let connection_resource = resource_manager.acquire_connection().await?;
        let _managed2 = connection_resource.managed("DatabaseConnection");

        println!("✅ Created 2 managed resources");

        let current_stats = global_leak_detector().get_stats();
        println!("📊 Active resources: {}", current_stats.total_active);
        println!("📊 By type: {:?}", current_stats.by_type);

        // Resources are automatically tracked and cleaned up when dropped
    } // <- Automatic cleanup and leak detection update

    // Check final statistics
    let final_stats = global_leak_detector().get_stats();
    println!("📊 Final active resources: {}", final_stats.total_active);

    Ok(())
}

/// Example showing lock resource management
fn lock_resource_example() -> ResourceResult<()> {
    println!("\n=== Lock Resource Example ===");

    // Create a shared data structure
    let shared_data = UserAccount {
        id: 1,
        username: "alice".to_string(),
        balance: 1000.0,
    };

    // Create a mutex resource
    let mutex_resource = create_mutex_resource(shared_data);
    println!("✅ Mutex resource created");

    // Acquire the lock
    let guard = mutex_resource.lock()?;
    println!("🔒 Lock acquired, balance: ${}", guard.get().balance);

    // Lock is automatically released when guard is dropped
    drop(guard);
    println!("🔓 Lock released");

    // Try to acquire lock (non-blocking)
    if let Some(guard) = mutex_resource.try_lock()? {
        println!("🔒 Non-blocking lock acquired successfully");
        drop(guard);
    } else {
        println!("❌ Non-blocking lock acquisition failed");
    }

    Ok(())
}

/// Example showing database transaction management
#[cfg(feature = "postgres")]
async fn transaction_example() -> ResourceResult<()> {
    println!("\n=== Transaction Example ===");

    let pool = create_mock_pool().await?;
    let resource_manager = DatabaseResourceFactory::from_pool(Arc::new(pool));

    // Begin a transaction
    let mut transaction = resource_manager.begin_transaction().await?;
    println!("✅ Transaction started");

    // Execute queries within the transaction
    let _result = transaction
        .execute_query("INSERT INTO users (username) VALUES ('bob')")
        .await?;
    println!("📝 Query executed within transaction");

    let _result = transaction
        .execute_query("UPDATE accounts SET balance = balance + 100 WHERE user_id = 1")
        .await?;
    println!("📝 Another query executed within transaction");

    // Commit the transaction (consumes the resource)
    let _released = transaction.commit().await?;
    println!("✅ Transaction committed and resource released");

    Ok(())
}

/// Example showing error handling and recovery
#[cfg(feature = "postgres")]
async fn error_handling_example() -> ResourceResult<()> {
    println!("\n=== Error Handling Example ===");

    let pool = create_mock_pool().await?;
    let resource_manager = DatabaseResourceFactory::from_pool(Arc::new(pool));

    // Simulate resource acquisition failure
    // (In real usage, this might happen due to connection limits, etc.)

    match resource_manager.acquire_pool().await {
        Ok(resource) => {
            println!("✅ Resource acquired successfully");

            // Simulate operation failure
            match resource.execute_query("INVALID SQL QUERY").await {
                Ok(_) => println!("📊 Query executed successfully"),
                Err(ResourceError::InvalidState(msg)) => {
                    println!("❌ Query failed: {msg}");
                    // Resource is still valid, can continue using it
                }
                Err(other) => {
                    println!("❌ Unexpected error: {other}");
                    return Err(other);
                }
            }

            resource.release()?;
            println!("🔓 Resource cleaned up after error");
        }
        Err(ResourceError::AcquisitionFailed(msg)) => {
            println!("❌ Resource acquisition failed: {msg}");
            // Handle acquisition failure gracefully
        }
        Err(other) => {
            println!("❌ Unexpected acquisition error: {other}");
            return Err(other);
        }
    }

    Ok(())
}

/// Example showing comprehensive resource lifecycle patterns
#[cfg(feature = "postgres")]
async fn comprehensive_example() -> ResourceResult<()> {
    println!("\n=== Comprehensive Resource Lifecycle Example ===");

    let pool = create_mock_pool().await?;
    let resource_manager = DatabaseResourceFactory::from_pool_with_health_interval(
        Arc::new(pool),
        Duration::from_secs(30),
    );

    // Pattern 1: Simple acquire and release
    {
        let resource = resource_manager.acquire_pool().await?;
        let _stats = resource.pool_stats();
        resource.release()?;
    }

    // Pattern 2: Scoped automatic cleanup
    {
        let resource = resource_manager.acquire_pool().await?;
        let _scope = resource.scoped();
        // Automatic cleanup when scope ends
    }

    // Pattern 3: Managed with leak detection
    {
        let resource = resource_manager.acquire_pool().await?;
        let _managed = resource.managed("ExamplePool");
        // Automatic leak tracking and cleanup
    }

    // Pattern 4: Timed resource with timeout
    {
        let resource = resource_manager.acquire_pool().await?;
        let timed = resource.with_timeout(Duration::from_secs(5));

        // Use resource within timeout
        if let Some(_inner) = timed.get() {
            // Use the resource for some operation
            println!("🔧 Using resource within timed scope");
        }

        timed.release()?;
    }

    // Pattern 5: Transaction with automatic commit/rollback
    {
        let transaction = resource_manager.begin_transaction().await?;
        let _released = transaction.commit().await?;
    }

    println!("✅ All resource patterns demonstrated successfully");

    Ok(())
}

/// Mock function to create a `PostgreSQL` pool for demonstration
/// In real usage, you would use your actual database configuration
#[cfg(feature = "postgres")]
async fn create_mock_pool() -> ResourceResult<sqlx::PgPool> {
    // This is a mock implementation for the example
    // In real usage, you would create the pool from your database URL
    use sqlx::postgres::PgPoolOptions;

    // For this example, we'll try to connect to a local PostgreSQL instance
    // If not available, the example will show error handling
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5432/eventcore".to_string());

    PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .map_err(|e| ResourceError::AcquisitionFailed(format!("Failed to create pool: {e}")))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for better error reporting
    tracing_subscriber::fmt::init();

    println!("🚀 EventCore Resource Lifecycle Management Examples");
    println!("====================================================");

    // Note: These examples require a PostgreSQL database to be running
    // If you don't have one available, some examples will demonstrate error handling

    #[cfg(feature = "postgres")]
    {
        match basic_resource_example().await {
            Ok(()) => {}
            Err(e) => println!("⚠️  Basic example failed (expected if no database): {e}"),
        }

        match scoped_resource_example().await {
            Ok(()) => {}
            Err(e) => println!("⚠️  Scoped example failed (expected if no database): {e}"),
        }

        match timed_resource_example().await {
            Ok(()) => {}
            Err(e) => println!("⚠️  Timed example failed (expected if no database): {e}"),
        }

        match managed_resource_example().await {
            Ok(()) => {}
            Err(e) => println!("⚠️  Managed example failed (expected if no database): {e}"),
        }
    }

    #[cfg(not(feature = "postgres"))]
    {
        println!("⚠️  Database examples skipped (postgres feature not enabled)");
    }

    // Lock example doesn't require database
    lock_resource_example()?;

    #[cfg(feature = "postgres")]
    {
        match transaction_example().await {
            Ok(()) => {}
            Err(e) => println!("⚠️  Transaction example failed (expected if no database): {e}"),
        }

        match error_handling_example().await {
            Ok(()) => {}
            Err(e) => println!("⚠️  Error handling example failed (expected if no database): {e}"),
        }

        match comprehensive_example().await {
            Ok(()) => {}
            Err(e) => println!("⚠️  Comprehensive example failed (expected if no database): {e}"),
        }
    }

    // Show final leak detection statistics
    let final_stats = global_leak_detector().get_stats();
    println!("\n📊 Final Resource Statistics:");
    println!("   Total active resources: {}", final_stats.total_active);
    println!("   By type: {:?}", final_stats.by_type);

    if final_stats.total_active > 0 {
        println!(
            "⚠️  Warning: {} resources still active (potential leaks)",
            final_stats.total_active
        );

        let potential_leaks =
            global_leak_detector().find_potential_leaks(std::time::Duration::from_secs(1));
        if !potential_leaks.is_empty() {
            println!("🔍 Potential leaks: {potential_leaks:?}");
        }
    } else {
        println!("✅ All resources properly cleaned up!");
    }

    println!("\n🎉 Resource lifecycle examples completed!");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use eventcore::resource::{states, Resource, ResourceManager};

    // Test helper to create resources using the public API
    struct TestResourceManager;

    #[async_trait]
    impl ResourceManager<String> for TestResourceManager {
        async fn acquire() -> ResourceResult<Resource<String, states::Acquired>> {
            // Create resource in initializing state, then mark as acquired
            let resource = Self::create_initializing("test".to_string());
            Ok(resource.mark_acquired())
        }
    }

    #[tokio::test]
    async fn test_resource_state_transitions() {
        // Test that resource states work correctly
        let resource = TestResourceManager::acquire().await.unwrap();
        assert_eq!(resource.get(), "test");

        // Test scoped resource
        let scope = resource.scoped();
        assert!(!scope.is_released());

        // Scope automatically releases resource when dropped
        drop(scope);
    }

    #[tokio::test]
    async fn test_managed_resource() {
        let resource = TestResourceManager::acquire().await.unwrap();
        let managed = resource.managed("TestResource");

        // Resource should be tracked
        assert!(managed.get().is_some());

        // Taking resource should work
        let taken = managed.take();
        assert!(taken.is_some());
    }

    #[tokio::test]
    async fn test_timed_resource() {
        let resource = TestResourceManager::acquire().await.unwrap();
        let timed = resource.with_timeout(Duration::from_millis(100));

        // Initially should be available
        assert!(timed.get().is_some());

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should be timed out
        assert!(timed.is_timed_out());
        assert!(timed.get().is_none());
    }

    #[test]
    fn test_leak_detector() {
        let detector = global_leak_detector();
        let initial_count = detector.get_stats().total_active;

        // Register a resource
        detector.register_acquisition("test-123", "TestResource", None);

        let stats = detector.get_stats();
        assert_eq!(stats.total_active, initial_count + 1);
        assert!(stats.by_type.contains_key("TestResource"));

        // Release the resource
        detector.register_release("test-123");

        let final_stats = detector.get_stats();
        assert_eq!(final_stats.total_active, initial_count);
    }
}
