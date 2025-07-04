# CQRS Design for EventCore

## Overview

This document outlines the design for extending EventCore's projection system to support full Command Query Responsibility Segregation (CQRS) patterns. The goal is to provide production-ready abstractions for building scalable read models while maintaining EventCore's philosophy of type safety and simplicity.

## Current State Analysis

### Existing Components
- **Projection Trait**: Core abstraction for event processing
- **ProjectionRunner**: Manages subscription and event processing lifecycle
- **ProjectionManager**: Orchestrates multiple projections
- **In-Memory State**: Examples only use in-memory storage

### Key Gaps for CQRS
1. No persistent read model storage abstractions
2. No query API or filtering capabilities
3. No snapshot/materialized view support
4. Manual checkpoint persistence
5. Limited rebuild strategies
6. No read model versioning

## Proposed CQRS Architecture

### Core Abstractions

#### 1. ReadModelStore Trait
```rust
#[async_trait]
pub trait ReadModelStore: Send + Sync {
    type Model: Send + Sync;
    type Query: Send + Sync;
    type Error: std::error::Error + Send + Sync;
    
    /// Store or update a read model
    async fn upsert(&self, id: &str, model: Self::Model) -> Result<(), Self::Error>;
    
    /// Retrieve a read model by ID
    async fn get(&self, id: &str) -> Result<Option<Self::Model>, Self::Error>;
    
    /// Query read models
    async fn query(&self, query: Self::Query) -> Result<Vec<Self::Model>, Self::Error>;
    
    /// Delete a read model
    async fn delete(&self, id: &str) -> Result<(), Self::Error>;
    
    /// Bulk operations for efficiency
    async fn bulk_upsert(&self, models: Vec<(String, Self::Model)>) -> Result<(), Self::Error>;
}
```

#### 2. CheckpointStore Trait
```rust
#[async_trait]
pub trait CheckpointStore: Send + Sync {
    type Error: std::error::Error + Send + Sync;
    
    /// Load checkpoint for a projection
    async fn load(&self, projection_name: &str) -> Result<Option<ProjectionCheckpoint>, Self::Error>;
    
    /// Save checkpoint for a projection
    async fn save(&self, projection_name: &str, checkpoint: ProjectionCheckpoint) -> Result<(), Self::Error>;
    
    /// Delete checkpoint (for rebuilds)
    async fn delete(&self, projection_name: &str) -> Result<(), Self::Error>;
}
```

#### 3. CqrsProjection Trait
```rust
#[async_trait]
pub trait CqrsProjection: Projection {
    type ReadModel: Send + Sync;
    type Query: Send + Sync;
    
    /// Extract read model ID from event
    fn extract_model_id(&self, event: &Event<Self::Event>) -> Option<String>;
    
    /// Build/update read model from event
    async fn apply_to_model(
        &self,
        model: Option<Self::ReadModel>,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<Option<Self::ReadModel>>;
    
    /// Handle queries against the read model
    async fn handle_query(
        &self,
        store: &dyn ReadModelStore<Model = Self::ReadModel, Query = Self::Query>,
        query: Self::Query,
    ) -> ProjectionResult<Vec<Self::ReadModel>>;
}
```

### Storage Implementations

#### PostgreSQL Read Model Store
```rust
pub struct PostgresReadModelStore<M> {
    pool: PgPool,
    table_name: String,
    _phantom: PhantomData<M>,
}

impl<M> PostgresReadModelStore<M> {
    pub async fn new(pool: PgPool, table_name: String) -> Self {
        // Auto-create table based on model structure
        Self::ensure_table_exists(&pool, &table_name).await;
        Self { pool, table_name, _phantom: PhantomData }
    }
}
```

#### In-Memory Read Model Store (for testing)
```rust
pub struct InMemoryReadModelStore<M, Q> {
    models: Arc<RwLock<HashMap<String, M>>>,
    _phantom: PhantomData<Q>,
}
```

### Projection Rebuild Strategy

#### RebuildCoordinator
```rust
pub struct RebuildCoordinator<P, E>
where
    P: CqrsProjection<Event = E>,
{
    projection: Arc<P>,
    event_store: Arc<dyn EventStore<Event = E>>,
    read_model_store: Arc<dyn ReadModelStore<Model = P::ReadModel>>,
    checkpoint_store: Arc<dyn CheckpointStore>,
}

impl<P, E> RebuildCoordinator<P, E> {
    pub async fn rebuild_from_beginning(&self) -> ProjectionResult<()> {
        // 1. Mark projection as rebuilding
        // 2. Clear existing read models (or version them)
        // 3. Reset checkpoint
        // 4. Stream all events from beginning
        // 5. Apply in batches with progress tracking
    }
    
    pub async fn rebuild_from_checkpoint(&self, checkpoint: ProjectionCheckpoint) -> ProjectionResult<()> {
        // Similar but starting from checkpoint
    }
}
```

### Read Model Versioning

Support for maintaining multiple versions during migration:

```rust
pub struct VersionedReadModel<M> {
    pub version: u32,
    pub model: M,
}

pub trait VersionedReadModelStore: ReadModelStore {
    async fn migrate_version(
        &self,
        from_version: u32,
        to_version: u32,
        migration_fn: Box<dyn Fn(Self::Model) -> Self::Model>,
    ) -> Result<(), Self::Error>;
}
```

### Query API Design

