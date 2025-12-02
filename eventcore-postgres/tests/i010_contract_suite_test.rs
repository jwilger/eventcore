mod postgres_contract_suite {
    use std::env;

    use futures::executor::block_on;
    use sqlx::{Executor, postgres::PgPoolOptions};

    use eventcore_postgres::PostgresEventStore;
    use eventcore_testing::contract::event_store_contract_tests;

    fn postgres_connection_string() -> String {
        env::var("EVENTCORE_TEST_POSTGRES_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                "postgres://postgres:postgres@localhost:5433/eventcore_test".to_string()
            })
    }

    async fn clean_database(connection_string: &str) -> Result<(), sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(connection_string)
            .await?;

        pool.execute("DROP TABLE IF EXISTS eventcore_events CASCADE")
            .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS public._sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                success BOOLEAN NOT NULL,
                checksum BYTEA NOT NULL,
                execution_time BIGINT NOT NULL
            )",
        )
        .execute(&pool)
        .await?;

        sqlx::query("DELETE FROM public._sqlx_migrations")
            .execute(&pool)
            .await?;

        Ok(())
    }

    async fn make_store() -> PostgresEventStore {
        let connection_string = postgres_connection_string();

        clean_database(&connection_string)
            .await
            .expect("contract suite should start from clean database");

        let store = PostgresEventStore::new(connection_string.clone())
            .await
            .expect("contract suite should construct postgres event store");

        store
            .migrate()
            .await
            .expect("contract suite migrations should succeed");

        store
    }

    event_store_contract_tests! {
        suite = postgres_contract,
        make_store = || {
            crate::postgres_contract_suite::block_on(crate::postgres_contract_suite::make_store())
        },
    }
}
