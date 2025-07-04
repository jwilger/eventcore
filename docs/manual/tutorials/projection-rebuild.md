# Tutorial: Projection Rebuild and Recovery

This tutorial explains how to rebuild projections in EventCore, a critical capability for production systems. You'll learn when and how to rebuild projections, monitor progress, and handle various rebuild scenarios.

## What is Projection Rebuild?

Projection rebuild is the process of recreating a projection's state by replaying events from the event store. This is necessary when:

- **Initial deployment**: Building a new projection from existing events
- **Bug fixes**: Correcting projection logic requires reprocessing events
- **Schema changes**: Updating the structure of your read models
- **Data corruption**: Recovering from storage failures or bugs
- **Performance optimization**: Rebuilding with improved data structures

## Prerequisites

```toml
[dependencies]
eventcore = "0.1"
tokio = { version = "1.0", features = ["full"] }
async-trait = "0.1"
serde = { version = "1.0", features = ["derive"] }
tracing = "0.1"
```

## Basic Rebuild Example

Let's start with a simple example using the CQRS rebuild functionality:

```rust
use eventcore::{
    cqrs::{
        CqrsProjection, RebuildCoordinator, RebuildStrategy, 
        RebuildProgress, InMemoryReadModelStore, InMemoryCheckpointStore
    },
    Event, EventStore, ProjectionResult,
    projection::{Projection, ProjectionConfig, ProjectionCheckpoint},
};
use std::sync::Arc;

// Example projection that tracks user activity
#[derive(Debug)]
struct UserActivityProjection;

#[async_trait::async_trait]
impl Projection for UserActivityProjection {
    type State = UserActivityState;
    type Event = UserEvent;

    fn config(&self) -> &ProjectionConfig {
        // Implementation details...
    }

    async fn apply_event(
        &self,
        state: &mut Self::State,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<()> {
        // Update state based on event
        Ok(())
    }

    async fn initialize_state(&self) -> ProjectionResult<Self::State> {
        Ok(UserActivityState::default())
    }
}

#[async_trait::async_trait]
impl CqrsProjection for UserActivityProjection {
    type ReadModel = UserActivity;
    type Query = UserQuery;

    fn extract_model_id(&self, event: &Event<Self::Event>) -> Option<String> {
        // Extract user ID from event
        match &event.payload {
            UserEvent::UserRegistered { user_id, .. } => Some(user_id.clone()),
            UserEvent::UserLoggedIn { user_id, .. } => Some(user_id.clone()),
            // ... other events
        }
    }

    async fn apply_to_model(
        &self,
        model: Option<Self::ReadModel>,
        event: &Event<Self::Event>,
    ) -> ProjectionResult<Option<Self::ReadModel>> {
        // Update or create read model
        let mut activity = model.unwrap_or_else(|| UserActivity::new(/* ... */));
        
        match &event.payload {
            UserEvent::UserLoggedIn { .. } => {
                activity.login_count += 1;
                activity.last_login = Some(event.created_at);
            }
            // ... handle other events
        }
        
        Ok(Some(activity))
    }
}

// Perform a rebuild
async fn rebuild_user_activity_projection(
    event_store: Arc<dyn EventStore<Event = UserEvent>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let projection = UserActivityProjection;
    let read_model_store = Arc::new(InMemoryReadModelStore::new());
    let checkpoint_store = Arc::new(InMemoryCheckpointStore::new());
    
    // Create rebuild coordinator
    let coordinator = RebuildCoordinator::new(
        projection,
        event_store,
        read_model_store.clone(),
        checkpoint_store.clone(),
    );
    
    // Start rebuild from the beginning
    println!("Starting projection rebuild...");
    let progress = coordinator.rebuild_from_beginning().await?;
    
    println!("Rebuild completed!");
    println!("  Events processed: {}", progress.events_processed);
    println!("  Models updated: {}", progress.models_updated);
    println!("  Time elapsed: {:?}", progress.elapsed());
    
    if let Some(percentage) = progress.completion_percentage() {
        println!("  Completion: {:.2}%", percentage);
    }
    
    Ok(())
}
```

## Rebuild Strategies

EventCore supports multiple rebuild strategies for different scenarios:

### 1. Rebuild From Beginning

Completely rebuilds the projection from the first event:

```rust
// Clear all existing data and start fresh
let progress = coordinator.rebuild_from_beginning().await?;
```

Use when:
- Deploying a new projection
- Fixing fundamental bugs in projection logic
- Changing the projection's data model significantly

### 2. Rebuild From Checkpoint

Resumes rebuilding from a saved position:

```rust
// Load the last known good checkpoint
let checkpoint = checkpoint_store
    .load("user-activity-projection")
    .await?
    .unwrap_or_else(|| ProjectionCheckpoint::initial());

let progress = coordinator
    .rebuild_from_checkpoint(checkpoint)
    .await?;
```

Use when:
- Recovering from a partial failure
- Continuing after fixing a bug that only affects recent events
- Implementing incremental rebuilds

### 3. Rebuild From Specific Event

Starts rebuilding from a particular event ID:

