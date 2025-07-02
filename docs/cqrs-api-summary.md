# CQRS API Summary

This document provides a quick reference for the CQRS APIs in EventCore, with links to detailed documentation.

## Core Types

### RebuildCoordinator

The main entry point for projection rebuilds.

```rust
use eventcore::cqrs::RebuildCoordinator;

let coordinator = RebuildCoordinator::new(
    projection,      // impl CqrsProjection
    event_store,     // Arc<dyn EventStore>
    read_model_store,// Arc<dyn ReadModelStore>
    checkpoint_store,// Arc<dyn CheckpointStore>
);
```

**Key Methods:**
- `rebuild_from_beginning()` - Complete rebuild from scratch
- `rebuild_from_checkpoint(checkpoint)` - Resume from saved position
- `rebuild(strategy)` - Advanced rebuild with custom strategy
- `get_progress()` - Monitor current progress
- `cancel()` - Stop rebuild gracefully

[Full API documentation](../eventcore/src/cqrs/rebuild.rs)

### RebuildStrategy

Defines how a rebuild should be performed.

```rust
pub enum RebuildStrategy {
    FromBeginning,
    FromCheckpoint(ProjectionCheckpoint),
    FromEvent(EventId),
    SpecificStreams(StreamIds), // Planned
}
```

### RebuildProgress

Real-time metrics for rebuild operations.

```rust
pub struct RebuildProgress {
    pub total_events: Option<u64>,
    pub events_processed: u64,
    pub models_updated: u64,
    pub started_at: Instant,
    pub estimated_completion: Option<Instant>,
    pub events_per_second: f64,
    pub is_running: bool,
    pub error: Option<String>,
}
```

**Key Methods:**
- `completion_percentage()` - Get progress as percentage
- `elapsed()` - Time since rebuild started

## Storage Traits

### ReadModelStore

Storage abstraction for read models.

```rust
#[async_trait]
pub trait ReadModelStore: Send + Sync {
    type Model: Send + Sync;
    type Query: Send + Sync;
    type Error: std::error::Error + Send + Sync;
    
    async fn upsert(&self, id: &str, model: Self::Model) -> Result<(), Self::Error>;
    async fn get(&self, id: &str) -> Result<Option<Self::Model>, Self::Error>;
    async fn query(&self, query: Self::Query) -> Result<Vec<Self::Model>, Self::Error>;
    async fn delete(&self, id: &str) -> Result<(), Self::Error>;
    async fn clear(&self) -> Result<(), Self::Error>;
}
```

**Implementations:**
- `InMemoryReadModelStore` - For testing
- `PostgresReadModelStore` - Production storage (implement yourself)

### CheckpointStore

Manages projection checkpoints for resumability.

```rust
#[async_trait]
pub trait CheckpointStore: Send + Sync {
    type Error: std::error::Error + Send + Sync;
    
    async fn load(&self, name: &str) -> Result<Option<ProjectionCheckpoint>, Self::Error>;
    async fn save(&self, name: &str, checkpoint: ProjectionCheckpoint) -> Result<(), Self::Error>;
    async fn delete(&self, name: &str) -> Result<(), Self::Error>;
}
```

**Implementations:**
- `InMemoryCheckpointStore` - For testing
- `PostgresCheckpointStore` - Production storage (implement yourself)

## Projection Traits

### CqrsProjection

Extended projection trait for CQRS support.

```rust
#[async_trait]
pub trait CqrsProjection: Projection {
    type ReadModel: Send + Sync;
    type Query: Send + Sync;
    
    fn extract_model_id(&self, event: &Event<Self::Event>) -> Option<String>;
    
    async fn apply_to_model(
        &self,
        model: Option<Self::ReadModel>,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<Option<Self::ReadModel>>;
}
```

## Usage Examples

### Basic Rebuild

```rust
// Complete rebuild
let progress = coordinator.rebuild_from_beginning().await?;

// Incremental rebuild
let checkpoint = checkpoint_store.load("my-projection").await?
    .unwrap_or_else(|| ProjectionCheckpoint::initial());
let progress = coordinator.rebuild_from_checkpoint(checkpoint).await?;
```

### Progress Monitoring

```rust
// Real-time monitoring
let coordinator = Arc::new(coordinator);
let monitor = coordinator.clone();

tokio::spawn(async move {
    loop {
        let progress = monitor.get_progress().await;
        if !progress.is_running { break; }
        println!("Progress: {} events", progress.events_processed);
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
});

let result = coordinator.rebuild_from_beginning().await?;
```

### Cancellation

```rust
// Cancel on timeout
let rebuild_future = coordinator.rebuild_from_beginning();
match timeout(Duration::from_secs(300), rebuild_future).await {
    Ok(result) => result?,
    Err(_) => {
        coordinator.cancel();
        return Err("Timeout")?;
    }
}
```

## Error Handling

```rust
pub enum CqrsError {
    Rebuild(String),              // Rebuild-specific errors
    Storage(Box<dyn Error>),      // Storage layer errors
    Projection(ProjectionError),  // Projection logic errors
    Checkpoint(String),           // Checkpoint errors
}
```

## Complete Example

See [cqrs_rebuild_example.rs](../eventcore/examples/cqrs_rebuild_example.rs) for a comprehensive example demonstrating:
- Setting up CQRS projections
- Different rebuild strategies
- Progress monitoring
- Error handling
- Querying rebuilt data

## Related Documentation

- [CQRS Rebuild Reference](cqrs-rebuild-reference.md) - Comprehensive guide
- [Projection Rebuild Tutorial](tutorials/projection-rebuild.md) - Step-by-step tutorial
- [Implementing Projections](tutorials/implementing-projections.md) - Projection basics
- [CQRS Design](cqrs-design.md) - Architecture and design decisions