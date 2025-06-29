//! # EventCore
//!
//! A multi-stream aggregateless event sourcing library implementing the **aggregate-per-command pattern**.
//!
//! This revolutionary approach eliminates traditional aggregate boundaries in favor of self-contained
//! commands that can read from and write to multiple streams atomically.
//!
//! ## Key Features
//!
//! - **Aggregate-Per-Command Pattern**: Commands define their own state model and processing logic
//! - **Multi-Stream Atomicity**: Commands can atomically read from and write to multiple event streams
//! - **Type-Driven Development**: Uses Rust's type system to make illegal states unrepresentable
//! - **Pluggable Storage**: Support for multiple event store implementations
//! - **Optimistic Concurrency**: Built-in version control and conflict detection
//! - **Performance**: Designed for high-throughput event processing
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use eventcore::{CommandExecutor, Command};
//! use eventcore_memory::InMemoryEventStore;
//!
//! #[tokio::main]
//! async fn main() {
//!     let event_store = InMemoryEventStore::<String>::new();
//!     let executor = CommandExecutor::new(event_store);
//!     // Define and execute your commands...
//! }
//! ```
//!
//! ## Crate Usage Patterns
//!
//! EventCore is designed as a modular system with separate crates for different concerns:
//!
//! ### Core Library + In-Memory Adapter (Testing)
//!
//! For testing and development, use the core library with the in-memory adapter:
//!
//! ```toml
//! [dependencies]
//! eventcore = "0.1"
//! eventcore-memory = "0.1"
//! tokio = { version = "1.0", features = ["full"] }
//! async-trait = "0.1"
//! ```
//!
//! ```rust,no_run
//! use eventcore::{CommandExecutor, Command};
//! use eventcore_memory::InMemoryEventStore;
//!
//! async fn setup_for_testing() {
//!     let event_store = InMemoryEventStore::<String>::new();
//!     let executor = CommandExecutor::new(event_store);
//!     // Use executor for testing...
//! }
//! ```
//!
//! ### Production with PostgreSQL
//!
//! For production deployments with PostgreSQL:
//!
//! ```toml
//! [dependencies]
//! eventcore = "0.1"
//! eventcore-postgres = "0.1"
//! tokio = { version = "1.0", features = ["full"] }
//! sqlx = { version = "0.7", features = ["runtime-tokio-rustls", "postgres"] }
//! async-trait = "0.1"
//! ```
//!
//! ```rust,ignore
//! use eventcore::CommandExecutor;
//! use eventcore_postgres::{PostgresEventStore, PostgresConfig};
//!
//! async fn setup_for_production() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = PostgresConfig::new("postgresql://user:pass@localhost/eventcore".to_string());
//!     let event_store = PostgresEventStore::new(config).await?;
//!     event_store.initialize().await?;
//!     
//!     let executor = CommandExecutor::new(event_store);
//!     // Use executor in production...
//!     Ok(())
//! }
//! ```
//!
//! ### Development with Examples
//!
//! To explore patterns and learn the library:
//!
//! ```toml
//! [dependencies]
//! eventcore = "0.1"
//! eventcore-examples = "0.1"
//! eventcore-memory = "0.1"  # For running examples
//! tokio = { version = "1.0", features = ["full"] }
//! ```
//!
//! ### Benchmarking and Performance Testing
//!
//! For performance testing and benchmarking:
//!
//! ```toml
//! [dev-dependencies]
//! eventcore-benchmarks = "0.1"
//! criterion = { version = "0.5", features = ["async_futures"] }
//! tokio = { version = "1.0", features = ["rt-multi-thread"] }
//! ```
//!
//! ## Adapter Selection Guide
//!
//! Choose the right event store adapter for your use case:
//!
//! ### InMemoryEventStore (`eventcore-memory`)
//! - **Use for**: Unit tests, integration tests, rapid prototyping
//! - **Features**: Fast, thread-safe, no persistence
//! - **Limitations**: Data lost on restart, limited to single process
//!
//! ```rust,no_run
//! use eventcore_memory::InMemoryEventStore;
//! let store = InMemoryEventStore::<String>::new();
//! ```
//!
//! ### PostgresEventStore (`eventcore-postgres`)  
//! - **Use for**: Production applications, high-throughput scenarios
//! - **Features**: ACID transactions, multi-stream atomicity, persistence
//! - **Requirements**: PostgreSQL 12+, proper connection pooling
//!
//! ```rust,ignore
//! use eventcore_postgres::{PostgresEventStore, PostgresConfig};
//!
//! async fn setup_postgres() -> Result<PostgresEventStore, Box<dyn std::error::Error>> {
//!     let config = PostgresConfig::new("postgresql://localhost/eventcore".to_string())
//!         .with_max_connections(10)
//!         .with_connect_timeout(std::time::Duration::from_secs(5));
//!         
//!     let store = PostgresEventStore::new(config).await?;
//!     store.initialize().await?;  // Creates tables if needed
//!     Ok(store)
//! }
//! ```
//!
//! ## Initialization Patterns
//!
//! ### Simple Setup (Testing/Development)
//! ```rust,no_run
//! use eventcore::CommandExecutor;
//! use eventcore_memory::InMemoryEventStore;
//!
//! fn setup_simple() -> CommandExecutor<InMemoryEventStore<String>> {
//!     let event_store = InMemoryEventStore::<String>::new();
//!     CommandExecutor::new(event_store)
//! }
//! ```
//!
//! ### Production Setup with Configuration
//! ```rust,ignore
//! use eventcore::CommandExecutor;
//! use eventcore_postgres::{PostgresEventStore, PostgresConfig};
//! use std::time::Duration;
//!
//! async fn setup_production(database_url: String) -> Result<CommandExecutor<PostgresEventStore>, Box<dyn std::error::Error>> {
//!     let config = PostgresConfig::new(database_url)
//!         .with_max_connections(20)
//!         .with_min_connections(5)
//!         .with_connect_timeout(Duration::from_secs(10))
//!         .with_idle_timeout(Duration::from_secs(600));
//!     
//!     let event_store = PostgresEventStore::new(config).await?;
//!     event_store.initialize().await?;
//!     
//!     Ok(CommandExecutor::new(event_store))
//! }
//! ```
//!
//! ### Dependency Injection Pattern
//! ```rust,no_run
//! use eventcore::{CommandExecutor, EventStore};
//! use std::sync::Arc;
//!
//! struct AppServices<E: EventStore> {
//!     executor: CommandExecutor<E>,
//!     // other services...
//! }
//!
//! impl<E: EventStore> AppServices<E> {
//!     fn new(event_store: E) -> Self {
//!         Self {
//!             executor: CommandExecutor::new(event_store),
//!         }
//!     }
//! }
//! ```
//!
//! ## Architecture
//!
//! `EventCore` is built around a few key abstractions:
//!
//! - [`command::Command`] - Defines the business logic and state model for a specific operation
//! - [`event_store::EventStore`] - Provides storage and retrieval of events
//! - [`executor::CommandExecutor`] - Orchestrates command execution with concurrency control
//! - [`projection::Projection`] - Builds read models from event streams
//!
//! For detailed examples and patterns, see the `eventcore-examples` crate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![allow(dead_code)] // Many public APIs not used internally
#![allow(clippy::doc_markdown)] // Allow proper nouns without backticks

