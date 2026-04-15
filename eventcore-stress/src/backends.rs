use std::env;

use sqlx::postgres::PgPoolOptions;

use crate::config::BackendChoice;

/// Create the postgres connection string from environment variables.
pub fn postgres_connection_string() -> String {
    let port = env::var("POSTGRES_PORT").unwrap_or_else(|_| "5432".to_string());
    let host = env::var("POSTGRES_HOST").unwrap_or_else(|_| "localhost".to_string());
    let user = env::var("POSTGRES_USER").unwrap_or_else(|_| "postgres".to_string());
    let password = env::var("POSTGRES_PASSWORD").unwrap_or_else(|_| "postgres".to_string());
    let db = env::var("POSTGRES_DB").unwrap_or_else(|_| "postgres".to_string());
    format!("postgres://{user}:{password}@{host}:{port}/{db}")
}

/// Truncate all eventcore tables in the PostgreSQL database.
///
/// This removes stale data from contract tests and previous stress test runs
/// so each stress test starts from a clean state. TRUNCATE bypasses the
/// row-level delete-prevention trigger on `eventcore_events`.
pub async fn clean_postgres_database() -> Result<(), Box<dyn std::error::Error>> {
    let conn = postgres_connection_string();
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&conn)
        .await?;

    sqlx::query("TRUNCATE TABLE eventcore_events, eventcore_subscription_versions")
        .execute(&pool)
        .await?;

    println!("Cleaned PostgreSQL database (truncated eventcore tables)");
    Ok(())
}

/// Print which backend is being used.
pub fn print_backend_info(backend: &BackendChoice) {
    match backend {
        BackendChoice::Memory => println!("Using in-memory backend"),
        BackendChoice::Sqlite => println!("Using SQLite in-memory backend"),
        BackendChoice::Postgres => {
            println!(
                "Using PostgreSQL backend at {}",
                postgres_connection_string()
            );
        }
    }
}
