# Chapter 7.1: API Documentation

Complete reference for all EventCore APIs, traits, and public interfaces.

## Core Types

### EventId

Unique identifier for events using UUIDv7 for chronological ordering.

```rust
pub struct EventId(uuid::Uuid);

impl EventId {
    pub fn new_v7() -> Self
    pub fn from(uuid: uuid::Uuid) -> Result<Self, ValidationError>
    pub fn as_uuid(&self) -> &uuid::Uuid
    pub fn is_nil(&self) -> bool
}

// Traits: Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize
```

**Example:**
```rust
let event_id = EventId::new_v7();
assert!(!event_id.is_nil());
```

### StreamId

Identifier for event streams with validation.

```rust
pub struct StreamId(String);

impl StreamId {
    pub fn try_new(value: String) -> Result<Self, ValidationError>
    pub fn from_static(value: &'static str) -> Self
    pub fn cached(value: String) -> Result<Self, ValidationError>
    pub fn as_str(&self) -> &str
    pub fn into_string(self) -> String
}

// Traits: Debug, Clone, PartialEq, Eq, Hash, AsRef<str>, Deref<Target = str>, Serialize, Deserialize
```

**Validation Rules:**
- Must not be empty after trimming
- Maximum length: 255 characters
- Automatically trimmed

**Example:**
```rust
let stream_id = StreamId::try_new("user-12345".to_string())?;
let static_id = StreamId::from_static("system-events");
```

### EventVersion

Version number for events within a stream.

```rust
pub struct EventVersion(u64);

impl EventVersion {
    pub fn initial() -> Self
    pub fn from(value: u64) -> Self
    pub fn next(&self) -> Self
    pub fn as_u64(&self) -> u64
}

// Traits: Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Into<u64>, Serialize, Deserialize
```

**Example:**
```rust
let version = EventVersion::initial(); // Version 0
let next = version.next(); // Version 1
assert_eq!(next.as_u64(), 1);
```

### Money

Type-safe money representation with currency support.

```rust
pub struct Money {
    pub amount: Decimal,
    pub currency: Currency,
}

impl Money {
    pub fn new(amount: Decimal, currency: Currency) -> Self
    pub fn zero(currency: Currency) -> Self
    pub fn from_cents(cents: i64, currency: Currency) -> Self
    pub fn add(&self, other: &Money) -> Result<Money, MoneyError>
    pub fn subtract(&self, other: &Money) -> Result<Money, MoneyError>
    pub fn multiply(&self, factor: Decimal) -> Money
    pub fn is_positive(&self) -> bool
    pub fn is_zero(&self) -> bool
}

// Traits: Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize
```

**Example:**
```rust
let price = Money::from_cents(1299, Currency::USD); // $12.99
let tax = price.multiply(Decimal::new(8, 2))?; // 8% tax
let total = price.add(&tax)?;
```

## Command System

### Command Trait

Core trait for all commands in EventCore.

```rust
#[async_trait]
pub trait Command: Send + Sync + Clone {
    type StreamSet: Send + Sync;
    type State: Default + Send + Sync;
    type Event: Send + Sync;
    
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>>;
    
    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>);
}
```

### CommandStreams Trait

Handles stream access patterns for commands.

```rust
pub trait CommandStreams: Send + Sync + Clone {
    type StreamSet: Send + Sync;
    
    fn read_streams(&self) -> Vec<StreamId>;
    fn __derive_read_streams(&self) -> Vec<StreamId> {
        self.read_streams()
    }
}
```

### CommandLogic Trait

Contains the domain logic for command execution.

```rust
#[async_trait]
pub trait CommandLogic: Send + Sync {
    type StreamSet: Send + Sync;
    type State: Default + Send + Sync;
    type Event: Send + Sync;
    
    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>>;
    
    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>);
}
```

### Command Derive Macro

Automatically implements command infrastructure.

```rust
#[derive(Command, Clone)]
struct TransferMoney {
    #[stream]
    source_account: StreamId,
    
    #[stream] 
    target_account: StreamId,
    
    amount: Money,
    reference: String,
}
```

**Generated Implementation:**
- `CommandStreams` trait implementation
- `StreamSet` type definition  
- `read_streams()` method from `#[stream]` fields
- Integration with `CommandLogic` trait

### CommandExecutor

Executes commands against the event store.

