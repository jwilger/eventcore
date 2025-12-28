//! Backend contract suite for PostgresEventStore.
//!
//! Uses the unified `backend_contract_tests!` macro to run ALL contract tests.
//! When new tests are added to eventcore-testing, they automatically run here.

mod common;

mod postgres_contract_suite {
    use eventcore_postgres::{PostgresCheckpointStore, PostgresEventStore};
    use eventcore_testing::contract::backend_contract_tests;

    use crate::common::IsolatedPostgresFixture;

    fn make_store() -> PostgresEventStore {
        // Use block_in_place to allow blocking within multi-threaded tokio runtime
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Contract tests query across all events in the database
                // so they need true database-level isolation (not just stream-level)
                let fixture = IsolatedPostgresFixture::new().await;
                PostgresEventStore::new(fixture.connection_string)
                    .await
                    .expect("should connect to isolated test database")
            })
        })
    }

    fn make_checkpoint_store() -> PostgresCheckpointStore {
        // Use block_in_place to allow blocking within multi-threaded tokio runtime
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let fixture = IsolatedPostgresFixture::new().await;
                PostgresCheckpointStore::new(fixture.connection_string)
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
    }
}
