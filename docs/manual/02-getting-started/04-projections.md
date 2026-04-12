# Chapter 2.4: Working with Projections

Projections transform your event streams into read models optimized for queries. This chapter shows how to build projections that answer specific questions about your data.

## What Are Projections?

Projections are read-side views built from events. They:

- Listen to event streams
- Apply events to build state
- Optimize for specific queries
- Can be rebuilt from scratch

Think of projections as materialized views that are kept up-to-date by processing events.

## Our First Projection: User Task List

Let's build a projection that answers: "What tasks does each user have?"

### `src/projections/task_list.rs`

```rust
use crate::domain::{events::*, types::*};
use eventcore::Projector;
use std::collections::HashMap;
use std::convert::Infallible;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};

/// A summary of a task for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub assigned_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Projection that maintains task lists for each user.
///
/// Implements the `Projector` trait so it can be run via `run_projection()`.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct UserTaskListProjection {
    /// Tasks indexed by user
    tasks_by_user: HashMap<UserName, HashMap<TaskId, TaskSummary>>,

    /// Reverse index: task to user
    task_assignments: HashMap<TaskId, UserName>,

    /// Task details cache
    task_details: HashMap<TaskId, TaskDetails>,
}

#[derive(Clone, Serialize, Deserialize)]
struct TaskDetails {
    title: String,
    created_at: DateTime<Utc>,
    priority: Priority,
}

impl UserTaskListProjection {
    /// Get all tasks for a user
    pub fn get_user_tasks(&self, user: &UserName) -> Vec<TaskSummary> {
        self.tasks_by_user
            .get(user)
            .map(|tasks| {
                let mut list: Vec<_> = tasks.values().cloned().collect();
                list.sort_by(|a, b| {
                    b.priority.cmp(&a.priority)
                        .then_with(|| a.assigned_at.cmp(&b.assigned_at))
                });
                list
            })
            .unwrap_or_default()
    }

    /// Get active task count for a user
    pub fn get_active_task_count(&self, user: &UserName) -> usize {
        self.tasks_by_user
            .get(user)
            .map(|tasks| {
                tasks.values()
                    .filter(|t| matches!(t.status, TaskStatus::Open | TaskStatus::InProgress))
                    .count()
            })
            .unwrap_or(0)
    }
}

impl Projector for UserTaskListProjection {
    type Event = SystemEvent;
    type Error = Infallible;
    type Context = ();

    fn apply(
        &mut self,
        event: Self::Event,
        _position: eventcore::StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        match event {
            SystemEvent::Task(task_event) => {
                self.apply_task_event(&task_event);
            }
            SystemEvent::User(_) => {
                // User events handled separately if needed
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "user_task_list"
    }
}

impl UserTaskListProjection {
    fn apply_task_event(&mut self, event: &TaskEvent) {
        match event {
            TaskEvent::Created { task_id, title, .. } => {
                self.task_details.insert(
                    *task_id,
                    TaskDetails {
                        title: title.to_string(),
                        created_at: Utc::now(),
                        priority: Priority::default(),
                    }
                );
            }

            TaskEvent::Assigned { task_id, assignee, assigned_at, .. } => {
                if let Some(previous_user) = self.task_assignments.get(task_id) {
                    if let Some(user_tasks) = self.tasks_by_user.get_mut(previous_user) {
                        user_tasks.remove(task_id);
                    }
                }

                if let Some(task_details) = self.task_details.get(task_id) {
                    let summary = TaskSummary {
                        id: *task_id,
                        title: task_details.title.clone(),
                        status: TaskStatus::Open,
                        priority: task_details.priority,
                        assigned_at: *assigned_at,
                        completed_at: None,
                    };

                    self.tasks_by_user
                        .entry(assignee.clone())
                        .or_default()
                        .insert(*task_id, summary);
                }

                self.task_assignments.insert(*task_id, assignee.clone());
            }

            TaskEvent::Completed { task_id, completed_at, .. } => {
                if let Some(user) = self.task_assignments.get(task_id) {
                    if let Some(task) = self.tasks_by_user
                        .get_mut(user)
                        .and_then(|tasks| tasks.get_mut(task_id))
                    {
                        task.status = TaskStatus::Completed;
                        task.completed_at = Some(*completed_at);
                    }
                }
            }

            _ => {} // Handle other events as needed
        }
    }
}
```