// Internal modules - implementation details
mod command;
mod errors;
mod event;
mod event_store;
mod event_store_adapter;
mod executor;
mod metadata;
mod monitoring;
mod projection;
mod projection_manager;
mod projection_runner;
mod serialization;
mod state_reconstruction;
mod subscription;
mod type_registry;
mod types;

// Public API exports
pub use command::{Command, CommandResult};
pub use errors::{
    CommandError, EventStoreError, ProjectionError, ProjectionResult, ValidationError,
};
pub use event::Event;
pub use event_store::{
    EventStore, EventToWrite, ExpectedVersion, ReadOptions, StoredEvent, StreamData, StreamEvents,
};
pub use executor::{CommandExecutor, ExecutionContext, RetryConfig, RetryPolicy};
pub use metadata::{CausationId, CorrelationId, EventMetadata, UserId};
pub use projection::{Projection, ProjectionCheckpoint, ProjectionConfig, ProjectionStatus};
pub use projection_manager::ProjectionManager;
pub use subscription::{Subscription, SubscriptionImpl, SubscriptionOptions};
pub use types::{EventId, EventVersion, StreamId, Timestamp};

/// Testing utilities for event sourcing applications
#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// Prelude module with commonly used imports
///
/// This module provides a convenient way to import the most commonly used
/// types and traits from `EventCore`. Import it like this:
///
/// ```rust
/// use eventcore::prelude::*;
/// ```
pub mod prelude {

    pub use crate::{
        Command, CommandError, CommandExecutor, CommandResult, Event, EventId, EventMetadata,
        EventStore, EventToWrite, EventVersion, ExpectedVersion, ProjectionResult, ReadOptions,
        StoredEvent, StreamData, StreamEvents, StreamId, Timestamp,
    };

    #[cfg(any(test, feature = "testing"))]
    pub use crate::testing::prelude::*;
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        // Placeholder test
        assert_eq!(2 + 2, 4);
    }
}