#### Type-Safe Query Builder
```rust
#[derive(Debug, Clone)]
pub struct QueryBuilder<M> {
    filters: Vec<Filter>,
    ordering: Option<Ordering>,
    limit: Option<usize>,
    offset: Option<usize>,
    _phantom: PhantomData<M>,
}

impl<M> QueryBuilder<M> {
    pub fn new() -> Self { ... }
    
    pub fn filter(mut self, field: &str, op: FilterOp, value: Value) -> Self { ... }
    
    pub fn order_by(mut self, field: &str, direction: Direction) -> Self { ... }
    
    pub fn limit(mut self, limit: usize) -> Self { ... }
    
    pub fn offset(mut self, offset: usize) -> Self { ... }
}
```

### Eventual Consistency Patterns

#### 1. Read Model Lag Monitoring
```rust
pub struct ReadModelLagMonitor {
    /// Track latest event ID in write model vs read model
    write_model_position: Arc<AtomicU64>,
    read_model_positions: Arc<RwLock<HashMap<String, u64>>>,
}
```

#### 2. Consistency Boundaries
```rust
pub enum ConsistencyLevel {
    /// Read from latest snapshot (fastest, may be stale)
    Eventual,
    /// Wait for specific event to be processed
    CausallyConsistent { after_event: EventId },
    /// Force synchronous update before read
    Strong,
}
```

### Integration with Existing System

#### Enhanced ProjectionManager
```rust
impl ProjectionManager {
    pub async fn register_cqrs_projection<P>(
        &self,
        projection: P,
        read_model_store: Arc<dyn ReadModelStore<Model = P::ReadModel>>,
        checkpoint_store: Arc<dyn CheckpointStore>,
    ) -> ProjectionResult<()>
    where
        P: CqrsProjection + 'static,
    {
        // Wrap in CqrsProjectionRunner
        // Register with health monitoring
        // Setup rebuild coordinator
    }
}
```

## Implementation Phases

### Phase 1: Core Abstractions
1. Define ReadModelStore trait
2. Define CheckpointStore trait
3. Define CqrsProjection trait
4. Create type-safe query builder

### Phase 2: Storage Implementations
1. PostgreSQL read model store
2. PostgreSQL checkpoint store
3. In-memory implementations for testing
4. Redis cache layer (optional)

### Phase 3: Projection Enhancement
1. CqrsProjectionRunner with storage integration
2. Batch processing optimizations
3. Error recovery with read model rollback
4. Progress tracking for long-running operations

### Phase 4: Rebuild & Migration
1. RebuildCoordinator implementation
2. Versioned read model support
3. Zero-downtime migration strategies
4. Parallel rebuild capabilities

### Phase 5: Monitoring & Operations
1. Read model lag monitoring
2. Health checks for projections
3. Metrics collection (events/sec, lag, errors)
4. Admin API for projection management

## Example Usage

```rust
// Define a read model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSummary {
    pub account_id: AccountId,
    pub balance: Money,
    pub transaction_count: u64,
    pub last_activity: Timestamp,
}

// Define a CQRS projection
pub struct AccountSummaryProjection;

#[async_trait]
impl CqrsProjection for AccountSummaryProjection {
    type ReadModel = AccountSummary;
    type Query = AccountQuery;
    
    fn extract_model_id(&self, event: &Event<BankingEvent>) -> Option<String> {
        match &event.payload {
            BankingEvent::AccountOpened(e) => Some(e.account_id.to_string()),
            BankingEvent::MoneyDeposited(e) => Some(e.account_id.to_string()),
            // ... other events
        }
    }
    
    async fn apply_to_model(
        &self,
        model: Option<Self::ReadModel>,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<Option<Self::ReadModel>> {
        let mut summary = model.unwrap_or_else(|| AccountSummary::default());
        
        match &event.payload {
            BankingEvent::MoneyDeposited(e) => {
                summary.balance = summary.balance.add(&e.amount)?;
                summary.transaction_count += 1;
                summary.last_activity = event.created_at;
            }
            // ... handle other events
        }
        
        Ok(Some(summary))
    }
}

// Usage
let projection = AccountSummaryProjection::new();
let read_store = PostgresReadModelStore::new(pool, "account_summaries").await;
let checkpoint_store = PostgresCheckpointStore::new(pool).await;

projection_manager
    .register_cqrs_projection(projection, read_store, checkpoint_store)
    .await?;

// Query read models
let query = AccountQuery::by_balance_range(Money::from(1000), Money::from(10000));
let accounts = projection.handle_query(&read_store, query).await?;
```

## Testing Strategy

1. **Unit Tests**: Test each trait implementation in isolation
2. **Integration Tests**: Test full CQRS flow with in-memory stores
3. **PostgreSQL Tests**: Test with real database
4. **Performance Tests**: Benchmark read model updates and queries
5. **Chaos Tests**: Test rebuild under load, concurrent updates

## Migration Path

For existing EventCore users:
1. Existing projections continue to work unchanged
2. New CqrsProjection trait extends existing Projection trait
3. Gradual migration by implementing storage traits
4. Backward compatible with existing projection runner

## Performance Considerations

1. **Batch Processing**: Update multiple read models in single transaction
2. **Async I/O**: Non-blocking storage operations
3. **Connection Pooling**: Reuse database connections
4. **Caching**: Optional Redis layer for hot read models
5. **Partitioning**: Support for sharding read models

## Security Considerations

1. **Read Model Isolation**: Separate permissions for read/write
2. **Query Validation**: Prevent injection attacks
3. **Rate Limiting**: Protect against expensive queries
4. **Audit Trail**: Track who queries what data

## Future Enhancements

1. **GraphQL Integration**: Auto-generate GraphQL schema from read models
2. **Real-time Subscriptions**: WebSocket/SSE for live updates
3. **Federated Queries**: Query across multiple read model stores
4. **Machine Learning**: Predictive read models based on event patterns