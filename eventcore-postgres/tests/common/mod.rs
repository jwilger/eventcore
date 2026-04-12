//! Shared test fixtures for eventcore-postgres integration tests.
//!
//! Uses docker-compose to manage a shared Postgres instance across all tests.
//! The container persists between test runs for faster iteration.
//! Clean up with: `docker compose down -v`

use std::env;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use sqlx::postgres::PgPoolOptions;

/// Singleton to ensure container is started only once across all tests.
pub(crate) static POSTGRES_CONTAINER: OnceLock<()> = OnceLock::new();

/// Ensure Postgres is available, either from CI service or docker-compose.
///
/// This is idempotent and works in both CI and local development:
/// - In CI: Postgres is provided as a service container, already running
/// - Locally: Starts postgres via docker-compose if not already running
pub(crate) fn ensure_postgres_running() {
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
fn can_connect_to_postgres(connection_string: &str) -> bool {
    let connection_string = connection_string.to_string();

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
pub(crate) fn connection_string() -> String {
    let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
    format!("postgres://postgres:postgres@localhost:{}/postgres", port)
}

/// Create a PostgresEventStore connected to the shared test container.
///
/// Ensures the container is running (starts it if not), waits for it to be ready,
/// runs migrations, and returns a connected store.
pub(crate) async fn create_test_store() -> eventcore_postgres::PostgresEventStore {
    let _ = POSTGRES_CONTAINER.get_or_init(|| {
        ensure_postgres_running();
    });

    let connection_string = connection_string();

    let max_retries = 30;
    let retry_delay = Duration::from_millis(500);
    let mut store = None;

    for attempt in 0..max_retries {
        match eventcore_postgres::PostgresEventStore::new(connection_string.clone()).await {
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
    store.migrate().await;
    store
}
