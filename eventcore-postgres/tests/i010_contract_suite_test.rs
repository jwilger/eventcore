mod common;

mod postgres_contract_suite {
    use std::sync::OnceLock;

    use eventcore_postgres::PostgresEventStore;
    use eventcore_testing::contract::event_store_contract_tests;
    use testcontainers::{Container, ImageExt, runners::SyncRunner};
    use testcontainers_modules::postgres::Postgres;

    use crate::common::postgres_version;

    /// Shared container and connection string for all contract tests.
    /// The container lives for the entire test binary lifetime.
    static SHARED_CONTAINER: OnceLock<SharedPostgres> = OnceLock::new();

    struct SharedPostgres {
        connection_string: String,
        #[allow(dead_code)]
        container: Container<Postgres>,
    }

    fn get_shared_postgres() -> &'static SharedPostgres {
        SHARED_CONTAINER.get_or_init(|| {
            // Run container setup in a separate thread to avoid tokio runtime conflicts
            std::thread::spawn(|| {
                // Use blocking API to start container
                let version = postgres_version();
                let container = Postgres::default()
                    .with_tag(&version)
                    .start()
                    .expect("should start postgres container");

                let host_port = container
                    .get_host_port_ipv4(5432)
                    .expect("should get postgres port");

                let connection_string = format!(
                    "postgres://postgres:postgres@127.0.0.1:{}/postgres",
                    host_port
                );

                // Run migrations using a temporary runtime
                let rt = tokio::runtime::Runtime::new()
                    .expect("should create tokio runtime for migrations");
                rt.block_on(async {
                    let store = PostgresEventStore::new(connection_string.clone())
                        .await
                        .expect("should connect to postgres container");
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