## Statistics Projection

Let's build another projection for team statistics:

### `src/projections/statistics.rs`

```rust
use crate::domain::{events::*, types::*};
use eventcore::Projector;
use std::collections::HashMap;
use std::convert::Infallible;
use serde::{Serialize, Deserialize};

/// Team statistics projection
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct TeamStatisticsProjection {
    pub total_tasks_created: u64,
    pub tasks_by_status: HashMap<TaskStatus, u64>,
    pub tasks_by_priority: HashMap<Priority, u64>,
    pub user_stats: HashMap<UserName, UserStatistics>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct UserStatistics {
    pub tasks_assigned: u64,
    pub tasks_completed: u64,
    pub tasks_in_progress: u64,
}

impl TeamStatisticsProjection {
    pub fn completion_rate(&self) -> f64 {
        if self.total_tasks_created == 0 {
            return 0.0;
        }
        let completed = self.tasks_by_status
            .get(&TaskStatus::Completed)
            .copied()
            .unwrap_or(0);
        (completed as f64 / self.total_tasks_created as f64) * 100.0
    }
}

impl Projector for TeamStatisticsProjection {
    type Event = SystemEvent;
    type Error = Infallible;
    type Context = ();

    fn apply(
        &mut self,
        event: Self::Event,
        _position: eventcore::StreamPosition,
        _ctx: &mut Self::Context,
    ) -> Result<(), Self::Error> {
        match event {
            SystemEvent::Task(task_event) => {
                self.apply_task_event(&task_event);
            }
            SystemEvent::User(_) => {}
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "team_statistics"
    }
}

impl TeamStatisticsProjection {
    fn apply_task_event(&mut self, event: &TaskEvent) {
        match event {
            TaskEvent::Created { .. } => {
                self.total_tasks_created += 1;
                *self.tasks_by_status.entry(TaskStatus::Open).or_insert(0) += 1;
            }
            TaskEvent::Assigned { assignee, .. } => {
                let stats = self.user_stats.entry(assignee.clone()).or_default();
                stats.tasks_assigned += 1;
                stats.tasks_in_progress += 1;
            }
            TaskEvent::Completed { completed_by, .. } => {
                *self.tasks_by_status.entry(TaskStatus::Open).or_insert(0) =
                    self.tasks_by_status.get(&TaskStatus::Open).unwrap_or(&0).saturating_sub(1);
                *self.tasks_by_status.entry(TaskStatus::Completed).or_insert(0) += 1;

                let stats = self.user_stats.entry(completed_by.clone()).or_default();
                stats.tasks_completed += 1;
                stats.tasks_in_progress = stats.tasks_in_progress.saturating_sub(1);
            }
            _ => {}
        }
    }
}
```

## Running Projections

EventCore provides infrastructure for running projections:

### Running a Projection

Use the `run_projection()` free function to process all events through your projector:

```rust
use eventcore::run_projection;
use eventcore_memory::InMemoryEventStore;

async fn setup_projections() -> Result<(), Box<dyn std::error::Error>> {
    // Event store (already populated with events from command execution)
    let store = InMemoryEventStore::new();

    // Create and run a projection
    let projection = UserTaskListProjection::default();
    run_projection(projection, &store).await?;

    Ok(())
}
```

## Querying Projections

Query your projection's state using the methods you defined on it:

```rust
fn query_tasks(projection: &UserTaskListProjection) {
    let alice = UserName::try_new("alice").unwrap();

    // Get all tasks for Alice
    let all_tasks = projection.get_user_tasks(&alice);

    // Filter high priority tasks
    let high_priority: Vec<_> = all_tasks
        .iter()
        .filter(|t| t.priority == Priority::High)
        .collect();

    // Get active tasks only
    let active_tasks: Vec<_> = all_tasks
        .iter()
        .filter(|t| matches!(t.status, TaskStatus::Open | TaskStatus::InProgress))
        .collect();

    println!("Alice's tasks:");
    println!("- Total: {}", all_tasks.len());
    println!("- High priority: {}", high_priority.len());
    println!("- Active: {}", active_tasks.len());
}
```

