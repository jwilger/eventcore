//! # EventCore
//!
//! An event sourcing library that implements **multi-stream event sourcing** with dynamic
//! consistency boundaries, enabling atomic operations across multiple event streams.
//!
//! ## What is EventCore?
//!
//! EventCore builds on established event sourcing patterns by eliminating the need for
//! predefined aggregate boundaries. Instead, each command defines its own consistency
//! boundary, reading from and writing to multiple streams atomically. This approach
//! provides flexibility while maintaining strong consistency guarantees.
//!
//! ## Key Features
//!
//! - **🎯 Multi-Stream Event Sourcing**: Commands define their own consistency boundaries
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
//! Here's a complete example showing how to create a simple banking application with EventCore:
//!
//! ```rust,ignore
//! use eventcore::prelude::*;
//! use eventcore::{ReadStreams, StreamWrite, StreamResolver};
//! use eventcore_memory::InMemoryEventStore;
//! use async_trait::async_trait;
//! use serde::{Serialize, Deserialize};
//!
//! // Define your domain events
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
//!     fn try_from(value: &BankEvent) -> Result<Self, Self::Error> {
//!         Ok(value.clone())
//!     }
//! }
//!
//! // Command input type (self-validating through construction)
//! #[derive(Clone)]
//! struct OpenAccountInput {
//!     account_id: StreamId,
//!     owner: String,
//!     initial_balance: u64,
//! }
//!
//! impl OpenAccountInput {
//!     /// Smart constructor ensures valid inputs
//!     fn new(account_id: &str, owner: &str, initial_balance: u64) -> Result<Self, String> {
//!         if owner.trim().is_empty() {
//!             return Err("Owner cannot be empty".to_string());
//!         }
//!         if initial_balance == 0 {
//!             return Err("Initial balance must be greater than zero".to_string());
//!         }
//!         Ok(Self {
//!             account_id: StreamId::try_new(account_id).map_err(|e| e.to_string())?,
//!             owner: owner.to_string(),
//!             initial_balance,
//!         })
//!     }
//! }
//!
//! // Account state for event folding
//! #[derive(Default)]
//! struct AccountState {
//!     exists: bool,
//!     owner: String,
//!     balance: u64,
//! }
//!
//! // OpenAccount command implementation
//! struct OpenAccount;
//!
//! #[async_trait]
//! impl Command for OpenAccount {
//!     type Input = OpenAccountInput;
//!     type State = AccountState;
//!     type Event = BankEvent;
//!     type StreamSet = (); // Phantom type for compile-time stream access control
//!
//!     fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
//!         // Read the account stream to check if it exists
//!         vec![input.account_id.clone()]
//!     }
//!
//!     fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
//!         // Fold events into account state
//!         match &event.payload {
//!             BankEvent::AccountOpened { owner, initial_balance } => {
//!                 state.exists = true;
//!                 state.owner = owner.clone();
//!                 state.balance = *initial_balance;
//!             }
//!             BankEvent::MoneyDeposited { amount } => {
//!                 state.balance += amount;
//!             }
//!             BankEvent::MoneyWithdrawn { amount } => {
//!                 state.balance = state.balance.saturating_sub(*amount);
//!             }
//!         }
//!     }
//!
//!     async fn handle(
//!         &self,
//!         read_streams: ReadStreams<Self::StreamSet>,
//!         state: Self::State,
//!         input: Self::Input,
//!         _stream_resolver: &mut StreamResolver,
//!     ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
//!         // Business rule: account must not already exist
//!         if state.exists {
//!             return Err(CommandError::BusinessRuleViolation(
//!                 format!("Account {} already exists", input.account_id)
//!             ));
//!         }
//!
//!         // Create account opened event with type-safe stream access
//!         let event = StreamWrite::new(
//!             &read_streams,
//!             input.account_id.clone(),
//!             BankEvent::AccountOpened {
//!                 owner: input.owner,
//!                 initial_balance: input.initial_balance,
//!             }
//!         )?;
//!
//!         Ok(vec![event])
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Set up the event store and executor
//!     let event_store = InMemoryEventStore::<BankEvent>::new();
//!     let executor = CommandExecutor::new(event_store);
//!
//!     // Create a new account with validation
//!     let input = OpenAccountInput::new("account-alice", "Alice Smith", 1000)?;
//!     
//!     // Execute the command
//!     let result = executor.execute(&OpenAccount, input, ExecutionOptions::default()).await?;
//!     println!("Account opened successfully! {} events written", result.events_written.len());
//!     
//!     Ok(())
//! }
//! ```
//!
//! ## Multi-Stream Event Sourcing
//!
//! Traditional event sourcing forces you to define aggregate boundaries upfront, which can
//! become a limitation when business operations span multiple aggregates. EventCore's
//! multi-stream event sourcing approach solves this by letting each command define exactly
//! what data it needs.
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
//! ### EventCore Solution: Complete Money Transfer Example
//!
//! ```rust,ignore
//! use eventcore::prelude::*;
//! use eventcore::{ReadStreams, StreamWrite, StreamResolver};
//! use async_trait::async_trait;
//! use serde::{Serialize, Deserialize};
//! use std::collections::HashMap;
//!
//! // Events for account operations
//! #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
//! enum AccountEvent {
//!     AccountOpened { owner: String, initial_balance: u64 },
//!     MoneyDebited { amount: u64, reference: String },
//!     MoneyCredited { amount: u64, reference: String },
//! }
//!
//! impl TryFrom<&AccountEvent> for AccountEvent {
//!     type Error = std::convert::Infallible;
//!     fn try_from(value: &AccountEvent) -> Result<Self, Self::Error> { Ok(value.clone()) }
//! }
//!
//! // Self-validating input type
//! #[derive(Clone)]
//! struct TransferMoneyInput {
//!     from_account: StreamId,
//!     to_account: StreamId,
//!     amount: u64,
//!     reference: String,
//! }
//!
//! impl TransferMoneyInput {
//!     fn new(from: &str, to: &str, amount: u64, reference: &str) -> Result<Self, String> {
//!         if amount == 0 { return Err("Amount must be greater than zero".to_string()); }
//!         if from == to { return Err("Cannot transfer to the same account".to_string()); }
//!         Ok(Self {
//!             from_account: StreamId::try_new(from).map_err(|e| e.to_string())?,
//!             to_account: StreamId::try_new(to).map_err(|e| e.to_string())?,
//!             amount,
//!             reference: reference.to_string(),
//!         })
//!     }
//! }
//!
//! // Transfer state tracks both account balances
//! #[derive(Default)]
//! struct TransferState {
//!     accounts: HashMap<StreamId, (bool, u64)>, // (exists, balance)
//! }
//!
//! // Transfer command: reads from and writes to multiple streams atomically
//! struct TransferMoney;
//!
//! #[async_trait]
//! impl Command for TransferMoney {
//!     type Input = TransferMoneyInput;
//!     type State = TransferState;
//!     type Event = AccountEvent;
//!     type StreamSet = (); // Phantom type for compile-time stream access control
//!     
//!     fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
//!         // Read from both accounts atomically - this is the consistency boundary
//!         vec![input.from_account.clone(), input.to_account.clone()]
//!     }
//!
//!     fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
//!         // Fold events into account state
//!         match &event.payload {
//!             AccountEvent::AccountOpened { initial_balance, .. } => {
//!                 state.accounts.insert(event.stream_id.clone(), (true, *initial_balance));
//!             }
//!             AccountEvent::MoneyDebited { amount, .. } => {
//!                 if let Some((exists, balance)) = state.accounts.get_mut(&event.stream_id) {
//!                     *balance = balance.saturating_sub(*amount);
//!                 }
//!             }
//!             AccountEvent::MoneyCredited { amount, .. } => {
//!                 if let Some((exists, balance)) = state.accounts.get_mut(&event.stream_id) {
//!                     *balance += amount;
//!                 }
//!             }
//!         }
//!     }
//!
//!     async fn handle(
//!         &self,
//!         read_streams: ReadStreams<Self::StreamSet>,
//!         state: Self::State,
//!         input: Self::Input,
//!         _stream_resolver: &mut StreamResolver,
//!     ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
//!         // Check business rules using current state
//!         let from_balance = state.accounts.get(&input.from_account)
//!             .map(|(exists, balance)| if *exists { *balance } else { 0 })
//!             .unwrap_or(0);
//!         
//!         let to_exists = state.accounts.get(&input.to_account)
//!             .map(|(exists, _)| *exists)
//!             .unwrap_or(false);
//!
//!         if from_balance < input.amount {
//!             return Err(CommandError::BusinessRuleViolation(
//!                 format!("Insufficient funds: {} < {}", from_balance, input.amount)
//!             ));
//!         }
//!
//!         if !to_exists {
//!             return Err(CommandError::BusinessRuleViolation(
//!                 format!("Destination account {} does not exist", input.to_account)
//!             ));
//!         }
//!
//!         // Write to both accounts atomically with type-safe stream access
//!         Ok(vec![
//!             StreamWrite::new(
//!                 &read_streams,
//!                 input.from_account,
//!                 AccountEvent::MoneyDebited {
//!                     amount: input.amount,
//!                     reference: input.reference.clone()
//!                 }
//!             )?,
//!             StreamWrite::new(
//!                 &read_streams,
//!                 input.to_account,
//!                 AccountEvent::MoneyCredited {
//!                     amount: input.amount,
//!                     reference: input.reference
//!                 }
//!             )?,
//!         ])
//!     }
//! }
//!
//! // Usage example showing atomic cross-stream operations
//! # #[tokio::main]
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! #     use eventcore_memory::InMemoryEventStore;
//! #     let event_store = InMemoryEventStore::<AccountEvent>::new();
//! #     let executor = CommandExecutor::new(event_store);
//! #
//!       // Transfer money between accounts - all happens in one transaction
//!       let transfer_input = TransferMoneyInput::new(
//!           "account-alice",
//!           "account-bob",
//!           500,
//!           "monthly-allowance"
//!       )?;
//!       
//!       let result = executor.execute(
//!           &TransferMoney,
//!           transfer_input,
//!           ExecutionOptions::default()
//!       ).await?;
//!       
//!       println!("✅ Transfer completed atomically! {} events written",
//!                result.events_written.len());
//! #     Ok(())
//! # }
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
//! ```rust,ignore
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
//! ```rust,ignore
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
//! ```rust,ignore
//! use eventcore::{CommandExecutor, CommandExecutorBuilder};
//! use eventcore_memory::InMemoryEventStore;
//!
//! // Traditional constructor
//! fn setup_simple() -> CommandExecutor<InMemoryEventStore<String>> {
//!     let event_store = InMemoryEventStore::<String>::new();
//!     CommandExecutor::new(event_store)
//! }
//!
//! // Using the fluent builder API
//! fn setup_simple_with_builder() -> CommandExecutor<InMemoryEventStore<String>> {
//!     let event_store = InMemoryEventStore::<String>::new();
//!     CommandExecutorBuilder::new()
//!         .with_store(event_store)
//!         .build()
//! }
//! ```
//!
//! ### Production Setup with Configuration
//! ```rust,ignore
//! use eventcore::{CommandExecutor, CommandExecutorBuilder, RetryPolicy};
//! use eventcore_postgres::{PostgresEventStore, PostgresConfig};
//! use std::time::Duration;
//!
//! // Traditional constructor approach
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
//!
//! // Using builder with advanced configuration
//! async fn setup_production_with_builder(database_url: String) -> Result<CommandExecutor<PostgresEventStore>, Box<dyn std::error::Error>> {
//!     let config = PostgresConfig::new(database_url)
//!         .with_max_connections(20)
//!         .with_min_connections(5)
//!         .with_connect_timeout(Duration::from_secs(10))
//!         .with_idle_timeout(Duration::from_secs(600));
//!     
//!     let event_store = PostgresEventStore::new(config).await?;
//!     event_store.initialize().await?;
//!     
//!     Ok(CommandExecutorBuilder::new()
//!         .with_store(event_store)
//!         .with_fault_tolerant_retry()
//!         .with_retry_policy(RetryPolicy::ConcurrencyAndTransient)
//!         .with_tracing(true)
//!         .build())
//! }
//! ```
//!
//! ### Dependency Injection Pattern
//! ```rust,ignore
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
//! ## Fluent Configuration API
//!
//! EventCore provides a fluent builder pattern for configuring command executors:
//!
//! ```rust,ignore
//! use eventcore::{CommandExecutorBuilder, RetryConfig, RetryPolicy};
//! use eventcore_memory::InMemoryEventStore;
//! use std::time::Duration;
//!
//! // Basic usage
//! let executor = CommandExecutorBuilder::new()
//!     .with_store(InMemoryEventStore::<String>::new())
//!     .build();
//!
//! // Advanced configuration
//! let executor = CommandExecutorBuilder::new()
//!     .with_store(my_event_store)
//!     .with_retry_config(RetryConfig {
//!         max_attempts: 5,
//!         base_delay: Duration::from_millis(100),
//!         max_delay: Duration::from_secs(30),
//!         backoff_multiplier: 2.0,
//!     })
//!     .with_retry_policy(RetryPolicy::ConcurrencyAndTransient)
//!     .with_tracing(true)
//!     .build();
//!
//! // Preset configurations
//! let fast_executor = CommandExecutorBuilder::new()
//!     .with_store(my_event_store)
//!     .with_fast_retry()  // Optimized for high-throughput
//!     .build();
//!
//! let fault_tolerant_executor = CommandExecutorBuilder::new()
//!     .with_store(my_event_store)
//!     .with_fault_tolerant_retry()  // Optimized for reliability
//!     .build();
//!
//! // Simple execution with convenience methods
//! let result = executor.execute_simple(&command, input).await?;
//! let result = executor.execute_with_correlation(&command, input, "req-123".to_string()).await?;
//! let result = executor.execute_as_user(&command, input, "user-456".to_string()).await?;
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
//!     type StreamSet = ();       // Phantom type for compile-time stream access control
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
//!         read_streams: ReadStreams<Self::StreamSet>,
//!         state: Self::State,
//!         input: Self::Input,
//!         stream_resolver: &mut StreamResolver,
//!     ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
//!         // Pure business logic with type-safe stream access
//!         // Can only write to streams declared in read_streams()
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
//! ### Simplified Command Creation with Procedural Macros
//!
//! **Recommended Approach**: Use the `#[derive(Command)]` procedural macro for streamlined command development:
//!
//! ```rust,ignore
//! use eventcore_macros::Command;
//! use eventcore::types::StreamId;
//!
//! #[derive(Command)]
//! struct TransferMoney {
//!     #[stream]
//!     from_account: StreamId,
//!     #[stream]
//!     to_account: StreamId,
//!     amount: Money,
//! }
//!
//! // The macro automatically generates:
//! // - TransferMoneyStreamSet phantom type for compile-time stream access control
//! // - Implementation of read_streams() that returns [from_account, to_account]
//! // - Partial Command trait implementation
//!
//! // You still implement the business logic manually for full control:
//! #[async_trait]
//! impl Command for TransferMoney {
//!     type Input = TransferMoneyInput;
//!     type State = TransferState;
//!     type Event = BankingEvent;
//!     // StreamSet is automatically set by the macro
//!
//!     // read_streams() is automatically implemented
//!     
//!     fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>) {
//!         // Your event folding logic
//!     }
//!
//!     async fn handle(/* ... */) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
//!         // Your business logic with compile-time stream access guarantees
//!     }
//! }
//! ```
//!
//! #### Manual Implementation (Advanced Users)
//!
//! For complete control, you can still implement the `Command` trait manually:
//!
//! ```rust,ignore
//! // This approach is available but not recommended as the primary interface
//! struct TransferMoney;
//!
//! #[async_trait]
//! impl Command for TransferMoney {
//!     type Input = TransferMoneyInput;
//!     type State = TransferState;
//!     type Event = BankingEvent;
//!     type StreamSet = (); // You manage phantom types yourself
//!
//!     fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
//!         vec![input.from_account.clone(), input.to_account.clone()]
//!     }
//!
//!     // ... rest of implementation
//! }
//! ```
//!
//! #### Using Helper Macros in Command Handlers
//!
//! ```rust,ignore
//! use eventcore::prelude::*;
//!
//! async fn handle(
//!     &self,
//!     read_streams: ReadStreams<Self::StreamSet>,
//!     state: Self::State,
//!     input: Self::Input,
//!     stream_resolver: &mut StreamResolver,
//! ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
//!     // Use require! for business rule validation
//!     require!(state.balance >= input.amount, "Insufficient funds");
//!     require!(state.is_active, "Account is not active");
//!     
//!     let mut events = vec![];
//!     
//!     // Use emit! for creating events with type-safe stream access
//!     emit!(events, &read_streams, input.from_account, AccountDebited {
//!         amount: input.amount,
//!         reference: input.reference,
//!     });
//!     
//!     emit!(events, &read_streams, input.to_account, AccountCredited {
//!         amount: input.amount,
//!         reference: input.reference,
//!     });
//!     
//!     Ok(events)
//! }
//! ```
//!
//! ### Projections: Building Read Models
//!
//! Create efficient read models by projecting events into queryable views:
//!
//! ```rust,ignore
//! use eventcore::prelude::*;
//! use async_trait::async_trait;
//! use serde::{Serialize, Deserialize};
//! use std::collections::HashMap;
//!
//! // Domain events
//! #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
//! enum BankEvent {
//!     AccountOpened { owner: String, initial_balance: u64 },
//!     MoneyDeposited { amount: u64 },
//!     MoneyWithdrawn { amount: u64 },
//! }
//!
//! // Account summary for queries
//! #[derive(Debug, Clone, Serialize, Deserialize)]
//! struct AccountSummary {
//!     account_id: String,
//!     owner: String,
//!     current_balance: u64,
//!     total_deposits: u64,
//!     total_withdrawals: u64,
//!     transaction_count: u64,
//!     last_activity: Option<Timestamp>,
//! }
//!
//! // Projection that maintains account summaries
//! struct AccountSummaryProjection {
//!     accounts: HashMap<String, AccountSummary>,
//!     checkpoint: Option<ProjectionCheckpoint>,
//! }
//!
//! impl AccountSummaryProjection {
//!     fn new() -> Self {
//!         Self {
//!             accounts: HashMap::new(),
//!             checkpoint: None,
//!         }
//!     }
//!
//!     /// Query account by ID
//!     fn get_account(&self, account_id: &str) -> Option<&AccountSummary> {
//!         self.accounts.get(account_id)
//!     }
//!
//!     /// Query accounts by owner
//!     fn accounts_by_owner(&self, owner: &str) -> Vec<&AccountSummary> {
//!         self.accounts.values()
//!             .filter(|account| account.owner == owner)
//!             .collect()
//!     }
//!
//!     /// Query accounts with balance above threshold
//!     fn high_balance_accounts(&self, threshold: u64) -> Vec<&AccountSummary> {
//!         self.accounts.values()
//!             .filter(|account| account.current_balance >= threshold)
//!             .collect()
//!     }
//! }
//!
//! #[async_trait]
//! impl Projection for AccountSummaryProjection {
//!     type Event = BankEvent;
//!     type Checkpoint = ProjectionCheckpoint;
//!     type Error = ProjectionError;
//!
//!     async fn handle_event(&mut self, event: &StoredEvent<Self::Event>) -> ProjectionResult<()> {
//!         let account_id = event.stream_id.to_string();
//!         
//!         match &event.payload {
//!             BankEvent::AccountOpened { owner, initial_balance } => {
//!                 let summary = AccountSummary {
//!                     account_id: account_id.clone(),
//!                     owner: owner.clone(),
//!                     current_balance: *initial_balance,
//!                     total_deposits: *initial_balance,
//!                     total_withdrawals: 0,
//!                     transaction_count: 1,
//!                     last_activity: Some(event.timestamp),
//!                 };
//!                 self.accounts.insert(account_id, summary);
//!             }
//!             BankEvent::MoneyDeposited { amount } => {
//!                 if let Some(account) = self.accounts.get_mut(&account_id) {
//!                     account.current_balance += amount;
//!                     account.total_deposits += amount;
//!                     account.transaction_count += 1;
//!                     account.last_activity = Some(event.timestamp);
//!                 }
//!             }
//!             BankEvent::MoneyWithdrawn { amount } => {
//!                 if let Some(account) = self.accounts.get_mut(&account_id) {
//!                     account.current_balance = account.current_balance.saturating_sub(*amount);
//!                     account.total_withdrawals += amount;
//!                     account.transaction_count += 1;
//!                     account.last_activity = Some(event.timestamp);
//!                 }
//!             }
//!         }
//!         
//!         // Update checkpoint to track progress
//!         self.checkpoint = Some(ProjectionCheckpoint::new(event.id));
//!         Ok(())
//!     }
//!
//!     fn checkpoint(&self) -> Option<&Self::Checkpoint> {
//!         self.checkpoint.as_ref()
//!     }
//!
//!     async fn reset(&mut self) -> ProjectionResult<()> {
//!         self.accounts.clear();
//!         self.checkpoint = None;
//!         Ok(())
//!     }
//! }
//!
//! // Usage example
//! # #[tokio::main]
//! # async fn projection_example() -> Result<(), Box<dyn std::error::Error>> {
//! #     use eventcore_memory::InMemoryEventStore;
//! #     let event_store = InMemoryEventStore::<BankEvent>::new();
//!       
//!       // Set up projection
//!       let mut projection = AccountSummaryProjection::new();
//!       
//!       // Process events to build read model
//!       // (In practice, you'd use ProjectionManager for this)
//!       
//!       // Query the projection
//!       if let Some(account) = projection.get_account("account-123") {
//!           println!("Account {} owner: {}, balance: {}",
//!                   account.account_id, account.owner, account.current_balance);
//!       }
//!       
//!       let high_balance = projection.high_balance_accounts(10000);
//!       println!("Found {} high-balance accounts", high_balance.len());
//! #     Ok(())
//! # }
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
//! EventCore provides rich error types with actionable diagnostics for different failure scenarios:
//!
//! ```rust,ignore
//! use eventcore::prelude::*;
//! use eventcore::miette::{Diagnostic, Report};
//! use std::time::Duration;
//! use tokio::time::sleep;
//!
//! // Example showing comprehensive error handling patterns
//! async fn handle_transfer_with_retries(
//!     executor: &CommandExecutor<impl EventStore<Event = AccountEvent>>,
//!     from: &str,
//!     to: &str,
//!     amount: u64
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     let mut attempts = 0;
//!     let max_attempts = 3;
//!     
//!     // Create input with validation
//!     let input = match TransferMoneyInput::new(from, to, amount, "api-transfer") {
//!         Ok(input) => input,
//!         Err(validation_error) => {
//!             eprintln!("❌ Input validation failed: {}", validation_error);
//!             return Err(validation_error.into());
//!         }
//!     };
//!     
//!     loop {
//!         match executor.execute(&TransferMoney, input.clone(), ExecutionOptions::default()).await {
//!             Ok(result) => {
//!                 println!("✅ Transfer successful! {} events written", result.events_written.len());
//!                 return Ok(());
//!             }
//!             
//!             // Handle specific error types with different strategies
//!             Err(CommandError::BusinessRuleViolation(msg)) => {
//!                 eprintln!("❌ Business rule violation: {}", msg);
//!                 return Err(CommandError::BusinessRuleViolation(msg).into());
//!             }
//!             
//!             Err(CommandError::ConcurrencyConflict { streams }) => {
//!                 attempts += 1;
//!                 if attempts >= max_attempts {
//!                     let error = CommandError::ConcurrencyConflict { streams: streams.clone() };
//!                     eprintln!("❌ Max retry attempts reached");
//!                     eprintln!("{:?}", Report::new(error.clone()));
//!                     return Err(error.into());
//!                 }
//!                 
//!                 let delay = Duration::from_millis(100 * 2_u64.pow(attempts - 1));
//!                 println!("⚠️  Concurrency conflict on streams: {:?}", streams);
//!                 println!("🔄 Retrying in {:?} (attempt {}/{})", delay, attempts, max_attempts);
//!                 sleep(delay).await;
//!                 continue;
//!             }
//!             
//!             Err(CommandError::InvalidStreamAccess { stream, declared_streams }) => {
//!                 eprintln!("❌ Invalid stream access detected!");
//!                 eprintln!("   Attempted to access: {}", stream);
//!                 eprintln!("   Declared streams: {:?}", declared_streams);
//!                 eprintln!("   💡 Fix: Add '{}' to your command's read_streams() method", stream);
//!                 return Err(CommandError::InvalidStreamAccess { stream, declared_streams }.into());
//!             }
//!             
//!             Err(CommandError::StreamNotDeclared { stream, command_type }) => {
//!                 eprintln!("❌ Stream not declared in command!");
//!                 eprintln!("   Stream: {}", stream);
//!                 eprintln!("   Command: {}", command_type);
//!                 eprintln!("   💡 Fix: Add stream to read_streams() method to enable write access");
//!                 return Err(CommandError::StreamNotDeclared { stream, command_type }.into());
//!             }
//!             
//!             Err(CommandError::EventStore(store_error)) => {
//!                 eprintln!("❌ Event store error: {}", store_error);
//!                 match store_error {
//!                     EventStoreError::ConnectionFailed(_) => {
//!                         if attempts < max_attempts {
//!                             attempts += 1;
//!                             println!("🔄 Retrying due to connection issue...");
//!                             sleep(Duration::from_millis(1000)).await;
//!                             continue;
//!                         }
//!                     }
//!                     _ => {}
//!                 }
//!                 return Err(CommandError::EventStore(store_error).into());
//!             }
//!             
//!             Err(other_error) => {
//!                 eprintln!("❌ Unexpected error: {}", other_error);
//!                 eprintln!("{:?}", Report::new(other_error.clone()));
//!                 return Err(other_error.into());
//!             }
//!         }
//!     }
//! }
//!
//! // Example showing enhanced error reporting with diagnostics
//! async fn demonstrate_error_diagnostics() {
//!     // This will show rich error messages with helpful hints
//!     let result = handle_transfer_with_retries(
//!         &executor,
//!         "nonexistent-account",
//!         "another-account",
//!         1000
//!     ).await;
//!     
//!     if let Err(error) = result {
//!         // miette provides beautiful, actionable error reports
//!         eprintln!("{:?}", error);
//!     }
//! }
//! ```
//!
//! ### Error Categories
//!
//! - **CommandError**: Business logic violations, validation failures, stream access errors
//! - **EventStoreError**: Storage layer issues, connection failures, version conflicts  
//! - **ProjectionError**: Event processing failures in read model updates
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
mod macros;
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
pub use executor::{
    CommandExecutor, CommandExecutorBuilder, ExecutionContext, ExecutionOptions, RetryConfig,
    RetryPolicy,
};
pub use metadata::{CausationId, CorrelationId, EventMetadata, UserId};
pub use projection::{Projection, ProjectionCheckpoint, ProjectionConfig, ProjectionStatus};
pub use projection_manager::ProjectionManager;
pub use subscription::{
    Checkpoint, EventProcessor, Subscription, SubscriptionError, SubscriptionImpl,
    SubscriptionName, SubscriptionOptions, SubscriptionPosition, SubscriptionResult,
};
pub use types::{EventId, EventVersion, StreamId, Timestamp};