```rust
use eventcore::EventId;

// Start from a known event
let event_id = EventId::try_new("01234567-89ab-cdef-0123-456789abcdef")?;
let progress = coordinator
    .rebuild(RebuildStrategy::FromEvent(event_id))
    .await?;
```

Use when:
- You know exactly when the issue started
- Testing projection behavior from a specific point
- Implementing custom recovery logic

### 4. Rebuild Specific Streams (Future)

Rebuilds only events from specific streams:

```rust
// Note: This is planned functionality
let strategy = RebuildStrategy::SpecificStreams(stream_ids);
let progress = coordinator.rebuild(strategy).await?;
```

## Monitoring Rebuild Progress

The rebuild process provides detailed progress tracking:

```rust
use std::time::Duration;
use tokio::time::interval;

// Start rebuild in background
let coordinator = Arc::new(coordinator);
let coordinator_clone = coordinator.clone();

let rebuild_task = tokio::spawn(async move {
    coordinator_clone.rebuild_from_beginning().await
});

// Monitor progress
let mut progress_interval = interval(Duration::from_secs(1));

loop {
    progress_interval.tick().await;
    
    let progress = coordinator.get_progress().await;
    
    if !progress.is_running {
        break;
    }
    
    println!("Progress: {} events @ {:.0} events/sec",
        progress.events_processed,
        progress.events_per_second
    );
    
    if let Some(percentage) = progress.completion_percentage() {
        println!("  {:.1}% complete", percentage);
    }
    
    if let Some(eta) = progress.estimated_completion {
        let remaining = eta.duration_since(std::time::Instant::now());
        println!("  ETA: {:?}", remaining);
    }
}

// Get final result
let final_progress = rebuild_task.await??;
```

## Handling Large Rebuilds

For projections with millions of events, consider these strategies:

### 1. Batch Processing with Checkpoints

The rebuild system automatically saves checkpoints periodically (default: every 100 events):

```rust
// The rebuild processor handles checkpointing automatically
// You can monitor checkpoint saves in the logs
```

### 2. Parallel Rebuilds

For independent projections, run multiple rebuilds concurrently:

```rust
use futures::future::join_all;

async fn rebuild_all_projections(
    event_store: Arc<dyn EventStore<Event = MyEvent>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let rebuilds = vec![
        rebuild_projection("user-activity", event_store.clone()),
        rebuild_projection("order-summary", event_store.clone()),
        rebuild_projection("inventory-levels", event_store.clone()),
    ];
    
    let results = join_all(rebuilds).await;
    
    // Check all results
    for result in results {
        result?;
    }
    
    Ok(())
}
```

### 3. Cancellation Support

Long-running rebuilds can be cancelled gracefully:

```rust
use tokio::select;
use tokio::signal;

// Start rebuild with cancellation support
let coordinator = Arc::new(coordinator);
let coordinator_cancel = coordinator.clone();

let rebuild_task = tokio::spawn(async move {
    coordinator.rebuild_from_beginning().await
});

// Handle shutdown signal
select! {
    result = rebuild_task => {
        match result? {
            Ok(progress) => println!("Rebuild completed: {:?}", progress),
            Err(e) => eprintln!("Rebuild failed: {}", e),
        }
    }
    _ = signal::ctrl_c() => {
        println!("Received shutdown signal, cancelling rebuild...");
        coordinator_cancel.cancel();
        // The rebuild will stop at the next checkpoint
    }
}
```

## Production Best Practices

### 1. Zero-Downtime Rebuilds

Implement blue-green deployments for projections:

```rust
// 1. Create new projection instance with versioned name
let new_projection_name = format!("{}-v2", original_name);

// 2. Rebuild in background
let coordinator = create_rebuild_coordinator(&new_projection_name);
let progress = coordinator.rebuild_from_beginning().await?;

// 3. Verify new projection is healthy
verify_projection_health(&new_projection_name).await?;

// 4. Switch traffic to new projection
atomic_swap_projection(original_name, new_projection_name).await?;

// 5. Clean up old projection
cleanup_old_projection(&original_name).await?;
```

### 2. Incremental Rebuilds

For frequently updated projections, use incremental rebuilds:

```rust
use std::time::{Duration, SystemTime};

async fn incremental_rebuild(
    coordinator: &RebuildCoordinator<impl CqrsProjection, impl Send + Sync>,
    checkpoint_store: &dyn CheckpointStore,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load last rebuild checkpoint
    let last_checkpoint = checkpoint_store
        .load("last-rebuild")
        .await?
        .unwrap_or_else(|| ProjectionCheckpoint::initial());
    
    // Only rebuild if enough time has passed
    let time_since_last = SystemTime::now()
        .duration_since(last_checkpoint.checkpoint_time.into())?;
    
    if time_since_last < Duration::from_hours(24) {
        println!("Skipping rebuild - last rebuild was {:?} ago", time_since_last);
        return Ok(());
    }
    
    // Perform incremental rebuild
    let progress = coordinator
        .rebuild_from_checkpoint(last_checkpoint)
        .await?;
    
    // Save new checkpoint
    let new_checkpoint = ProjectionCheckpoint::initial()
        .with_last_event_id(Some(EventId::new()));
    checkpoint_store.save("last-rebuild", new_checkpoint).await?;
    
    Ok(())
}
```