## Real-time Updates

For continuous projection updates, use `run_projection()` which polls for
new events. EventCore's projection system handles checkpointing and
resumption automatically. See the `projection-system` blueprint and
ADR-0036 for details on the continuous polling architecture.

## Rebuilding Projections

One of the powerful features of event sourcing is the ability to rebuild
projections from scratch. Simply create a fresh projector instance and run
it against the store -- it will replay all events from the beginning:

```rust
use eventcore::run_projection;

async fn rebuild_projection(
    store: &InMemoryEventStore,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create a fresh projection (starts from the beginning)
    let projection = UserTaskListProjection::default();

    // Run it -- processes all events from the store
    run_projection(projection, store).await?;

    println!("Projection rebuilt successfully");
    Ok(())
}
```

## Testing Projections

Testing projections is straightforward:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use eventcore::{execute, RetryPolicy, run_projection};
    use eventcore_memory::InMemoryEventStore;
    use eventcore_testing::EventCollector;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn test_projections_via_execute_and_run_projection() {
        // Given: A store with events from command execution
        let store = InMemoryEventStore::new();

        // Execute commands to populate the store
        let create = CreateTask {
            task_id: StreamId::try_new("task-123").unwrap(),
            title: TaskTitle::try_new("Test").unwrap(),
            description: TaskDescription::try_new("").unwrap(),
            creator: UserName::try_new("alice").unwrap(),
            priority: Priority::default(),
        };
        execute(&store, create, RetryPolicy::new()).await.unwrap();

        // Then: Run an EventCollector to gather events
        let storage: Arc<Mutex<Vec<SystemEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let collector = EventCollector::new(storage.clone());
        run_projection(collector, &store).await.unwrap();

        let events = storage.lock().unwrap();
        assert_eq!(events.len(), 1);
    }
}
```

> **Note:** The `eventcore-testing` crate provides `EventCollector` for
> gathering events in tests. For custom projections, implement the
> `Projector` trait and use `run_projection()` to process events.

## Performance Considerations

### 1. Projector Design

Keep your `Projector::apply()` implementation fast and focused. Each call
processes a single event, so avoid expensive I/O inside the apply method.

### 2. Selective Processing

Filter events within your `apply()` method to only process relevant ones:

```rust
fn apply(
    &mut self,
    event: Self::Event,
    _position: eventcore::StreamPosition,
    _ctx: &mut Self::Context,
) -> Result<(), Self::Error> {
    // Only process task events, ignore user events
    if let SystemEvent::Task(task_event) = event {
        self.handle_task_event(&task_event);
    }
    Ok(())
}
```

### 3. Caching

Use in-memory caching for frequently accessed projection data:

```rust
struct CachedProjection {
    inner: UserTaskListProjection,
    cache: HashMap<UserName, Vec<TaskSummary>>,
    cache_ttl: Duration,
}
```

## Common Patterns

### 1. Denormalized Views

Projections often denormalize data for query performance:

```rust
// Instead of joins, store everything needed
struct TaskView {
    task_id: TaskId,
    title: String,
    assignee_name: String,      // Denormalized
    assignee_email: String,     // Denormalized
    creator_name: String,       // Denormalized
    // ... all data needed for display
}
```

### 2. Multiple Projections

Create different projections for different query needs:

- `UserTaskListProjection` - For user-specific views
- `TeamDashboardProjection` - For manager overview
- `SearchIndexProjection` - For full-text search
- `ReportingProjection` - For analytics

### 3. Event Enrichment

Projections can enrich events with additional context:

```rust
async fn enrich_event(&self, event: &TaskEvent) -> EnrichedTaskEvent {
    // Add user details, timestamps, etc.
}
```

## Summary

Projections in EventCore:

- ✅ Transform events into query-optimized read models
- ✅ Can be rebuilt from events at any time
- ✅ Support real-time updates
- ✅ Enable complex queries without affecting write performance
- ✅ Allow multiple views of the same data

Key benefits:

- **Flexibility**: Change read models without touching events
- **Performance**: Optimized for specific queries
- **Evolution**: Add new projections as needs change
- **Testing**: Easy to test with synthetic events

Next, let's look at [testing your application](./05-testing.md) →
