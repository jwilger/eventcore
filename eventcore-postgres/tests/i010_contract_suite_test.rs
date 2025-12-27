mod common;

mod postgres_contract_suite {
    use eventcore_postgres::PostgresEventStore;
    use eventcore_testing::contract::{event_reader_contract_tests, event_store_contract_tests};

    use crate::common::IsolatedPostgresFixture;

    fn make_store() -> PostgresEventStore {
        // Use block_in_place to allow blocking within multi-threaded tokio runtime
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Event reader contract tests query across all events in the database
                // so they need true database-level isolation (not just stream-level)
                let fixture = IsolatedPostgresFixture::new().await;
                PostgresEventStore::new(fixture.connection_string)
                    .await
                    .expect("should connect to isolated test database")
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