// Re-export miette for enhanced error diagnostics
pub use miette;

/// Testing utilities for event sourcing applications
#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// Interactive tutorials and documentation
///
/// This module contains comprehensive tutorials for learning EventCore.
/// Each tutorial is designed to be interactive and build upon the previous ones.
pub mod docs {
    /// Getting Started Tutorial
    ///
    /// Learn the fundamentals of EventCore by building a simple banking application.
    /// This tutorial covers:
    /// - Defining domain events
    /// - Creating self-validating input types
    /// - Implementing commands with business logic
    /// - Using the event store and executor
    ///
    /// The complete tutorial is available in `docs/tutorials/first-command.md`.
    pub mod first_command {
        /// Writing Your First Command
        ///
        /// This tutorial guides you through creating your first EventCore command.
        /// See `docs/tutorials/first-command.md` for the complete tutorial.
        pub struct Tutorial;
    }

    /// Procedural Macro Tutorial  
    ///
    /// Learn how to use EventCore's `#[derive(Command)]` procedural macro to reduce boilerplate:
    /// - Using `#[derive(Command)]` for automatic stream management
    /// - Stream field annotations with `#[stream]`
    /// - Helper macros like `require!` and `emit!`
    ///
    /// The complete tutorial is available in `docs/tutorials/macro-dsl.md`.
    pub mod macro_dsl {
        /// Using the Procedural Macro
        ///
        /// This tutorial shows how to use EventCore's `#[derive(Command)]` macro for cleaner code.
        /// See `docs/tutorials/macro-dsl.md` for the complete tutorial.
        pub struct Tutorial;
    }

