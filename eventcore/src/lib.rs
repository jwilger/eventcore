//! # EventCore
//!
//! A revolutionary event sourcing library that implements the **aggregate-per-command pattern**,
//! enabling atomic operations across multiple event streams without traditional aggregate boundaries.
//!
//! ## What is EventCore?
//!
//! EventCore rethinks event sourcing by eliminating the need for predefined aggregate boundaries.
//! Instead, each command defines its own consistency boundary, reading from and writing to
//! multiple streams atomically. This approach provides unprecedented flexibility while maintaining
//! strong consistency guarantees.
//!
//! ## Key Features
//!
//! - **🎯 Aggregate-Per-Command Pattern**: Commands define their own consistency boundaries
//! - **⚛️ Multi-Stream Atomicity**: Read and write to multiple streams in a single transaction
//! - **🦀 Type-Driven Development**: Leverage Rust's type system for domain modeling
//! - **🔌 Pluggable Storage**: PostgreSQL, in-memory, and custom adapters
//! - **🔄 Optimistic Concurrency**: Version-based conflict detection and resolution
//! - **⚡ High Performance**: Designed for 10,000+ commands/second
//! - **📊 Projections**: Build read models from event streams
//! - **🔍 Event Metadata**: Track causation, correlation, and custom metadata
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use eventcore::prelude::*;
//! use eventcore::{ReadStreams, StreamWrite, StreamResolver};
//! use eventcore_memory::InMemoryEventStore;
//! use async_trait::async_trait;
//! use serde::{Serialize, Deserialize};
//!
//! // Define your events (must derive or implement TryFrom for executor)
//! #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
//! enum BankEvent {
//!     AccountOpened { owner: String, initial_balance: u64 },
//!     MoneyDeposited { amount: u64 },
//!     MoneyWithdrawn { amount: u64 },
//! }
//!
//! // Required for type conversion in executor
//! impl TryFrom<&BankEvent> for BankEvent {
//!     type Error = std::convert::Infallible;
//!
//!     fn try_from(value: &BankEvent) -> Result<Self, Self::Error> {
//!         Ok(value.clone())
//!     }
//! }
//!
//! // Define a command
//! struct OpenAccount;
//!
//! #[async_trait]
//! impl Command for OpenAccount {
//!     type Input = OpenAccountInput;
//!     type State = ();  // No pre-existing state needed
//!     type Event = BankEvent;
//!     type StreamSet = (); // Simple phantom type for this example
//!
//!     fn read_streams(&self, _input: &Self::Input) -> Vec<StreamId> {
//!         vec![]  // New account, no streams to read
//!     }
//!
//!     fn apply(&self, _state: &mut Self::State, _event: &StoredEvent<Self::Event>) {
//!         // No state to update for account opening
//!     }
//!
//!     async fn handle(
//!         &self,
//!         _read_streams: ReadStreams<Self::StreamSet>,
//!         _state: Self::State,
//!         input: Self::Input,
//!         _stream_resolver: &mut StreamResolver,
//!     ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
//!         // Since we don't read streams, we can't write to them in this example
//!         // This is just for demonstration purposes
//!         Ok(vec![])
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Set up the event store and executor
//!     let event_store = InMemoryEventStore::<BankEvent>::new();
//!     let executor = CommandExecutor::new(event_store);
//!
//!     // Execute a command
//!     let input = OpenAccountInput {
//!         account_id: StreamId::try_new("account-123")?,
//!         owner: "Alice".to_string(),
//!         initial_balance: 1000,
//!     };
//!     
//!     executor.execute(&OpenAccount, input, ExecutionOptions::default()).await?;
//!     
//!     Ok(())
//! }
//!
//! # #[derive(Clone)]
//! # struct OpenAccountInput {
//! #     account_id: StreamId,
//! #     owner: String,
//! #     initial_balance: u64,
//! # }
//! ```
//!
//! ## The Aggregate-Per-Command Pattern
//!
//! Traditional event sourcing forces you to define aggregate boundaries upfront, which can
//! become a limitation when business operations span multiple aggregates. EventCore's
//! aggregate-per-command pattern solves this by letting each command define exactly what
//! data it needs.
//!
//! ### Traditional Event Sourcing Challenges
//!
//! ```rust,ignore
//! // Traditional: Forced to use sagas or process managers
//! // for cross-aggregate operations
//! struct TransferMoneySaga {
//!     // Complex coordination logic
//!     // Multiple round trips
//!     // Eventual consistency issues
//! }
//! ```
//!
//! ### EventCore Solution
//!
//! ```rust,ignore
//! // EventCore: Direct, atomic operations across streams
//! impl Command for TransferMoney {
//!     fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
//!         // Read from both accounts atomically
//!         vec![input.from_account, input.to_account]
//!     }
//!
//!     async fn handle(...) -> CommandResult<Vec<(StreamId, Event)>> {
//!         // Write to both accounts atomically
//!         Ok(vec![
//!             (from_account, MoneyDebited { amount }),
//!             (to_account, MoneyCredited { amount }),
//!         ])
//!     }
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
//! ## Core Concepts
//!
//! ### Commands
//!
//! Commands are the heart of EventCore. Each command:
//! - Defines what streams it needs to read
//! - Specifies how to fold events into state
//! - Implements business logic that produces new events
//!
//! ```rust,ignore
//! #[async_trait]
//! impl Command for YourCommand {
//!     type Input = YourInput;    // Self-validating input types
//!     type State = YourState;    // Command-specific state model
//!     type Event = YourEvent;    // Domain events
//!
//!     fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
//!         // Define consistency boundary
//!     }
//!
//!     fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
//!         // Fold events into state
//!     }
//!
//!     async fn handle(
//!         &self,
//!         state: Self::State,
//!         input: Self::Input,
//!     ) -> CommandResult<Vec<(StreamId, Self::Event)>> {
//!         // Pure business logic
//!     }
//! }
//! ```
//!
//! ### Event Stores
//!
//! Event stores provide durable storage with:
//! - Multi-stream atomic writes
//! - Optimistic concurrency control
//! - Global event ordering via UUIDv7
//! - Subscription support for projections
//!
//! ### Type-Driven Design
//!
//! EventCore uses Rust's type system to make illegal states unrepresentable:
//!
//! ```rust,ignore
//! use nutype::nutype;
//!
//! // StreamId is guaranteed non-empty and ≤255 chars
//! #[nutype(sanitize(trim), validate(not_empty, len_char_max = 255))]
//! struct StreamId(String);
//!
//! // EventVersion is guaranteed non-negative
//! #[nutype(validate(greater_or_equal = 0))]
//! struct EventVersion(u64);
//!
//! // Your domain types should follow the same pattern
//! #[nutype(validate(greater = 0))]
//! struct Money(u64);
//! ```
//!
//! ## Architecture Overview
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌────────────┐
//! │   Command   │────▶│   Executor   │────▶│ Event Store│
//! └─────────────┘     └──────────────┘     └────────────┘
//!        │                    │                     │
//!        │                    │                     ▼
//!        │                    │              ┌────────────┐
//!        │                    │              │   Events   │
//!        │                    │              └────────────┘
//!        │                    │                     │
//!        ▼                    ▼                     ▼
//! ┌─────────────┐     ┌──────────────┐     ┌────────────┐
//! │    Input    │     │ Concurrency  │     │ Projection │
//! │ Validation  │     │   Control    │     │   System   │
//! └─────────────┘     └──────────────┘     └────────────┘
//! ```
//!
//! ## Advanced Features
//!
//! ### Projections
//!
//! Build read models from event streams:
//!
//! ```rust,ignore
//! #[async_trait]
//! impl Projection for AccountBalanceProjection {
//!     async fn handle_event(&mut self, event: &StoredEvent<BankEvent>) -> ProjectionResult<()> {
//!         match &event.payload {
//!             BankEvent::MoneyDeposited { amount } => {
//!                 self.balance += amount;
//!             }
//!             BankEvent::MoneyWithdrawn { amount } => {
//!                 self.balance -= amount;
//!             }
//!             _ => {}
//!         }
//!         Ok(())
//!     }
//! }
//! ```
//!
//! ### Event Metadata
//!
//! Track causation, correlation, and custom metadata:
//!
//! ```rust,ignore
//! let metadata = EventMetadata::new()
//!     .with_causation_id(previous_event_id)
//!     .with_correlation_id(correlation_id)
//!     .with_user_id(current_user)
//!     .with_custom("source", json!("web"))
//!     .with_custom("ip_address", json!("192.168.1.1"));
//! ```
//!
//! ### Retry Policies
//!
//! Configure retry behavior for transient failures:
//!
//! ```rust,ignore
//! let retry_config = RetryConfig::default()
//!     .with_max_attempts(3)
//!     .with_initial_delay(Duration::from_millis(100))
//!     .with_policy(RetryPolicy::ExponentialBackoff { factor: 2.0 });
//!
//! executor.execute_with_retry(&command, input, retry_config).await?;
//! ```
//!
//! ## Performance Considerations
//!
//! - **Event Ordering**: UUIDv7 provides chronological ordering without coordination
//! - **Batching**: Write multiple events to multiple streams in one transaction
//! - **Caching**: Commands can cache frequently accessed reference data
//! - **Indexing**: Create indexes on `stream_id` and `event_id` for fast queries
//!
//! ## Error Handling
//!
//! EventCore provides rich error types for different failure scenarios:
//!
//! - `CommandError`: Business logic violations, validation failures
//! - `EventStoreError`: Storage layer issues, version conflicts
//! - `ProjectionError`: Event processing failures
//!
//! ## Getting Help
//!
//! - **Examples**: See the `eventcore-examples` crate
//! - **API Docs**: Run `cargo doc --open`
//! - **GitHub**: <https://github.com/your-org/eventcore>
//!
//! ## Feature Flags
//!
//! - `testing`: Enables test utilities and fixtures
//! - `metrics`: Enables performance metrics collection
//! - `tracing`: Enables distributed tracing support

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
pub use command::{Command, CommandResult, ReadStreams, StreamResolver, StreamWrite};
pub use errors::{
    CommandError, EventStoreError, ProjectionError, ProjectionResult, ValidationError,
};
pub use event::Event;
pub use event_store::{
    EventStore, EventToWrite, ExpectedVersion, ReadOptions, StoredEvent, StreamData, StreamEvents,
};
pub use executor::{CommandExecutor, ExecutionContext, ExecutionOptions, RetryConfig, RetryPolicy};
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
        EventStore, EventToWrite, EventVersion, ExecutionOptions, ExpectedVersion,
        ProjectionResult, ReadOptions, StoredEvent, StreamData, StreamEvents, StreamId, Timestamp,
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
