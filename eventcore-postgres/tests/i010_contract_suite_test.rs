mod common;

mod postgres_contract_suite {
    use std::sync::OnceLock;

    use eventcore_postgres::PostgresEventStore;
    use eventcore_testing::contract::{event_reader_contract_tests, event_store_contract_tests};
    use testcontainers::{Container, ImageExt, runners::SyncRunner};
    use testcontainers_modules::postgres::Postgres;

    use crate::common::postgres_version;

    /// Shared container and connection string for all contract tests in this module.
    /// Tests share a container but are isolated via unique stream IDs with UUIDs.
    static SHARED_CONTAINER: OnceLock<SharedPostgres> = OnceLock::new();

    struct SharedPostgres {
        connection_string: String,
        #[allow(dead_code)]
        container: Container<Postgres>,
    }

    /// Start a container for testing.
    ///
    /// Each test module gets its own container for proper test isolation.
    /// Tests can run in parallel without interfering with each other.
    fn start_container() -> Container<Postgres> {
        let version = postgres_version();
        Postgres::default()
            .with_tag(&version)
            .start()
            .expect("should start postgres container")
    }

    fn get_shared_postgres() -> &'static SharedPostgres {
        SHARED_CONTAINER.get_or_init(|| {
            // Run container setup in a separate thread to avoid tokio runtime conflicts
            std::thread::spawn(|| {
                let container = start_container();

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

    event_reader_contract_tests! {
        suite = postgres_reader_contract,
        make_store = || {
            crate::postgres_contract_suite::make_store()
        },
    }
}
