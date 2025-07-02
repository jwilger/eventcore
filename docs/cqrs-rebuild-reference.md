# CQRS Rebuild Reference

This document provides a comprehensive reference for the CQRS projection rebuild functionality in EventCore. For a step-by-step tutorial, see [projection-rebuild.md](tutorials/projection-rebuild.md).

## Overview

The EventCore CQRS module provides powerful projection rebuild capabilities that enable:

- **Complete Rebuilds**: Reprocess all events from the beginning
- **Incremental Rebuilds**: Resume from a checkpoint or specific event
- **Selective Rebuilds**: Target specific streams (planned feature)
- **Progress Monitoring**: Real-time tracking of rebuild operations
- **Cancellation Support**: Gracefully stop long-running rebuilds
- **Fault Tolerance**: Automatic checkpointing and error recovery

## Core Components

### RebuildCoordinator

The `RebuildCoordinator` is the primary interface for managing projection rebuilds. It orchestrates the entire rebuild process, including subscription management, progress tracking, and error handling.

```rust
use eventcore::cqrs::{RebuildCoordinator, CqrsProjection};
use std::sync::Arc;

let coordinator = RebuildCoordinator::new(
    projection,
    event_store,
    read_model_store,
    checkpoint_store,
);
```

#### Key Responsibilities

- **Subscription Management**: Creates and manages event subscriptions for rebuild
- **State Coordination**: Manages projection state during rebuild
- **Progress Tracking**: Updates progress metrics in real-time
- **Checkpoint Management**: Saves progress periodically for resumability
- **Error Recovery**: Handles failures and supports retry strategies

### RebuildStrategy

Defines how the rebuild should be performed:

```rust
pub enum RebuildStrategy {
    /// Clear everything and rebuild from the beginning
    FromBeginning,
    
    /// Resume from a saved checkpoint
    FromCheckpoint(ProjectionCheckpoint),
    
    /// Start from a specific event ID
    FromEvent(EventId),
    
    /// Rebuild only specific streams (planned)
    SpecificStreams(StreamIds),
}
```

#### Strategy Selection Guide

| Strategy | Use When | Behavior |
|----------|----------|----------|
| `FromBeginning` | - Deploying new projection<br>- Major bug fixes<br>- Schema changes | Clears all data, processes all events |
| `FromCheckpoint` | - Resuming interrupted rebuild<br>- Incremental updates<br>- Minor fixes | Preserves existing data, processes new events |
| `FromEvent` | - Known issue start point<br>- Testing scenarios<br>- Custom recovery | Processes events after specific ID |
| `SpecificStreams` | - Targeted fixes<br>- Stream-specific issues | Only rebuilds affected streams |

### RebuildProgress

Provides detailed metrics about the rebuild operation:

```rust
pub struct RebuildProgress {
    /// Total events to process (if known)
    pub total_events: Option<u64>,
    
    /// Events processed so far
    pub events_processed: u64,
    
    /// Read models updated
    pub models_updated: u64,
    
    /// Start time
    pub started_at: Instant,
    
    /// Estimated completion
    pub estimated_completion: Option<Instant>,
    
    /// Processing rate
    pub events_per_second: f64,
    
    /// Running status
    pub is_running: bool,
    
    /// Error details
    pub error: Option<String>,
}
```

#### Progress Metrics

- **Completion Percentage**: Available when total event count is known
- **Processing Rate**: Events per second, updated in real-time
- **Time Estimates**: ETA based on current processing rate
- **Error Tracking**: Captures failure details for diagnostics

## API Reference

### Starting a Rebuild

#### rebuild_from_beginning()

Performs a complete rebuild, clearing all existing data:

```rust
let progress = coordinator.rebuild_from_beginning().await?;
```

**Behavior**:
1. Clears all read models
2. Deletes existing checkpoints
3. Subscribes from the beginning
4. Processes all events in order
5. Returns final progress statistics

**Use Cases**:
- Initial projection deployment
- Major bug fixes requiring complete reprocessing
- Schema migrations affecting all data

#### rebuild_from_checkpoint()

Resumes from a saved checkpoint:

```rust
let checkpoint = checkpoint_store.load("projection-name").await?
    .unwrap_or_else(|| ProjectionCheckpoint::initial());
    
let progress = coordinator.rebuild_from_checkpoint(checkpoint).await?;
```

**Behavior**:
1. Validates checkpoint has event ID
2. Subscribes from checkpoint position
3. Processes only new events
4. Updates affected read models
5. Preserves unaffected data

**Use Cases**:
- Resuming after interruption
- Regular incremental updates
- Fixing recent data issues

#### rebuild()

Advanced method accepting any rebuild strategy:

```rust
let strategy = RebuildStrategy::FromEvent(specific_event_id);
let progress = coordinator.rebuild(strategy).await?;
```

**Behavior**: Varies based on strategy (see RebuildStrategy section)

### Monitoring Progress

#### get_progress()

Returns current rebuild progress snapshot:

```rust
let progress = coordinator.get_progress().await;

if let Some(pct) = progress.completion_percentage() {
    println!("Progress: {:.1}%", pct);
}
println!("Rate: {:.0} events/sec", progress.events_per_second);
```

**Thread Safety**: Safe to call from any thread or async task

#### Progress Monitoring Pattern

```rust
use tokio::time::interval;
use std::time::Duration;

// Start rebuild in background
let coordinator = Arc::new(coordinator);
let rebuild_handle = {
    let coordinator = coordinator.clone();
    tokio::spawn(async move {
        coordinator.rebuild_from_beginning().await
    })
};

// Monitor progress
let mut ticker = interval(Duration::from_secs(1));
loop {
    ticker.tick().await;
    
    let progress = coordinator.get_progress().await;
    if !progress.is_running {
        break;
    }
    
    // Display progress metrics
    display_progress(&progress);
}

// Get final result
let final_progress = rebuild_handle.await??;
```

### Cancellation

#### cancel()

Signals the rebuild to stop gracefully:

```rust
coordinator.cancel();
```

**Behavior**:
1. Sets cancellation flag
2. Current event processing completes
3. Checkpoint saved at current position
4. Returns with cancellation error
5. Can resume from checkpoint later

**Cancellation Patterns**:

```rust
// Timeout-based cancellation
use tokio::time::timeout;

match timeout(Duration::from_secs(300), rebuild_future).await {
    Ok(result) => result?,
    Err(_) => {
        coordinator.cancel();
        return Err("Rebuild timeout")?;
    }
}

// Signal-based cancellation
use tokio::signal;

tokio::select! {
    result = rebuild_future => handle_result(result),
    _ = signal::ctrl_c() => {
        coordinator.cancel();
        println!("Rebuild cancelled by user");
    }
}
```

## Integration with EventCore

### CqrsProjection Trait

Projections must implement the `CqrsProjection` trait to support rebuilds:

```rust
#[async_trait]
pub trait CqrsProjection: Projection {
    type ReadModel: Send + Sync;
    type Query: Send + Sync;
    
    /// Extract model ID from event
    fn extract_model_id(&self, event: &Event<Self::Event>) -> Option<String>;
    
    /// Apply event to read model
    async fn apply_to_model(
        &self,
        model: Option<Self::ReadModel>,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<Option<Self::ReadModel>>;
}
```

### Storage Traits

Rebuild functionality requires implementations of:

#### ReadModelStore

```rust
#[async_trait]
pub trait ReadModelStore: Send + Sync {
    type Model: Send + Sync;
    type Query: Send + Sync;
    type Error: std::error::Error + Send + Sync;
    
    async fn upsert(&self, id: &str, model: Self::Model) -> Result<(), Self::Error>;
    async fn get(&self, id: &str) -> Result<Option<Self::Model>, Self::Error>;
    async fn delete(&self, id: &str) -> Result<(), Self::Error>;
    async fn clear(&self) -> Result<(), Self::Error>;
}
```

#### CheckpointStore

```rust
#[async_trait]
pub trait CheckpointStore: Send + Sync {
    type Error: std::error::Error + Send + Sync;
    
    async fn load(&self, name: &str) -> Result<Option<ProjectionCheckpoint>, Self::Error>;
    async fn save(&self, name: &str, checkpoint: ProjectionCheckpoint) -> Result<(), Self::Error>;
    async fn delete(&self, name: &str) -> Result<(), Self::Error>;
}
```

## Performance Optimization

### Checkpoint Frequency

The rebuild system automatically saves checkpoints every 100 events by default. This provides a good balance between:

- **Recovery granularity**: Not losing too much progress on failure
- **Performance overhead**: Minimizing checkpoint write operations

Adjust based on your needs:
- **Large events**: Increase frequency (e.g., every 50 events)
- **Small events**: Decrease frequency (e.g., every 500 events)
- **Critical data**: More frequent checkpoints for safety

### Batch Processing

Read model updates are processed individually by default. For better performance with large rebuilds:

1. **Batch Updates**: Accumulate changes and write in batches
2. **Bulk Operations**: Use `bulk_upsert` when available
3. **Transaction Batching**: Group updates in database transactions

### Parallel Processing

While individual projections process events sequentially, you can:

1. **Run Multiple Projections**: Different projections can rebuild in parallel
2. **Partition by Stream**: Process independent streams concurrently
3. **Use Multiple Workers**: Distribute work across multiple rebuild coordinators

## Error Handling

### Error Types

The rebuild system can encounter several error types:

```rust
pub enum CqrsError {
    /// Rebuild-specific errors
    Rebuild(String),
    
    /// Storage errors
    Storage(Box<dyn std::error::Error>),
    
    /// Projection errors
    Projection(ProjectionError),
    
    /// Checkpoint errors
    Checkpoint(String),
}
```

### Recovery Strategies

#### Automatic Retry

Implement retry logic for transient failures:

