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
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

/// Singleton to ensure container is started only once across all tests.
static POSTGRES_CONTAINER: OnceLock<()> = OnceLock::new();

/// Ensure the Postgres container is running via docker-compose.
///
/// This is idempotent - safe to call multiple times.
/// Handles race conditions when multiple test processes try to start the container simultaneously.
fn ensure_postgres_running() {
    // Get the project root directory (two levels up from tests/common)
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let project_root = std::path::Path::new(manifest_dir)
        .parent()
        .expect("should have parent directory");

    // Try to start the container (idempotent - will reuse existing container)
    let result = Command::new("docker")
        .args(["compose", "up", "-d", "--wait"])
        .current_dir(project_root)
        .output()
        .expect("Failed to execute docker compose command");

    // Check if the command failed due to the container already existing (race condition)
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        // If the error is "already in use", that's fine - another test process started it
        if !stderr.contains("already in use") && !stderr.contains("already exists") {
            panic!("Failed to start postgres container: {}", stderr);
        }
    }

    // Verify the container is actually running
    let max_retries = 30;
    let retry_delay = Duration::from_millis(500);
    for attempt in 0..max_retries {
        let status = Command::new("docker")
            .args(["ps", "-q", "-f", "name=eventcore-postgres"])
            .output()
            .expect("Failed to check docker container status");

        if !status.stdout.is_empty() {
            // Container is running
            return;
        }

        if attempt < max_retries - 1 {
            std::thread::sleep(retry_delay);
        }
    }

    panic!(
        "Postgres container did not start after {} retries",
        max_retries
    );
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
/// This provides true database-level isolation for tests that need it
/// by creating a unique database per test instance and dropping it on cleanup.
/// Use this when tests need to query across all events/streams without interference.
///
/// Note: This fixture does NOT own the store - it only manages the database lifecycle.
/// You must create your own store connection using the connection_string field.
pub struct IsolatedPostgresFixture {
    /// The connection string for the isolated database.
    pub connection_string: String,
    /// The name of the isolated database (for cleanup).
    database_name: String,
}

impl IsolatedPostgresFixture {
    /// Create a new isolated test fixture with its own database.
    ///
    /// Ensures the container is running, creates a unique database,
    /// and runs migrations. Returns a connection string that can be used
    /// to create stores.
    pub async fn new() -> Self {
        // Ensure container is running (idempotent)
        POSTGRES_CONTAINER.get_or_init(|| {
            ensure_postgres_running();
        });

        // Create unique database name using UUIDv7
        let database_name = format!("test_{}", Uuid::now_v7().simple());

        // Get admin connection string
        let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
        let admin_conn_string = format!("postgres://postgres:postgres@localhost:{}/postgres", port);

        // Connect to postgres database to create the new database
        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&admin_conn_string)
            .await
            .expect("Failed to connect to postgres database");

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
}

// Note: We don't implement Drop to clean up the database because:
// 1. Drop runs synchronously but cleanup is async
// 2. Creating a new runtime in Drop fails when already inside a runtime
// 3. The database will be cleaned up when `docker compose down -v` is run
// 4. Each test gets a unique database name, so there's no cross-test pollution