```rust
pub struct CommandExecutor {
    // Private fields
}

impl CommandExecutor {
    pub fn new(event_store: Arc<dyn EventStore>) -> Self
    
    pub async fn execute<C: Command>(
        &self,
        command: &C,
    ) -> CommandResult<ExecutionResult>
    
    pub async fn execute_with_context<C: Command>(
        &self,
        command: &C,
        context: ExecutionContext,
    ) -> CommandResult<ExecutionResult>
    
    pub async fn execute_once<C: Command>(
        &self,
        command: &C,
    ) -> CommandResult<ExecutionResult>
}

// Builder pattern
impl CommandExecutor {
    pub fn builder() -> CommandExecutorBuilder
}

pub struct CommandExecutorBuilder {
    // Configuration methods
    pub fn with_event_store(self, store: Arc<dyn EventStore>) -> Self
    pub fn with_retry_config(self, config: RetryConfig) -> Self
    pub fn with_timeout(self, timeout: Duration) -> Self
    pub fn build(self) -> CommandExecutor
}
```

**Example:**
```rust
let executor = CommandExecutor::builder()
    .with_event_store(store)
    .with_retry_config(RetryConfig::default())
    .with_timeout(Duration::from_secs(30))
    .build();

let command = TransferMoney { /* ... */ };
let result = executor.execute(&command).await?;
```

### ExecutionResult

Result of command execution.

```rust
pub struct ExecutionResult {
    pub events_written: Vec<StoredEvent>,
    pub affected_streams: Vec<StreamId>,
    pub execution_time: Duration,
    pub correlation_id: CorrelationId,
}
```

### ReadStreams

Type-safe access to stream data during command execution.

```rust
pub struct ReadStreams<S> {
    // Private fields
}

impl<S> ReadStreams<S> {
    pub fn get(&self, stream_id: &StreamId) -> Option<&StreamData>
    pub fn iter(&self) -> impl Iterator<Item = &StreamData>
    pub fn len(&self) -> usize
    pub fn is_empty(&self) -> bool
}
```

### StreamWrite

Type-safe event writing with stream validation.

```rust
pub struct StreamWrite<S, E> {
    // Private fields
}

impl<S, E> StreamWrite<S, E> {
    pub fn new(
        read_streams: &ReadStreams<S>,
        stream_id: StreamId,
        event: E,
    ) -> Result<Self, StreamAccessError>
    
    pub fn stream_id(&self) -> &StreamId
    pub fn event(&self) -> &E
}
```

## Event Store

### EventStore Trait

Core abstraction for event persistence.

```rust
#[async_trait]
pub trait EventStore: Send + Sync {
    type Event: Send + Sync;
    type Error: Send + Sync;
    
    async fn write_events(
        &self,
        events: Vec<EventToWrite<Self::Event>>,
    ) -> Result<WriteResult, Self::Error>;
    
    async fn read_stream(
        &self,
        stream_id: &StreamId,
        options: ReadOptions,
    ) -> Result<StreamEvents<Self::Event>, Self::Error>;
    
    async fn read_multiple_streams(
        &self,
        stream_ids: Vec<StreamId>,
        options: ReadOptions,
    ) -> Result<Vec<StreamEvents<Self::Event>>, Self::Error>;
    
    async fn list_streams(&self) -> Result<Vec<StreamId>, Self::Error>;
    
    async fn stream_exists(&self, stream_id: &StreamId) -> Result<bool, Self::Error>;
    
    async fn get_stream_version(&self, stream_id: &StreamId) -> Result<EventVersion, Self::Error>;
    
    async fn health_check(&self) -> Result<(), Self::Error>;
}
```

### EventToWrite

Event data structure for writing to the store.

```rust
pub struct EventToWrite<E> {
    pub stream_id: StreamId,
    pub expected_version: ExpectedVersion,
    pub event_type: String,
    pub payload: E,
    pub metadata: EventMetadata,
}

impl<E> EventToWrite<E> {
    pub fn new(
        stream_id: StreamId,
        event_type: String,
        payload: E,
    ) -> Self
    
    pub fn with_expected_version(mut self, version: ExpectedVersion) -> Self
    pub fn with_metadata(mut self, metadata: EventMetadata) -> Self
}
```

### StoredEvent

Event data structure as stored in the event store.

```rust
pub struct StoredEvent<E = serde_json::Value> {
    pub id: EventId,
    pub stream_id: StreamId,
    pub version: EventVersion,
    pub event_type: String,
    pub payload: E,
    pub metadata: EventMetadata,
    pub occurred_at: DateTime<Utc>,
}
```

### ReadOptions

Configuration for reading events from streams.

```rust
pub struct ReadOptions {
    pub from_version: Option<EventVersion>,
    pub to_version: Option<EventVersion>,
    pub limit: Option<usize>,
    pub direction: ReadDirection,
}

impl ReadOptions {
    pub fn default() -> Self
    pub fn from_version(mut self, version: EventVersion) -> Self
    pub fn to_version(mut self, version: EventVersion) -> Self
    pub fn limit(mut self, limit: usize) -> Self
    pub fn backwards(mut self) -> Self
}

pub enum ReadDirection {
    Forward,
    Backward,
}
```

### ExpectedVersion

Version expectations for optimistic concurrency control.

