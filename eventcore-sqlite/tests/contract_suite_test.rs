mod sqlite_contract_suite {
    use eventcore_sqlite::{SqliteCheckpointStore, SqliteEventStore, SqliteProjectorCoordinator};
    use eventcore_testing::contract::backend_contract_tests;

    fn make_store() -> SqliteEventStore {
        let store = SqliteEventStore::in_memory().expect("should create in-memory SQLite store");
        // Use block_in_place to run async migrate in sync context
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                store.migrate().await.expect("migration should succeed");
            });
        });
        store
    }

    fn make_checkpoint_store() -> SqliteCheckpointStore {
        let store =
            SqliteCheckpointStore::in_memory().expect("should create in-memory checkpoint store");
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                store.migrate().await.expect("migration should succeed");
            });
        });
        store
    }

    fn make_coordinator() -> SqliteProjectorCoordinator {
        SqliteProjectorCoordinator::new()
    }

    backend_contract_tests! {
        suite = sqlite,
        make_store = || {
            crate::sqlite_contract_suite::make_store()
        },
        make_checkpoint_store = || {
            crate::sqlite_contract_suite::make_checkpoint_store()
        },
        make_coordinator = || {
            crate::sqlite_contract_suite::make_coordinator()
        },
    }
}
