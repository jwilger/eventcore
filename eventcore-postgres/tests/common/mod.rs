//! Shared test fixtures for eventcore-postgres integration tests.
//!
//! Uses testcontainers to spin up ephemeral Postgres containers on-demand.
//! Each test gets an isolated database instance with automatic cleanup.

// Allow dead_code because not all test binaries use all exports from this module
#![allow(dead_code)]

use std::env;

use eventcore::{Event, StreamId};
use eventcore_postgres::PostgresEventStore;
use serde::{Deserialize, Serialize};
use testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

/// Get the Postgres version to use for tests.
///
/// Reads from `POSTGRES_VERSION` env var, defaults to "17".
pub fn postgres_version() -> String {
    env::var("POSTGRES_VERSION").unwrap_or_else(|_| "17".to_string())
}

/// A test fixture that manages a Postgres container and store.
///
/// The container is kept alive as long as this struct exists.
/// When dropped, the container is automatically stopped and removed.
pub struct PostgresTestFixture {
    /// The Postgres event store connected to the container.
    pub store: PostgresEventStore,
    /// The connection string for direct database access (e.g., verification queries).
    pub connection_string: String,
    /// The container handle - kept alive to prevent cleanup.
    #[allow(dead_code)]
    container: ContainerAsync<Postgres>,
}

impl PostgresTestFixture {
    /// Create a new test fixture with an ephemeral Postgres container.
    ///
    /// Starts a Postgres container (version from `POSTGRES_VERSION` env var, default "17"),
    /// waits for it to be ready, runs migrations, and returns a connected store.
    pub async fn new() -> Self {
        let version = postgres_version();
        let container = Postgres::default()
            .with_tag(&version)
            .start()
            .await
            .expect("should start postgres container");

        // Get the dynamically assigned host port
        let host_port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("should get postgres port");

        let connection_string = format!(
            "postgres://postgres:postgres@127.0.0.1:{}/postgres",
            host_port
        );

        // Create store and run migrations
        let store = PostgresEventStore::new(connection_string.clone())
            .await
            .expect("should connect to postgres container");

        store.migrate().await;

        Self {
            store,
            connection_string,
            container,
        }
    }
}

/// Generate a unique stream ID for parallel test execution.
///
/// Uses UUIDv7 to ensure uniqueness across concurrent test runs.
pub fn unique_stream_id(prefix: &str) -> StreamId {
    StreamId::try_new(format!("{}-{}", prefix, Uuid::now_v7())).expect("valid stream id")
}

/// A simple test event for integration tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestEvent {
    pub stream_id: StreamId,
    pub payload: String,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }
}