```rust
pub enum ExpectedVersion {
    Any,
    NoStream,
    EmptyStream,
    Exact(EventVersion),
}
```

### EventMetadata

Metadata associated with events.

```rust
pub struct EventMetadata {
    pub correlation_id: CorrelationId,
    pub causation_id: Option<CausationId>,
    pub user_id: Option<UserId>,
    pub custom_fields: HashMap<String, serde_json::Value>,
}

impl EventMetadata {
    pub fn new() -> Self
    pub fn with_correlation_id(mut self, id: CorrelationId) -> Self
    pub fn caused_by(mut self, event_id: EventId) -> Self
    pub fn by_user(mut self, user_id: UserId) -> Self
    pub fn with_custom_field(mut self, key: String, value: serde_json::Value) -> Self
}
```

## Event Store Implementations

### InMemoryEventStore

In-memory implementation for testing.

```rust
pub struct InMemoryEventStore {
    // Private fields
}

impl InMemoryEventStore {
    pub fn new() -> Self
    pub fn clear(&mut self)
    pub fn event_count(&self) -> usize
    pub fn stream_count(&self) -> usize
}
```

### PostgresEventStore

PostgreSQL implementation for production.

```rust
pub struct PostgresEventStore {
    // Private fields
}

impl PostgresEventStore {
    pub async fn new(database_url: &str) -> Result<Self, PostgresError>
    pub async fn with_pool(pool: PgPool) -> Self
    pub async fn migrate(&self) -> Result<(), PostgresError>
    pub async fn health_check(&self) -> Result<HealthStatus, PostgresError>
}
```

## Projections

### Projection Trait

Core abstraction for building read models.

```rust
#[async_trait]
pub trait Projection: Send + Sync {
    type Event: Send + Sync;
    type Error: Send + Sync;
    
    async fn apply(&mut self, event: &StoredEvent<Self::Event>) -> Result<(), Self::Error>;
    
    async fn reset(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
```

### ProjectionManager

Manages projection lifecycle and checkpoints.

```rust
pub struct ProjectionManager {
    // Private fields
}

impl ProjectionManager {
    pub fn new() -> Self
    
    pub async fn register<P>(&mut self, name: String, projection: P) -> Result<(), ProjectionError>
    where P: Projection + 'static;
    
    pub async fn start_projection(&self, name: &str) -> Result<(), ProjectionError>
    pub async fn stop_projection(&self, name: &str) -> Result<(), ProjectionError>
    pub async fn reset_projection(&self, name: &str) -> Result<(), ProjectionError>
    
    pub async fn get_checkpoint(&self, name: &str) -> Result<ProjectionCheckpoint, ProjectionError>
    pub async fn save_checkpoint(&self, name: &str, checkpoint: ProjectionCheckpoint) -> Result<(), ProjectionError>
    
    pub async fn list_projections(&self) -> Vec<String>
    pub async fn get_status(&self, name: &str) -> Option<ProjectionStatus>
}
```

### ProjectionCheckpoint

Tracks projection progress.

```rust
pub struct ProjectionCheckpoint {
    pub projection_name: String,
    pub last_event_id: Option<EventId>,
    pub last_event_version: Option<EventVersion>,
    pub stream_positions: HashMap<StreamId, EventVersion>,
    pub events_processed: u64,
    pub last_processed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}
```

### ProjectionStatus

Current status of a projection.

```rust
pub enum ProjectionStatus {
    Stopped,
    Starting,
    Running,
    Paused,
    Error { message: String },
}
```

## CQRS Support

### CqrsProjection

Enhanced projection with query capabilities.

```rust
#[async_trait]
pub trait CqrsProjection: Projection {
    type ReadModel: Send + Sync;
    type Query: Send + Sync;
    type QueryResult: Send + Sync;
    
    async fn query(&self, query: Self::Query) -> Result<Self::QueryResult, Self::Error>;
    async fn get_read_model(&self) -> Result<Self::ReadModel, Self::Error>;
}
```

### ReadModelStore

Storage abstraction for read models.

```rust
#[async_trait]
pub trait ReadModelStore<T>: Send + Sync {
    type Error: Send + Sync;
    
    async fn save(&self, id: &str, model: &T) -> Result<(), Self::Error>;
    async fn load(&self, id: &str) -> Result<Option<T>, Self::Error>;
    async fn delete(&self, id: &str) -> Result<(), Self::Error>;
    async fn list(&self, offset: usize, limit: usize) -> Result<Vec<T>, Self::Error>;
}
```

## Monitoring and Observability

### MetricsCollector

Collects application metrics.

```rust
pub struct MetricsCollector {
    // Private fields
}

impl MetricsCollector {
    pub fn new() -> Self
    pub fn record_command_executed(&self, command_type: &str, duration: Duration, success: bool)
    pub fn record_events_written(&self, stream_id: &str, count: usize)
    pub fn record_projection_event(&self, projection_name: &str, lag_seconds: f64)
    pub async fn export_metrics(&self) -> String
}
```

