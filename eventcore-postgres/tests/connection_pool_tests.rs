//! Integration tests for `PostgreSQL` connection pool configuration

use eventcore_postgres::{PostgresConfig, PostgresConfigBuilder, PostgresEventStore};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// Test helper to create a test database URL
fn test_database_url() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/eventcore".to_string())
    })
}

#[tokio::test]
async fn test_connection_pool_configuration() {
    let config = PostgresConfigBuilder::new()
        .database_url(test_database_url())
        .max_connections(5)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(5))
        .build();

    let store = PostgresEventStore::<serde_json::Value>::new(config.clone())
        .await
        .expect("Failed to create event store");

    // Verify configuration was applied
    assert_eq!(store.config().max_connections, 5);
    assert_eq!(store.config().min_connections, 1);
    assert_eq!(store.config().connect_timeout, Duration::from_secs(5));
}

#[tokio::test]
async fn test_pool_health_check() {
    let config = PostgresConfig::new(test_database_url());

    let store = PostgresEventStore::<serde_json::Value>::new(config)
        .await
        .expect("Failed to create event store");

    store
        .initialize()
        .await
        .expect("Failed to initialize database");

    let health = store
        .health_check()
        .await
        .expect("Failed to perform health check");

    assert!(health.is_healthy);
    assert!(health.pool_status.size > 0);
    assert!(health.basic_latency < Duration::from_secs(1));
    assert!(health.schema_status.is_complete);
}

#[tokio::test]
async fn test_pool_metrics() {
    let config = PostgresConfig::new(test_database_url());

    let store = PostgresEventStore::<serde_json::Value>::new(config)
        .await
        .expect("Failed to create event store");

    let metrics = store.get_pool_metrics();

    assert!(metrics.current_connections <= 20); // Default max
    assert!(metrics.idle_connections <= metrics.current_connections);
    assert!(metrics.active_connections <= metrics.current_connections);
    assert!(metrics.is_healthy);
    assert!(metrics.utilization_percent >= 0.0);
    assert!(metrics.utilization_percent <= 100.0);
}

#[tokio::test]
async fn test_pool_monitoring_task() {
    let config = PostgresConfigBuilder::new()
        .database_url(test_database_url())
        .health_check_interval(Duration::from_millis(100)) // Fast interval for testing
        .build();

    let store = PostgresEventStore::<serde_json::Value>::new(config)
        .await
        .expect("Failed to create event store");

    let (task, stop_signal) = store.start_pool_monitoring();

    // Let it run for a bit
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Stop the monitoring task
    stop_signal.send(true).expect("Failed to send stop signal");

    // Wait for task to complete
    timeout(Duration::from_secs(1), task)
        .await
        .expect("Task didn't stop in time")
        .expect("Task panicked");
}

#[tokio::test]
async fn test_connection_pool_exhaustion() {
    let config = PostgresConfigBuilder::new()
        .database_url(test_database_url())
        .max_connections(2)
        .connect_timeout(Duration::from_secs(5)) // Normal timeout, test pool exhaustion instead
        .build();

    let store = Arc::new(
        PostgresEventStore::<serde_json::Value>::new(config)
            .await
            .expect("Failed to create event store"),
    );

    store
        .initialize()
        .await
        .expect("Failed to initialize database");

    // Spawn tasks that hold connections
    let mut handles = vec![];

    for i in 0..3 {
        let store = Arc::clone(&store);
        let handle = tokio::spawn(async move {
            let start = Instant::now();
            let result = store.health_check().await;
            let elapsed = start.elapsed();
            (i, result, elapsed)
        });
        handles.push(handle);
    }

    // Wait for all tasks
    let results: Vec<_> = futures::future::join_all(handles).await;

    let mut success_count = 0;
    let mut timeout_count = 0;

    for result in results {
        let (id, health_result, elapsed) = result.expect("Task panicked");
        match health_result {
            Ok(_) => {
                success_count += 1;
                println!("Task {id} succeeded in {elapsed:?}");
            }
            Err(e) => {
                timeout_count += 1;
                println!("Task {id} failed with error: {e} in {elapsed:?}");
            }
        }
    }

    // With only 2 connections and 3 concurrent tasks trying to hold connections,
    // we expect all to eventually succeed as connections are released
    assert!(success_count >= 2, "At least 2 tasks should succeed");
    println!("Success count: {success_count}, Timeout count: {timeout_count}");
}

#[tokio::test]
async fn test_performance_optimized_config() {
    let config = PostgresConfigBuilder::new()
        .database_url(test_database_url())
        .performance_optimized()
        .build();

    // Performance optimized should set specific values
    assert_eq!(config.max_connections, 30);
    assert_eq!(config.min_connections, 5);
    assert_eq!(config.connect_timeout, Duration::from_secs(5));
    assert_eq!(config.query_timeout, Some(Duration::from_secs(10)));
    assert!(!config.test_before_acquire);

    // Verify it works with real connection
    let store = PostgresEventStore::<serde_json::Value>::new(config)
        .await
        .expect("Failed to create event store with performance config");

    store
        .health_check()
        .await
        .expect("Failed health check with performance config");
}

#[tokio::test]
async fn test_query_timeout_configuration() {
    let config = PostgresConfigBuilder::new()
        .database_url(test_database_url())
        .query_timeout(Some(Duration::from_millis(100))) // Very short timeout
        .build();

    let store = PostgresEventStore::<serde_json::Value>::new(config)
        .await
        .expect("Failed to create event store");

    store
        .initialize()
        .await
        .expect("Failed to initialize database");

    // The query timeout is applied at the connection level
    // Basic health check should still work as it's fast
    let health = store
        .health_check()
        .await
        .expect("Health check should succeed");

    assert!(health.is_healthy);
    assert!(health.performance_status.is_performant);
}
