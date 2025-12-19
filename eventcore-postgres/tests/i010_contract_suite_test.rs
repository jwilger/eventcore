mod common;

mod postgres_contract_suite {
    use std::sync::OnceLock;

    use eventcore_postgres::PostgresEventStore;
    use eventcore_testing::contract::event_store_contract_tests;
    use testcontainers::{Container, ImageExt, ReuseDirective, runners::SyncRunner};
    use testcontainers_modules::postgres::Postgres;

    use crate::common::postgres_version;

    /// Container name for the shared reusable Postgres instance.
    const CONTAINER_NAME: &str = "eventcore-test-postgres";

    /// Shared container and connection string for all contract tests.
    /// The container persists between test runs for faster iteration.
    static SHARED_CONTAINER: OnceLock<SharedPostgres> = OnceLock::new();

    struct SharedPostgres {
        connection_string: String,
        #[allow(dead_code)]
        container: Container<Postgres>,
    }

    /// Start a reusable container with retry logic for cross-process races.
    ///
    /// When nextest runs test binaries in parallel, multiple processes may try to
    /// create the same named container simultaneously. This retries on "name already
    /// in use" errors, allowing the other process to finish creation.
    fn start_container_with_retry() -> Container<Postgres> {
        let version = postgres_version();
        let max_retries = 10;
        let retry_delay = std::time::Duration::from_millis(500);

        for attempt in 0..max_retries {
            match Postgres::default()
                .with_tag(&version)
                .with_container_name(CONTAINER_NAME)
                .with_reuse(ReuseDirective::Always)
                .start()
            {
                Ok(container) => return container,
                Err(e) => {
                    let error_str = e.to_string();
                    if error_str.contains("already in use") && attempt < max_retries - 1 {
                        // Another process is creating the container, wait and retry
                        std::thread::sleep(retry_delay);
                        continue;
                    }
                    panic!("should start postgres container: {}", e);
                }
            }
        }
        panic!(
            "failed to start postgres container after {} retries",
            max_retries
        );
    }

    fn get_shared_postgres() -> &'static SharedPostgres {
        SHARED_CONTAINER.get_or_init(|| {
            // Run container setup in a separate thread to avoid tokio runtime conflicts
            std::thread::spawn(|| {
                let container = start_container_with_retry();

                let host_port = container
                    .get_host_port_ipv4(5432)
                    .expect("should get postgres port");

                let connection_string = format!(
                    "postgres://postgres:postgres@127.0.0.1:{}/postgres",
                    host_port
                );

                // Run migrations using a temporary runtime
                // Retry connection in case postgres is still starting up
                let rt = tokio::runtime::Runtime::new()
                    .expect("should create tokio runtime for migrations");
                rt.block_on(async {
                    let max_conn_retries = 30;
                    let conn_retry_delay = std::time::Duration::from_millis(500);
                    let mut store = None;

                    for attempt in 0..max_conn_retries {
                        match PostgresEventStore::new(connection_string.clone()).await {
                            Ok(s) => {
                                store = Some(s);
                                break;
                            }
                            Err(e) => {
                                if attempt < max_conn_retries - 1 {
                                    tokio::time::sleep(conn_retry_delay).await;
                                    continue;
                                }
                                panic!(
                                    "should connect to postgres container after {} retries: {}",
                                    max_conn_retries, e
                                );
                            }
                        }
                    }

                    let store = store.expect("store should be set");
                    store.migrate().await;
                });

                SharedPostgres {
                    connection_string,
                    container,
                }
            })
            .join()
            .expect("container setup thread should complete")
        })
    }

    fn make_store() -> PostgresEventStore {
        let shared = get_shared_postgres();
        // Use block_in_place to allow blocking within multi-threaded tokio runtime
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                PostgresEventStore::new(shared.connection_string.clone())
                    .await
                    .expect("should connect to shared postgres container")
            })
        })
    }

    event_store_contract_tests! {
        suite = postgres_contract,
        make_store = || {
            crate::postgres_contract_suite::make_store()
        },
    }
}