    /// Projections Tutorial
    ///
    /// Build efficient read models from event streams:
    /// - Simple projections for basic queries
    /// - Aggregating projections for analytics
    /// - Time-window projections for recent activity
    /// - Managing projections with ProjectionManager
    ///
    /// The complete tutorial is available in `docs/tutorials/implementing-projections.md`.
    pub mod implementing_projections {
        /// Implementing Projections
        ///
        /// This tutorial covers building read models from event streams.
        /// See `docs/tutorials/implementing-projections.md` for the complete tutorial.
        pub struct Tutorial;
    }

    /// Error Handling Tutorial
    ///
    /// Learn best practices for handling errors in event-sourced systems:
    /// - Understanding different error categories
    /// - Implementing retry strategies
    /// - Using circuit breaker patterns  
    /// - Enhanced error reporting with miette
    ///
    /// The complete tutorial is available in `docs/tutorials/error-handling.md`.
    pub mod error_handling {
        /// Handling Errors Properly
        ///
        /// This tutorial covers error handling best practices.
        /// See `docs/tutorials/error-handling.md` for the complete tutorial.
        pub struct Tutorial;
    }
}

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
        Command, CommandError, CommandExecutor, CommandExecutorBuilder, CommandResult, Event,
        EventId, EventMetadata, EventStore, EventToWrite, EventVersion, ExecutionOptions,
        ExpectedVersion, ProjectionResult, ReadOptions, StoredEvent, StreamData, StreamEvents,
        StreamId, Timestamp,
    };

    // Re-export macros for convenience
    pub use crate::{emit, require};

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