```rust
async fn rebuild_with_retry(
    coordinator: &RebuildCoordinator<impl CqrsProjection, impl Send + Sync>,
    max_attempts: u32,
) -> Result<RebuildProgress, CqrsError> {
    let mut attempts = 0;
    
    loop {
        match coordinator.rebuild_from_beginning().await {
            Ok(progress) => return Ok(progress),
            Err(e) if attempts < max_attempts => {
                attempts += 1;
                let delay = Duration::from_secs(2_u64.pow(attempts));
                tokio::time::sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}
```

#### Partial Recovery

For non-critical errors, continue processing:

```rust
// In your projection's apply_to_model implementation
match process_event(event) {
    Ok(model) => Ok(Some(model)),
    Err(e) if is_recoverable(&e) => {
        log::warn!("Skipping event due to error: {}", e);
        Ok(existing_model) // Keep existing model
    }
    Err(e) => Err(e.into()),
}
```

## Production Deployment

### Zero-Downtime Rebuilds

Deploy projection updates without service interruption:

1. **Version Projections**: Add version suffix to projection names
2. **Deploy New Version**: Run new projection alongside old
3. **Rebuild in Background**: Process events without affecting reads
4. **Validate Results**: Ensure new projection is healthy
5. **Switch Traffic**: Update read queries to use new version
6. **Cleanup**: Remove old projection after verification

### Monitoring Integration

Track rebuild operations in production:

```rust
// Metrics to monitor
metrics.rebuild_duration.observe(duration);
metrics.events_processed.inc_by(count);
metrics.rebuild_failures.inc();
metrics.current_progress.set(percentage);

// Alerts to configure
- Rebuild duration > threshold
- Rebuild failure rate > threshold
- Progress stalled > duration
- Memory usage during rebuild
```

### Resource Management

Rebuilds can be resource-intensive. Consider:

1. **Memory Limits**: Monitor and limit memory usage
2. **CPU Throttling**: Implement rate limiting if needed
3. **I/O Scheduling**: Use lower priority for background rebuilds
4. **Connection Pooling**: Ensure adequate database connections

## Best Practices

### When to Rebuild

✅ **Good Reasons**:
- New projection deployment
- Bug fixes in projection logic
- Schema migrations
- Data corruption recovery
- Adding new derived fields

❌ **Avoid Rebuilding For**:
- Minor display changes
- Adding simple queries (use existing data)
- Temporary analysis (use read-time computation)

### Testing Rebuilds

Always test rebuild logic thoroughly:

```rust
#[tokio::test]
async fn test_rebuild_produces_correct_state() {
    // Generate test events
    let events = generate_test_events();
    
    // Build state via normal processing
    let expected_state = process_events_normally(&events).await;
    
    // Rebuild from scratch
    let coordinator = create_test_coordinator();
    let progress = coordinator.rebuild_from_beginning().await.unwrap();
    
    // Compare results
    let rebuilt_state = read_all_models().await;
    assert_eq!(rebuilt_state, expected_state);
    assert_eq!(progress.events_processed, events.len());
}
```

### Documentation

Document your projections' rebuild behavior:

```rust
/// Order summary projection
/// 
/// Rebuild behavior:
/// - Safe to rebuild anytime (idempotent)
/// - Typical rebuild time: ~5 min for 1M events
/// - Memory usage: ~500MB during rebuild
/// - Dependencies: Product catalog must be current
impl CqrsProjection for OrderSummaryProjection {
    // ... implementation
}
```

## Troubleshooting

### Common Issues

1. **Slow Rebuilds**
   - Check database indexes
   - Verify network latency
   - Monitor checkpoint frequency
   - Consider batch processing

2. **Memory Issues**
   - Implement streaming for large datasets
   - Clear intermediate state periodically
   - Use pagination for queries
   - Monitor heap usage

3. **Checkpoint Failures**
   - Verify checkpoint store permissions
   - Check storage capacity
   - Monitor checkpoint size
   - Implement checkpoint cleanup

4. **Inconsistent Results**
   - Ensure event ordering is preserved
   - Verify idempotent event handling
   - Check for race conditions
   - Validate transaction boundaries

### Debug Logging

Enable detailed logging for troubleshooting:

```rust
// Set environment variables
RUST_LOG=eventcore::cqrs::rebuild=debug
RUST_LOG_SPAN_EVENTS=full

// Key log points:
- Rebuild strategy selection
- Subscription creation
- Event processing progress
- Checkpoint saves
- Error details
```

## Future Enhancements

### Planned Features

1. **Specific Stream Rebuilds**: Target individual streams for surgical fixes
2. **Parallel Rebuild**: Process independent streams concurrently
3. **Incremental Snapshots**: Speed up rebuilds with periodic snapshots
4. **Live Migration**: Seamless projection updates with zero downtime
5. **Rebuild Orchestration**: Coordinate multiple projection rebuilds

### Extension Points

The rebuild system is designed for extensibility:

- **Custom Strategies**: Implement new `RebuildStrategy` variants
- **Progress Reporters**: Add custom progress tracking
- **Storage Adapters**: Integrate with different storage systems
- **Monitoring Hooks**: Add custom metrics and alerts