### 3. Error Recovery

Handle rebuild failures gracefully:

```rust
use tracing::{error, warn};

async fn rebuild_with_retry(
    coordinator: Arc<RebuildCoordinator<impl CqrsProjection, impl Send + Sync>>,
    max_attempts: u32,
) -> Result<RebuildProgress, Box<dyn std::error::Error>> {
    let mut attempts = 0;
    let mut last_error = None;
    
    while attempts < max_attempts {
        attempts += 1;
        
        match coordinator.rebuild_from_beginning().await {
            Ok(progress) => return Ok(progress),
            Err(e) => {
                last_error = Some(e);
                
                if attempts < max_attempts {
                    let delay = Duration::from_secs(2_u64.pow(attempts));
                    warn!(
                        "Rebuild attempt {} failed, retrying in {:?}",
                        attempts, delay
                    );
                    tokio::time::sleep(delay).await;
                } else {
                    error!("Rebuild failed after {} attempts", attempts);
                }
            }
        }
    }
    
    Err(last_error.unwrap().into())
}
```

### 4. Monitoring and Alerting

Implement comprehensive monitoring:

```rust
use prometheus::{Counter, Gauge, Histogram};

struct RebuildMetrics {
    rebuild_duration: Histogram,
    events_processed: Counter,
    rebuild_failures: Counter,
    current_progress: Gauge,
}

async fn monitored_rebuild(
    coordinator: Arc<RebuildCoordinator<impl CqrsProjection, impl Send + Sync>>,
    metrics: Arc<RebuildMetrics>,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = std::time::Instant::now();
    
    // Start progress monitoring
    let metrics_clone = metrics.clone();
    let coordinator_clone = coordinator.clone();
    
    let monitor_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        
        loop {
            interval.tick().await;
            let progress = coordinator_clone.get_progress().await;
            
            if !progress.is_running {
                break;
            }
            
            metrics_clone.current_progress.set(
                progress.completion_percentage().unwrap_or(0.0)
            );
        }
    });
    
    // Perform rebuild
    match coordinator.rebuild_from_beginning().await {
        Ok(progress) => {
            metrics.events_processed.inc_by(progress.events_processed);
            metrics.rebuild_duration.observe(start.elapsed().as_secs_f64());
            Ok(())
        }
        Err(e) => {
            metrics.rebuild_failures.inc();
            Err(e.into())
        }
    }
}
```

## Testing Rebuild Logic

Always test your rebuild logic thoroughly:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use eventcore_memory::InMemoryEventStore;

    #[tokio::test]
    async fn test_rebuild_from_beginning_clears_existing_data() {
        let event_store = create_test_event_store().await;
        let (projection, read_store, checkpoint_store) = create_test_projection();
        
        // Add some initial data
        populate_read_models(&read_store).await;
        
        // Perform rebuild
        let coordinator = RebuildCoordinator::new(
            projection,
            event_store,
            read_store.clone(),
            checkpoint_store,
        );
        
        let progress = coordinator.rebuild_from_beginning().await.unwrap();
        
        // Verify all models were recreated
        assert!(progress.events_processed > 0);
        assert!(progress.models_updated > 0);
        
        // Verify old data was cleared
        verify_read_models_rebuilt(&read_store).await;
    }

    #[tokio::test]
    async fn test_rebuild_cancellation() {
        let coordinator = create_large_rebuild_scenario().await;
        
        // Start rebuild
        let coordinator_clone = coordinator.clone();
        let rebuild_task = tokio::spawn(async move {
            coordinator_clone.rebuild_from_beginning().await
        });
        
        // Cancel after short delay
        tokio::time::sleep(Duration::from_millis(100)).await;
        coordinator.cancel();
        
        // Verify cancellation
        match rebuild_task.await.unwrap() {
            Err(e) => assert!(e.to_string().contains("cancelled")),
            Ok(_) => panic!("Expected rebuild to be cancelled"),
        }
    }
}
```

## Troubleshooting Common Issues

### 1. Slow Rebuilds

If rebuilds are taking too long:
- Check database query performance and indexes
- Increase batch sizes for bulk operations
- Use connection pooling effectively
- Consider parallel processing where possible

### 2. Memory Usage

For large projections:
- Implement streaming instead of loading all events
- Use pagination for large result sets
- Clear intermediate state periodically
- Monitor memory usage during rebuilds

### 3. Consistency Issues

Ensure consistency during rebuilds:
- Use transactions for atomic updates
- Implement proper version checking
- Handle concurrent modifications
- Test with realistic concurrent load

## Summary

Projection rebuild is a critical capability for maintaining event-sourced systems in production. EventCore provides:

- **Multiple rebuild strategies** for different scenarios
- **Progress tracking** with completion estimates
- **Cancellation support** for long-running rebuilds
- **Automatic checkpointing** for resumability
- **Integration with subscriptions** for efficient event replay

By following the patterns and best practices in this tutorial, you can implement robust projection rebuild capabilities that handle production requirements gracefully.