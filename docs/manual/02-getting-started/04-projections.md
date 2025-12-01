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
use eventcore::prelude::*;
use eventcore::cqrs::{CqrsProjection, ProjectionError};
use std::collections::{HashMap, HashSet};
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

/// Projection that maintains task lists for each user
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
                // Sort by priority (high to low) then by assigned date
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

    /// Get task by ID
    pub fn get_task(&self, task_id: &TaskId) -> Option<&TaskSummary> {
        self.task_assignments
            .get(task_id)
            .and_then(|user| {
                self.tasks_by_user
                    .get(user)?
                    .get(task_id)
            })
    }
}

#[async_trait]
impl CqrsProjection for UserTaskListProjection {
    type Event = SystemEvent;
    type Error = ProjectionError;

    async fn apply(&mut self, event: &StoredEvent<Self::Event>) -> Result<(), Self::Error> {
        match &event.payload {
            SystemEvent::Task(task_event) => {
                self.apply_task_event(task_event, &event.occurred_at)?;
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
    fn apply_task_event(
        &mut self,
        event: &TaskEvent,
        occurred_at: &DateTime<Utc>
    ) -> Result<(), ProjectionError> {
        match event {
            TaskEvent::Created { task_id, title, creator, .. } => {
                // Cache task details for later use
                self.task_details.insert(
                    *task_id,
                    TaskDetails {
                        title: title.to_string(),
                        created_at: *occurred_at,
                        priority: Priority::default(),
                    }
                );
            }

            TaskEvent::Assigned { task_id, assignee, assigned_at, .. } => {
                // Remove from previous assignee if any
                if let Some(previous_user) = self.task_assignments.get(task_id) {
                    if let Some(user_tasks) = self.tasks_by_user.get_mut(previous_user) {
                        user_tasks.remove(task_id);
                    }
                }

                // Add to new assignee
                let task_details = self.task_details.get(task_id)
                    .ok_or_else(|| ProjectionError::InvalidState(
                        format!("Task {} not found in cache", task_id)
                    ))?;

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

                self.task_assignments.insert(*task_id, assignee.clone());
            }

            TaskEvent::Unassigned { task_id, previous_assignee, .. } => {
                // Remove from assignee
                if let Some(user_tasks) = self.tasks_by_user.get_mut(previous_assignee) {
                    user_tasks.remove(task_id);
                }
                self.task_assignments.remove(task_id);
            }

            TaskEvent::Started { task_id, .. } => {
                // Update status
                if let Some(user) = self.task_assignments.get(task_id) {
                    if let Some(task) = self.tasks_by_user
                        .get_mut(user)
                        .and_then(|tasks| tasks.get_mut(task_id))
                    {
                        task.status = TaskStatus::InProgress;
                    }
                }
            }

            TaskEvent::Completed { task_id, completed_at, .. } => {
                // Update status and completion time
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

            TaskEvent::PriorityChanged { task_id, new_priority, .. } => {
                // Update priority in cache and summary
                if let Some(details) = self.task_details.get_mut(task_id) {
                    details.priority = *new_priority;
                }

                if let Some(user) = self.task_assignments.get(task_id) {
                    if let Some(task) = self.tasks_by_user
                        .get_mut(user)
                        .and_then(|tasks| tasks.get_mut(task_id))
                    {
                        task.priority = *new_priority;
                    }
                }
            }

            _ => {} // Handle other events as needed
        }

        Ok(())
    }
}
```

## Statistics Projection

Let's build another projection for team statistics:

### `src/projections/statistics.rs`

```rust
use crate::domain::{events::*, types::*};
use eventcore::prelude::*;
use eventcore::cqrs::{CqrsProjection, ProjectionError};
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc, Datelike};

/// Team statistics projection
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct TeamStatisticsProjection {
    /// Total tasks created
    pub total_tasks_created: u64,

    /// Tasks by status
    pub tasks_by_status: HashMap<TaskStatus, u64>,

    /// Tasks by priority
    pub tasks_by_priority: HashMap<Priority, u64>,

    /// User statistics
    pub user_stats: HashMap<UserName, UserStatistics>,

    /// Daily completion rates
    pub daily_completions: HashMap<String, u64>, // Date string -> count

    /// Average completion time in hours
    pub avg_completion_hours: f64,

    /// Completion times for average calculation
    completion_times: Vec<f64>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct UserStatistics {
    pub tasks_assigned: u64,
    pub tasks_completed: u64,
    pub tasks_in_progress: u64,
    pub total_comments: u64,
    pub avg_completion_hours: f64,
    completion_times: Vec<f64>,
}

impl TeamStatisticsProjection {
    /// Get completion rate percentage
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

    /// Get most productive user
    pub fn most_productive_user(&self) -> Option<(&UserName, u64)> {
        self.user_stats
            .iter()
            .max_by_key(|(_, stats)| stats.tasks_completed)
            .map(|(user, stats)| (user, stats.tasks_completed))
    }

    /// Get workload distribution
    pub fn workload_distribution(&self) -> Vec<(UserName, f64)> {
        let total_active: u64 = self.user_stats
            .values()
            .map(|s| s.tasks_in_progress)
            .sum();

        if total_active == 0 {
            return vec![];
        }

        self.user_stats
            .iter()
            .filter(|(_, stats)| stats.tasks_in_progress > 0)
            .map(|(user, stats)| {
                let percentage = (stats.tasks_in_progress as f64 / total_active as f64) * 100.0;
                (user.clone(), percentage)
            })
            .collect()
    }
}

#[async_trait]
impl CqrsProjection for TeamStatisticsProjection {
    type Event = SystemEvent;
    type Error = ProjectionError;

    async fn apply(&mut self, event: &StoredEvent<Self::Event>) -> Result<(), Self::Error> {
        match &event.payload {
            SystemEvent::Task(task_event) => {
                self.apply_task_event(task_event, &event.occurred_at)?;
            }
            SystemEvent::User(user_event) => {
                self.apply_user_event(user_event)?;
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "team_statistics"
    }
}

impl TeamStatisticsProjection {
    fn apply_task_event(
        &mut self,
        event: &TaskEvent,
        occurred_at: &DateTime<Utc>
    ) -> Result<(), ProjectionError> {
        match event {
            TaskEvent::Created { .. } => {
                self.total_tasks_created += 1;
                *self.tasks_by_status.entry(TaskStatus::Open).or_insert(0) += 1;
                *self.tasks_by_priority.entry(Priority::default()).or_insert(0) += 1;
            }

            TaskEvent::Assigned { assignee, .. } => {
                let stats = self.user_stats.entry(assignee.clone()).or_default();
                stats.tasks_assigned += 1;
                stats.tasks_in_progress += 1;
            }

            TaskEvent::Completed { task_id, completed_by, completed_at, .. } => {
                // Update status counts
                *self.tasks_by_status.entry(TaskStatus::Open).or_insert(0) =
                    self.tasks_by_status.get(&TaskStatus::Open).unwrap_or(&0).saturating_sub(1);
                *self.tasks_by_status.entry(TaskStatus::Completed).or_insert(0) += 1;

                // Update user stats
                let stats = self.user_stats.entry(completed_by.clone()).or_default();
                stats.tasks_completed += 1;
                stats.tasks_in_progress = stats.tasks_in_progress.saturating_sub(1);

                // Track daily completions
                let date_key = completed_at.format("%Y-%m-%d").to_string();
                *self.daily_completions.entry(date_key).or_insert(0) += 1;

                // Calculate completion time (would need task creation time)
                // For demo, using a placeholder
                let completion_hours = 24.0; // In real app, calculate from creation
                self.completion_times.push(completion_hours);
                stats.completion_times.push(completion_hours);

                // Update averages
                self.avg_completion_hours = self.completion_times.iter().sum::<f64>()
                    / self.completion_times.len() as f64;
                stats.avg_completion_hours = stats.completion_times.iter().sum::<f64>()
                    / stats.completion_times.len() as f64;
            }

            TaskEvent::CommentAdded { author, .. } => {
                let stats = self.user_stats.entry(author.clone()).or_default();
                stats.total_comments += 1;
            }

            TaskEvent::PriorityChanged { old_priority, new_priority, .. } => {
                *self.tasks_by_priority.entry(*old_priority).or_insert(0) =
                    self.tasks_by_priority.get(old_priority).unwrap_or(&0).saturating_sub(1);
                *self.tasks_by_priority.entry(*new_priority).or_insert(0) += 1;
            }

            _ => {}
        }

        Ok(())
    }

    fn apply_user_event(&mut self, event: &UserEvent) -> Result<(), ProjectionError> {
        // Handle user-specific events if needed
        Ok(())
    }
}
```

## Running Projections

EventCore provides infrastructure for running projections:

### Setting Up Projection Runner

```rust
use eventcore::prelude::*;
use eventcore::cqrs::{
    CqrsProjectionRunner,
    InMemoryCheckpointStore,
    InMemoryReadModelStore,
    ProjectionRunnerConfig,
};
use eventcore_memory::InMemoryEventStore;

async fn setup_projections() -> Result<(), Box<dyn std::error::Error>> {
    // Event store
    let event_store = InMemoryEventStore::<SystemEvent>::new();

    // Projection infrastructure
    let checkpoint_store = InMemoryCheckpointStore::new();
    let read_model_store = InMemoryReadModelStore::new();

    // Create projection
    let mut task_list_projection = UserTaskListProjection::default();

    // Configure runner
    let config = ProjectionRunnerConfig::default()
        .with_batch_size(100)
        .with_checkpoint_frequency(50);

    // Create and start runner
    let runner = CqrsProjectionRunner::new(
        event_store.clone(),
        checkpoint_store,
        read_model_store.clone(),
        config,
    );

    // Run projection
    runner.run_projection(&mut task_list_projection).await?;

    // Query the projection
    let alice_tasks = task_list_projection.get_user_tasks(
        &UserName::try_new("alice").unwrap()
    );

    println!("Alice has {} tasks", alice_tasks.len());

    Ok(())
}
```

## Querying Projections

EventCore provides a query builder for complex queries:

```rust
use eventcore::cqrs::{QueryBuilder, FilterOperator};

async fn query_tasks(
    projection: &UserTaskListProjection,
) -> Result<(), Box<dyn std::error::Error>> {
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

    Ok(())
}
```

## Real-time Updates

Projections can be updated in real-time as events are written:

```rust
use tokio::sync::RwLock;
use std::sync::Arc;

struct ProjectionService {
    projection: Arc<RwLock<UserTaskListProjection>>,
    event_store: Arc<dyn EventStore>,
}

impl ProjectionService {
    async fn start_real_time_updates(self) {
        let mut last_position = EventId::default();

        loop {
            // Poll for new events
            let events = self.event_store
                .read_all_events(ReadOptions::default().after(last_position))
                .await
                .unwrap_or_default();

            if !events.is_empty() {
                let mut projection = self.projection.write().await;

                for event in &events {
                    if let Err(e) = projection.apply(event).await {
                        eprintln!("Projection error: {}", e);
                    }
                    last_position = event.id.clone();
                }
            }

            // Sleep before next poll
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }
}
```

## Rebuilding Projections

One of the powerful features of event sourcing is the ability to rebuild projections:

```rust
use eventcore::cqrs::{RebuildCoordinator, RebuildStrategy};

async fn rebuild_projection(
    event_store: Arc<dyn EventStore>,
    projection: &mut UserTaskListProjection,
) -> Result<(), Box<dyn std::error::Error>> {
    let coordinator = RebuildCoordinator::new(event_store);

    // Clear existing state
    *projection = UserTaskListProjection::default();

    // Rebuild from beginning
    let strategy = RebuildStrategy::FromBeginning;

    coordinator.rebuild(projection, strategy).await?;

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
    use eventcore_testing::prelude::*;

    #[tokio::test]
    async fn test_user_task_list_projection() {
        let mut projection = UserTaskListProjection::default();

        // Create test events
        let task_id = TaskId::new();
        let alice = UserName::try_new("alice").unwrap();

        // Apply created event
        let created_event = create_test_event(
            StreamId::from_static("task-123"),
            SystemEvent::Task(TaskEvent::Created {
                task_id,
                title: TaskTitle::try_new("Test").unwrap(),
                description: TaskDescription::try_new("").unwrap(),
                creator: alice.clone(),
                created_at: Utc::now(),
            })
        );

        projection.apply(&created_event).await.unwrap();

        // Apply assigned event
        let assigned_event = create_test_event(
            StreamId::from_static("task-123"),
            SystemEvent::Task(TaskEvent::Assigned {
                task_id,
                assignee: alice.clone(),
                assigned_by: alice.clone(),
                assigned_at: Utc::now(),
            })
        );

        projection.apply(&assigned_event).await.unwrap();

        // Verify
        let tasks = projection.get_user_tasks(&alice);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task_id);
        assert_eq!(tasks[0].status, TaskStatus::Open);
    }

    #[tokio::test]
    async fn test_statistics_projection() {
        let mut projection = TeamStatisticsProjection::default();

        // Apply multiple events
        for i in 0..10 {
            let event = create_test_event(
                StreamId::from_static(&format!("task-{}", i)),
                SystemEvent::Task(TaskEvent::Created {
                    task_id: TaskId::new(),
                    title: TaskTitle::try_new("Task").unwrap(),
                    description: TaskDescription::try_new("").unwrap(),
                    creator: UserName::try_new("alice").unwrap(),
                    created_at: Utc::now(),
                })
            );
            projection.apply(&event).await.unwrap();
        }

        assert_eq!(projection.total_tasks_created, 10);
        assert_eq!(projection.completion_rate(), 0.0);
    }
}
```

## Performance Considerations

### 1. Batch Processing

Process events in batches for better performance:

```rust
let config = ProjectionRunnerConfig::default()
    .with_batch_size(1000)  // Process 1000 events at a time
    .with_checkpoint_frequency(100);  // Checkpoint every 100 events
```

### 2. Selective Projections

Only process relevant streams:

```rust
impl CqrsProjection for UserTaskListProjection {
    fn relevant_streams(&self) -> Vec<&str> {
        vec!["task-*", "user-*"]  // Only process task and user streams
    }
}
```

### 3. Caching

Use in-memory caching for frequently accessed data:

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
