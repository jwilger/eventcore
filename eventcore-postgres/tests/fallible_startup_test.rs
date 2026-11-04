//! Fallible startup APIs preserve database errors and legacy panic behavior.

use std::env;
use std::error::Error;
use std::time::Duration;

use eventcore_postgres::{PostgresCheckpointStore, PostgresEventStore};
use eventcore_types::{CheckpointStore, StreamPosition};
use sqlx::PgPool;
use sqlx::migrate::MigrateError;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

#[path = "common/fixture_lifecycle.rs"]
mod fixture_lifecycle;

type TestError = Box<dyn Error + Send + Sync>;

struct TestDatabase {
    name: String,
    admin: Option<PgPool>,
    pool: Option<PgPool>,
}

#[tokio::test]
async fn fresh_database_migrations_are_usable_and_idempotent() {
    // Given an owned database that has never had EventCore migrations applied.
    let database = TestDatabase {
        name: format!("startup_{}", Uuid::now_v7().simple()),
        admin: None,
        pool: None,
    };
    fixture_lifecycle::run_fixture(
        database,
        fixture_lifecycle::FixtureTimeouts::new(Duration::from_secs(30)),
        |database| {
            Box::pin(async move {
                let host = env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".into());
                let port = env::var("POSTGRES_PORT")
                    .unwrap_or_else(|_| "5433".into())
                    .parse::<u16>()?;
                let options = PgConnectOptions::new()
                    .host(&host)
                    .port(port)
                    .username("postgres")
                    .password("postgres")
                    .database("postgres");
                database.admin = Some(
                    PgPoolOptions::new()
                        .max_connections(1)
                        .acquire_timeout(Duration::from_secs(5))
                        .connect_with(options.clone())
                        .await?,
                );
                let admin = database.admin.as_ref().expect("admin pool was assigned");
                let _ = sqlx::query(&format!("CREATE DATABASE {}", database.name))
                    .execute(admin)
                    .await?;
                database.pool = Some(
                    PgPoolOptions::new()
                        .max_connections(2)
                        .acquire_timeout(Duration::from_secs(5))
                        .connect_with(options.database(&database.name))
                        .await?,
                );
                Ok::<(), TestError>(())
            })
        },
        |database| {
            Box::pin(async move {
                let pool = database.pool.as_ref().expect("database was initialized");
                let store = PostgresEventStore::from_pool(pool.clone());
                let ledger: Option<String> =
                    sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations')::text")
                        .fetch_one(pool)
                        .await?;
                assert_eq!(ledger, None);

                // When connectivity is checked before migrations, then it succeeds independently
                // of schema readiness. Applying migrations makes the public checkpoint API usable.
                let ping: Result<(), sqlx::Error> = store.try_ping().await;
                ping?;
                let migration: Result<(), MigrateError> = store.try_migrate().await;
                migration?;
                let checkpoints = PostgresCheckpointStore::from_pool(pool.clone());
                let position = StreamPosition::new(Uuid::now_v7());
                checkpoints.save("startup-proof", position).await?;
                assert_eq!(checkpoints.load("startup-proof").await?, Some(position));
                let ledger_before: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as(
                    "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version",
                )
                .fetch_all(pool)
                .await?;
                assert!(!ledger_before.is_empty());
                assert!(ledger_before.iter().all(|(_, _, success)| *success));

                // When migrations run again through both APIs, then existing data and the ledger
                // remain unchanged. The legacy connectivity success path remains available too.
                store.try_migrate().await?;
                store.migrate().await;
                store.ping().await;
                assert_eq!(checkpoints.load("startup-proof").await?, Some(position));
                let ledger_after: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as(
                    "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version",
                )
                .fetch_all(pool)
                .await?;
                assert_eq!(ledger_after, ledger_before);

                // A migration-specific validation failure also survives the public boundary.
                let version = ledger_before[0].0;
                let _ = sqlx::query("UPDATE _sqlx_migrations SET checksum = $1 WHERE version = $2")
                    .bind(Vec::<u8>::new())
                    .bind(version)
                    .execute(pool)
                    .await?;
                assert!(matches!(
                    store.try_migrate().await,
                    Err(MigrateError::VersionMismatch(actual)) if actual == version
                ));
                Ok(())
            })
        },
        |database| {
            Box::pin(async move {
                if let Some(pool) = database.pool.take() {
                    pool.close().await;
                }
                if let Some(admin) = database.admin.take() {
                    let result = sqlx::query(&format!("DROP DATABASE IF EXISTS {}", database.name))
                        .execute(&admin)
                        .await;
                    admin.close().await;
                    let _ = result?;
                }
                Ok(())
            })
        },
    )
    .await
    .expect("startup fixture must initialize, exercise the APIs, and clean up");
}

async fn closed_pool_store() -> PostgresEventStore {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://postgres:postgres@localhost/postgres")
        .expect("valid test connection URL");
    pool.close().await;
    PostgresEventStore::from_pool(pool)
}

#[tokio::test]
async fn closed_pool_returns_concrete_ping_and_migration_errors() {
    // Given a closed pool, without opening any database connections.
    let store = closed_pool_store().await;

    // When the fallible APIs are called, then their original typed errors are returned.
    let ping: Result<(), sqlx::Error> = store.try_ping().await;
    assert!(matches!(ping, Err(sqlx::Error::PoolClosed)));
    let migration: Result<(), MigrateError> = store.try_migrate().await;
    assert!(matches!(
        migration,
        Err(MigrateError::Execute(sqlx::Error::PoolClosed))
    ));
}

#[tokio::test]
#[should_panic(expected = "postgres ping failed")]
async fn legacy_ping_retains_its_failure_panic() {
    // Given a closed pool, when the legacy API is called, then its panic prefix is preserved.
    closed_pool_store().await.ping().await;
}

#[tokio::test]
#[should_panic(expected = "postgres migration failed")]
async fn legacy_migrate_retains_its_failure_panic() {
    // Given a closed pool, when the legacy API is called, then its panic prefix is preserved.
    closed_pool_store().await.migrate().await;
}
