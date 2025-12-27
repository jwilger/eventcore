//! Shared test fixtures for eventcore-postgres integration tests.
//!
//! Uses docker-compose to manage a shared Postgres instance across all tests.
//! The container persists between test runs for faster iteration.
//! Clean up with: `docker compose down -v`

// Allow dead_code because not all test binaries use all exports from this module
#![allow(dead_code)]

use std::env;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use eventcore_postgres::PostgresEventStore;
use eventcore_types::{Event, StreamId};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

/// Singleton to ensure container is started only once across all tests.
static POSTGRES_CONTAINER: OnceLock<()> = OnceLock::new();

/// Ensure Postgres is available, either from CI service or docker-compose.
///
/// This is idempotent and works in both CI and local development:
/// - In CI: Postgres is provided as a service container, already running
/// - Locally: Starts postgres via docker-compose if not already running
fn ensure_postgres_running() {
    let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
    let connection_string = format!("postgres://postgres:postgres@localhost:{}/postgres", port);

    // First, check if postgres is already accessible (e.g., CI service container)
    if can_connect_to_postgres(&connection_string) {
        eprintln!("Postgres already available at {}", connection_string);
        return;
    }

    // Postgres not accessible, try to start via docker-compose (local dev)
    eprintln!("Postgres not accessible, attempting to start via docker-compose...");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let project_root = std::path::Path::new(manifest_dir)
        .parent()
        .expect("should have parent directory");

    // Try to start docker-compose (may fail in CI where docker isn't available)
    let result = Command::new("docker")
        .args(["compose", "up", "-d", "--wait"])
        .current_dir(project_root)
        .output();

    match result {
        Ok(output) if output.status.success() => {
            eprintln!("Started postgres via docker-compose");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // "already in use" is fine - another process started it
            if !stderr.contains("already in use") && !stderr.contains("already exists") {
                eprintln!("Warning: docker-compose failed: {}", stderr);
            }
        }
        Err(e) => {
            eprintln!("Warning: docker command not available: {}", e);
        }
    }

    // Verify postgres is now accessible (whether from CI service or docker-compose)
    let max_retries = 30;
    let retry_delay = Duration::from_millis(500);
    for attempt in 0..max_retries {
        if can_connect_to_postgres(&connection_string) {
            eprintln!("Postgres is now accessible");
            return;
        }

        if attempt < max_retries - 1 {
            std::thread::sleep(retry_delay);
        }
    }

    panic!(
        "Postgres is not accessible at {} after {} retries. \
         Ensure either: (1) GitHub Actions service container is configured, \
         or (2) Docker is installed and docker-compose.yml exists",
        connection_string, max_retries
    );
}

/// Check if we can connect to postgres at the given connection string.
///
/// Uses sqlx to attempt a quick connection test. This works with both
/// GitHub Actions service containers and local docker-compose.
///
/// Runs in a separate thread to avoid "runtime within runtime" issues.
fn can_connect_to_postgres(connection_string: &str) -> bool {
    let connection_string = connection_string.to_string();

    // Spawn a new thread to avoid runtime conflicts
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().ok()?;
        rt.block_on(async {
            PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(2))
                .connect(&connection_string)
                .await
                .ok()
        })
    })
    .join()
    .ok()
    .flatten()
    .is_some()
}

/// Get the connection string for the Postgres container.
///
/// Reads `POSTGRES_PORT` from env var, defaults to 5432.
fn connection_string() -> String {
    let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
    format!("postgres://postgres:postgres@localhost:{}/postgres", port)
}

/// A test fixture that manages a Postgres container and store.
///
/// The container is shared across all tests and persists between test runs.
/// This avoids the overhead of starting a new container for each test.
/// Clean up with: `docker compose down -v`
pub struct PostgresTestFixture {
    /// The Postgres event store connected to the container.
    pub store: PostgresEventStore,
    /// The connection string for direct database access (e.g., verification queries).
    pub connection_string: String,
}

