use std::env;

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
