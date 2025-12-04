mod postgres_contract_suite {
    use std::env;

    use futures::executor::block_on;

    use eventcore_postgres::PostgresEventStore;
    use eventcore_testing::contract::event_store_contract_tests;

    fn postgres_connection_string() -> String {
        env::var("DATABASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                "postgres://postgres:postgres@localhost:5433/eventcore_test".to_string()
            })
    }

    async fn make_store() -> PostgresEventStore {
        let connection_string = postgres_connection_string();

        let store = PostgresEventStore::new(connection_string.clone())
            .await
            .expect("contract suite should construct postgres event store");

        store.migrate().await;

        store
    }

    event_store_contract_tests! {
        suite = postgres_contract,
        make_store = || {
            crate::postgres_contract_suite::block_on(crate::postgres_contract_suite::make_store())
        },
    }
}