impl PostgresTestFixture {
    /// Create a new test fixture with a Postgres container.
    ///
    /// Ensures the container is running (starts it if not), waits for it to be ready,
    /// runs migrations, and returns a connected store.
    pub async fn new() -> Self {
        // Ensure container is running (idempotent, only runs once)
        POSTGRES_CONTAINER.get_or_init(|| {
            ensure_postgres_running();
        });

        let connection_string = connection_string();

        // Wait for postgres to be ready and connect
        // Retry connection in case postgres is still starting up
        let max_retries = 30;
        let retry_delay = Duration::from_millis(500);
        let mut store = None;

        for attempt in 0..max_retries {
            match PostgresEventStore::new(connection_string.clone()).await {
                Ok(s) => {
                    store = Some(s);
                    break;
                }
                Err(e) => {
                    if attempt < max_retries - 1 {
                        eprintln!(
                            "Postgres not ready (attempt {}/{}): {}",
                            attempt + 1,
                            max_retries,
                            e
                        );
                        tokio::time::sleep(retry_delay).await;
                        continue;
                    }
                    panic!(
                        "Failed to connect to postgres after {} retries: {}",
                        max_retries, e
                    );
                }
            }
        }

        let store = store.expect("store should be set after successful connection");

        // Run migrations
        store.migrate().await;

        Self {
            store,
            connection_string,
        }
    }
}

/// Generate a unique stream ID for parallel test execution.
///
/// Uses UUIDv7 to ensure uniqueness across concurrent test runs.
pub fn unique_stream_id(prefix: &str) -> StreamId {
    StreamId::try_new(format!("{}-{}", prefix, Uuid::now_v7())).expect("valid stream id")
}

/// A simple test event for integration tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestEvent {
    pub stream_id: StreamId,
    pub payload: String,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }
}

/// A test fixture that creates an isolated database for each test.
///
/// This provides true database-level isolation for tests that query across all events
/// (e.g., event_reader contract tests). Tests that only access specific streams can use
/// PostgresTestFixture with unique stream IDs instead.
///
/// Cleans up old test databases on creation to prevent accumulation.
pub struct IsolatedPostgresFixture {
    /// The connection string for the isolated database.
    pub connection_string: String,
    /// The name of the isolated database (for tracking).
    pub database_name: String,
}

impl IsolatedPostgresFixture {
    /// Create a new isolated test fixture with its own database.
    ///
    /// Cleans up old test_* databases before creating a new one to prevent resource leaks.
    pub async fn new() -> Self {
        // Ensure container is running (idempotent)
        POSTGRES_CONTAINER.get_or_init(|| {
            ensure_postgres_running();
        });

        let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
        let admin_conn_string = format!("postgres://postgres:postgres@localhost:{}/postgres", port);

        // Connect to postgres database
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&admin_conn_string)
            .await
            .expect("Failed to connect to postgres database");

        // Clean up old test databases to prevent accumulation
        Self::cleanup_old_test_databases(&admin_pool).await;

        // Create unique database name using UUIDv7
        let database_name = format!("test_{}", Uuid::now_v7().simple());

        // Create the isolated database
        sqlx::query(&format!("CREATE DATABASE {}", database_name))
            .execute(&admin_pool)
            .await
            .expect("Failed to create isolated database");

        // Build connection string for the new database
        let connection_string = format!(
            "postgres://postgres:postgres@localhost:{}/{}",
            port, database_name
        );

        // Connect to the new database and run migrations
        let store = PostgresEventStore::new(connection_string.clone())
            .await
            .expect("Failed to connect to isolated database");

        store.migrate().await;

        Self {
            connection_string,
            database_name,
        }
    }

    /// Drop all test_* databases to prevent accumulation from previous test runs.
    async fn cleanup_old_test_databases(admin_pool: &sqlx::PgPool) {
        // Query for all databases matching test_* pattern
        let rows = sqlx::query("SELECT datname FROM pg_database WHERE datname LIKE 'test_%'")
            .fetch_all(admin_pool)
            .await
            .expect("Failed to query test databases");

        // Drop each test database
        for row in rows {
            let db_name: String = row.get("datname");
            eprintln!("Cleaning up old test database: {}", db_name);

            // Terminate connections to the database before dropping
            let terminate_query = format!(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}'",
                db_name
            );
            let _ = sqlx::query(&terminate_query).execute(admin_pool).await;

            // Drop the database
            let drop_query = format!("DROP DATABASE IF EXISTS {}", db_name);
            let _ = sqlx::query(&drop_query).execute(admin_pool).await;
        }
    }
}
