//! Backend contract suite for PostgresEventStore.
//!
//! Uses the unified `backend_contract_tests!` macro to run ALL contract tests.
//! When new tests are added to eventcore-testing, they automatically run here.

use std::env;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use eventcore_postgres::PostgresEventStore;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

/// Singleton to ensure container is started only once across all tests.
static POSTGRES_CONTAINER: OnceLock<()> = OnceLock::new();

fn ensure_postgres_running() {
    let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
    let connection_string = format!("postgres://postgres:postgres@localhost:{}/postgres", port);

    if can_connect_to_postgres(&connection_string) {
        eprintln!("Postgres already available at {}", connection_string);
        return;
    }

    eprintln!("Postgres not accessible, attempting to start via docker-compose...");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let project_root = std::path::Path::new(manifest_dir)
        .parent()
        .expect("should have parent directory");

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
            if !stderr.contains("already in use") && !stderr.contains("already exists") {
                eprintln!("Warning: docker-compose failed: {}", stderr);
            }
        }
        Err(e) => {
            eprintln!("Warning: docker command not available: {}", e);
        }
    }

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
        "Postgres is not accessible at {} after {} retries.",
        connection_string, max_retries
    );
}

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

/// A test fixture that creates an isolated database for each test.
struct IsolatedPostgresFixture {
    connection_string: String,
}

impl IsolatedPostgresFixture {
    async fn new() -> Self {
        let _ = POSTGRES_CONTAINER.get_or_init(|| {
            ensure_postgres_running();
        });

        let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
        let admin_conn_string = format!("postgres://postgres:postgres@localhost:{}/postgres", port);

        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&admin_conn_string)
            .await
            .expect("Failed to connect to postgres database");

        let database_name = format!("test_{}", Uuid::now_v7().simple());

        let _ = sqlx::query(&format!("CREATE DATABASE {}", database_name))
            .execute(&admin_pool)
            .await
            .expect("Failed to create isolated database");

        let connection_string = format!(
            "postgres://postgres:postgres@localhost:{}/{}",
            port, database_name
        );

        let store = PostgresEventStore::new(connection_string.clone())
            .await
            .expect("Failed to connect to isolated database");

        store.migrate().await;

        Self { connection_string }
    }
}

mod postgres_contract_suite {
    use eventcore_postgres::{
        PostgresCheckpointStore, PostgresEventStore, PostgresProjectorCoordinator,
    };
    use eventcore_testing::contract::backend_contract_tests;

    use crate::IsolatedPostgresFixture;

    fn make_store() -> PostgresEventStore {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let fixture = IsolatedPostgresFixture::new().await;
                PostgresEventStore::new(fixture.connection_string)
                    .await
                    .expect("should connect to isolated test database")
            })
        })
    }

    fn make_checkpoint_store() -> PostgresCheckpointStore {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let fixture = IsolatedPostgresFixture::new().await;
                PostgresCheckpointStore::new(fixture.connection_string)
                    .await
                    .expect("should connect to isolated test database")
            })
        })
    }

    fn make_coordinator() -> PostgresProjectorCoordinator {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let fixture = IsolatedPostgresFixture::new().await;
                PostgresProjectorCoordinator::new(fixture.connection_string)
                    .await
                    .expect("should connect to isolated test database")
            })
        })
    }

    backend_contract_tests! {
        suite = postgres,
        make_store = || {
            crate::postgres_contract_suite::make_store()
        },
        make_checkpoint_store = || {
            crate::postgres_contract_suite::make_checkpoint_store()
        },
        make_coordinator = || {
            crate::postgres_contract_suite::make_coordinator()
        },
    }
}
