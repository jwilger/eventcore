use std::env;

use sqlx::{Row, postgres::PgPoolOptions};

use eventcore_postgres::PostgresEventStore;

fn postgres_connection_string() -> String {
    env::var("EVENTCORE_TEST_POSTGRES_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "postgres://postgres:postgres@localhost:5433/eventcore_test".to_string())
}

async fn drop_events_table(connection_string: &str) -> Result<(), sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(connection_string)
        .await?;

    sqlx::query("DROP TABLE IF EXISTS eventcore_events CASCADE")
        .execute(&pool)
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

async fn table_exists(connection_string: &str, table: &str) -> bool {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(connection_string)
        .await
        .expect("should connect to postgres when checking table existence");

    let row = sqlx::query("SELECT to_regclass($1)::text AS table_name")
        .bind(table)
        .fetch_one(&pool)
        .await
        .expect("should query postgres catalog for table existence");

    let regclass: Option<String> = row
        .try_get("table_name")
        .expect("should read table_name column when checking table existence");

    regclass.is_some()
}

#[tokio::test]
async fn developer_connects_to_postgres_and_pings() {
    // Given: Developer has a PostgreSQL connection string
    let connection_string = postgres_connection_string();

    // When: Developer creates an event store and pings the database
    let ping_result = async move {
        let store = PostgresEventStore::new(connection_string).await?;
        store.ping().await
    }
    .await;

    // Then: Developer confirms the Postgres adapter responds to ping
    ping_result.expect("postgres event store should accept connections and respond to ping");
}

#[tokio::test]
async fn developer_runs_migrations_and_creates_events_table() {
    // Given: Developer starts from a clean database
    let connection_string = postgres_connection_string();
    drop_events_table(&connection_string)
        .await
        .expect("developer should be able to drop events table before migrations");
    let store = PostgresEventStore::new(connection_string.clone())
        .await
        .expect("developer should be able to construct postgres event store before migrations");

    // When: Developer calls migrate on the event store
    store
        .migrate()
        .await
        .expect("developer migrations should succeed");

    // Then: Events table exists after migrations
    let table_exists = table_exists(&connection_string, "eventcore_events").await;
    assert!(table_exists, "events table should exist after migrations");
}