### HealthChecker

Monitors system health.

```rust
pub struct HealthChecker {
    // Private fields
}

impl HealthChecker {
    pub fn new() -> Self
    pub async fn check_event_store(&self) -> HealthStatus
    pub async fn check_projections(&self) -> HealthStatus
    pub async fn overall_health(&self) -> HealthStatus
}

pub enum HealthStatus {
    Healthy,
    Degraded { reason: String },
    Unhealthy { reason: String },
}
```

## Error Types

### CommandError

Errors that can occur during command execution.

```rust
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
    
    #[error("Business rule violation: {0}")]
    BusinessRuleViolation(String),
    
    #[error("Concurrency conflict on streams: {streams:?}")]
    ConcurrencyConflict { streams: Vec<StreamId> },
    
    #[error("Stream not found: {stream_id}")]
    StreamNotFound { stream_id: StreamId },
    
    #[error("Event store error: {0}")]
    EventStoreError(Box<dyn std::error::Error + Send + Sync>),
    
    #[error("Timeout after {duration:?}")]
    Timeout { duration: Duration },
}
```

### EventStoreError

Errors from event store operations.

```rust
#[derive(Debug, thiserror::Error)]
pub enum EventStoreError {
    #[error("Version conflict: expected {expected:?}, got {actual:?}")]
    VersionConflict {
        expected: ExpectedVersion,
        actual: EventVersion,
    },
    
    #[error("Stream not found: {stream_id}")]
    StreamNotFound { stream_id: StreamId },
    
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Database error: {0}")]
    DatabaseError(Box<dyn std::error::Error + Send + Sync>),
}
```

### ProjectionError

Errors from projection operations.

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProjectionError {
    #[error("Projection not found: {name}")]
    NotFound { name: String },
    
    #[error("Projection already exists: {name}")]
    AlreadyExists { name: String },
    
    #[error("Event processing failed: {0}")]
    ProcessingFailed(String),
    
    #[error("Checkpoint save failed: {0}")]
    CheckpointFailed(String),
    
    #[error("Rebuild failed: {0}")]
    RebuildFailed(String),
}
```

## Configuration Types

### RetryConfig

Configuration for command retry behavior.

```rust
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
    pub retry_policy: RetryPolicy,
}

impl RetryConfig {
    pub fn default() -> Self
    pub fn none() -> Self
    pub fn aggressive() -> Self
    pub fn conservative() -> Self
}

pub enum RetryPolicy {
    None,
    ConcurrencyConflictsOnly,
    TransientErrorsOnly,
    All,
}
```

### ExecutionContext

Context for command execution.

```rust
pub struct ExecutionContext {
    pub correlation_id: CorrelationId,
    pub user_id: Option<UserId>,
    pub timeout: Option<Duration>,
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ExecutionContext {
    pub fn new() -> Self
    pub fn with_correlation_id(mut self, id: CorrelationId) -> Self
    pub fn by_user(mut self, user_id: UserId) -> Self
    pub fn with_timeout(mut self, timeout: Duration) -> Self
    pub fn with_metadata(mut self, key: String, value: serde_json::Value) -> Self
}
```

## Utility Functions

### Result Types

Type aliases for common result patterns.

```rust
pub type CommandResult<T> = Result<T, CommandError>;
pub type EventStoreResult<T> = Result<T, EventStoreError>;
pub type ProjectionResult<T> = Result<T, ProjectionError>;
```

### Macros

Convenience macros for common operations.

```rust
// Create stream IDs from string literals
stream_id!("user-12345") // -> StreamId

// Emit events with automatic typing
emit!(UserCreated { user_id, email }) // -> StreamWrite

// Require conditions in commands  
require!(balance >= amount, "Insufficient balance") // -> Result<(), CommandError>
```

## Testing Support

### Test Utilities

Utilities for testing EventCore applications.

```rust
pub mod testing {
    pub struct TestEventStore {
        // Testing-specific event store
    }
    
    pub struct EventBuilder {
        // Builder for test events
    }
    
    pub struct CommandTestHarness {
        // Test harness for commands
    }
    
    // Assertion helpers
    pub fn assert_events_match(expected: &[Event], actual: &[Event])
    pub fn assert_stream_version(store: &TestEventStore, stream_id: &StreamId, version: EventVersion)
    pub fn assert_event_exists<F>(store: &TestEventStore, predicate: F) 
    where F: Fn(&StoredEvent) -> bool;
}
```

This completes the API documentation. All public APIs are documented with their signatures, examples, and key behaviors. Use this as a reference when developing with EventCore.

Next, let's explore [Configuration Reference](./02-configuration-reference.md) →