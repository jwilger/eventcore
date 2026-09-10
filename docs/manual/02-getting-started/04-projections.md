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
use eventcore::{run_projection, ProjectionConfig};
use eventcore_memory::InMemoryEventStore;

async fn setup_projections() -> Result<(), Box<dyn std::error::Error>> {
    // Event store (already populated with events from command execution)
    let store = InMemoryEventStore::new();

    // Create and run a projection. `run_projection` takes the projector, a
    // reference to the backend, and a `ProjectionConfig`. The default config
    // runs in batch mode (process all available events, then return).
    let projection = UserTaskListProjection::default();
    run_projection(projection, &store, ProjectionConfig::default()).await?;

    Ok(())
}
```

This is the legacy projection API retained unchanged from EventCore 2.0.1. It
works across EventCore backends, but `Projector::apply` and checkpoint storage
are separate operations. Use it when that delivery model is sufficient; it does
not make an arbitrary read-model effect atomic with its checkpoint.

## Transactional PostgreSQL Projections

Enable the `postgres` feature when a PostgreSQL read model needs its mutation
and progress to commit atomically. This additive API is available through
`eventcore::postgres::projections`; existing `Projector` and `run_projection`
code remains source-compatible.

The source event store and destination read model are intentionally separate
arguments. They may be pools for different schemas in one database or entirely
different PostgreSQL databases. Only the destination participates in the
effect/progress transaction; this is not a distributed transaction with the
event store.

### End-to-end setup

The following is the core of the compiled consumer example in
`eventcore/tests/postgres_transactional_projection_api_test.rs`. Applications
use their own SQLx pool/query APIs, while importing `Postgres` and `Transaction`
from EventCore's facade so the projector signature always matches EventCore's
locked SQLx version (0.8.6 in the current workspace lockfile).

```rust,ignore
use std::future::Future;

use eventcore::postgres::projections::{
    DeliveryPosition, DeliverySourceId, EventTypeName, NoopAfterCommit, Postgres,
    PostgresProjectionConfig, PostgresProjectionSource, PostgresProjectionStore,
    PostgresProjector, ProjectionSelection, ProjectionSelectionId,
    ProjectionStreamFilter, ProjectorName, Transaction, run_transactional_projection,
};
use serde::Deserialize;
use sqlx::{PgPool, query};

#[derive(Deserialize)]
struct AccountCredited {
    amount: i64,
}

struct AccountTotals {
    name: ProjectorName,
}

impl PostgresProjector for AccountTotals {
    type Event = AccountCredited;
    type Error = sqlx::Error;
    type AfterCommit = NoopAfterCommit;

    fn name(&self) -> &ProjectorName {
        &self.name
    }

    fn apply<'a, 'c>(
        &'a mut self,
        event: &'a Self::Event,
        _position: DeliveryPosition,
        tx: &'a mut Transaction<'c, Postgres>,
    ) -> impl Future<Output = Result<Self::AfterCommit, Self::Error>> + Send + 'a
    where
        'c: 'a,
    {
        async move {
            let _ = query(
                "INSERT INTO account_totals (singleton, total) VALUES (TRUE, $1) \
                 ON CONFLICT (singleton) DO UPDATE \
                 SET total = account_totals.total + EXCLUDED.total",
            )
            .bind(event.amount)
            .execute(&mut **tx)
            .await?;
            Ok(NoopAfterCommit)
        }
    }
}

async fn catch_up(source_pool: PgPool, read_model_pool: PgPool)
    -> Result<(), Box<dyn std::error::Error>>
{
    // Run the normal event-store migration first. Then install the separate,
    // source-owned delivery migration on that same event-store database.
    let source = PostgresProjectionSource::from_pool(
        source_pool,
        DeliverySourceId::try_new("orders-primary")?,
    );
    source.migrate().await?;

    // The application migrates its read-model tables. EventCore separately
    // installs named progress and coordination state on the destination.
    let destination = PostgresProjectionStore::from_pool(read_model_pool);
    destination.migrate().await?;

    let selection = ProjectionSelection::try_new(
        ProjectionSelectionId::try_new("account-totals-events-v1")?,
        ProjectionStreamFilter::All,
        vec![EventTypeName::try_new("account-credited")?],
    )?;
    let projector = AccountTotals {
        name: ProjectorName::try_new("account-totals-v1")?,
    };

    let outcome = run_transactional_projection(
        projector,
        &source,
        &destination,
        PostgresProjectionConfig::new(selection), // batch mode
    )
    .await?;
    println!("{outcome:?}");
    Ok(())
}
```

Apply migrations in this order:

1. Run `PostgresEventStore::migrate()` on the event-store database.
2. Run `PostgresProjectionSource::migrate()` on that same source database.
3. Run the application's read-model migrations on the destination database.
4. Run `PostgresProjectionStore::migrate()` on the destination. EventCore uses
   a component-specific migration ledger rather than SQLx's application ledger.

The source migration establishes a positive, source-scoped global delivery
position in commit-safe order. Event UUIDs remain event identities; neither a
UUID nor a legacy `StreamPosition` is treated as this cross-stream delivery
position.

### Stable identity and progress

Treat these strings as persisted schema decisions:

- `DeliverySourceId` identifies the source delivery sequence.
- `ProjectorName` identifies the read model and its leadership/progress row.
- `ProjectionSelectionId` identifies the meaning of the stream filter and
  selected persisted event-type names.

Changing the source or selection while reusing existing progress returns an
identity-mismatch error. Use a new identity or perform a coordinated reset; do
not silently adopt the old row. `PostgresProjectionStore::progress` exposes the
last committed source/selection/position for operational inspection. Restarting
the same identities resumes after that position. A selected payload that cannot
deserialize into `PostgresProjector::Event` returns
`TransactionalProjectionError::Decode` without advancing progress. Unknown or
unselected event types are not decoded.

### Batch and continuous operation

`PostgresProjectionConfig::new(selection)` runs in batch mode. It captures a
source high-water mark, drains every page through that bound, and returns
`ProjectionRunOutcome::CaughtUp`; a batch is not limited to one page.

Continuous mode repeatedly catches up until its cancellation token is
cancelled:

```rust,ignore
use std::time::Duration;
use tokio_util::sync::CancellationToken;

let cancellation = CancellationToken::new();
let config = PostgresProjectionConfig::new(selection)
    .continuous(cancellation.clone())
    .with_continuous_poll_interval(Duration::from_millis(250))?;

// Run `run_transactional_projection(...)` in an owned task, then call
// `cancellation.cancel()` during graceful shutdown.
```

### Failure decisions, retries, and after-commit work

`PostgresProjector::on_error` makes application failure handling explicit:

- `Retry` rolls back the attempt, waits according to
  `ProjectionRetryPolicy`, and retries only up to its configured bound.
  Exhaustion returns `TransactionalProjectionError::RetryExhausted`.
- `Skip` rolls back the failed effect and then explicitly commits progress past
  that event. Use it only when losing that event's effect is an accepted domain
  decision.
- `Stop` rolls back and returns a stopped outcome with the position still
  pending.
- `Fatal` rolls back and returns the application error with the position still
  pending. This is the safe default.

```rust,ignore
let retry = ProjectionRetryPolicy::new(
    4,
    Duration::from_millis(100),
    2.0,
    Duration::from_secs(5),
)?;
let config = PostgresProjectionConfig::new(selection).with_retry_policy(retry);
```

`PostgresProjector::apply` returns an owned `AfterCommit` value. EventCore runs
it only after PostgreSQL acknowledges the effect/progress commit. If it fails,
`AfterCommitFailed` reports a position that is already committed; replay will
not rerun that callback. For durable messages or external side effects, write
an outbox row inside the supplied transaction and deliver the outbox
independently. A network call in `apply` or `AfterCommit` is not covered by the
database exactly-once guarantee.

If the connection fails while acknowledging `COMMIT`, EventCore returns
`CommitIndeterminate`: the effect and progress may both have committed or both
have rolled back. Do not blindly compensate or skip. Reconnect, inspect named
progress and the read model/outbox, then restart with the same identities.

### Leadership and pooling

Only one runner/reset for a `ProjectorName` may write at a time. The destination
store holds a PostgreSQL session advisory lock and runs every read-model
transaction on that same physical connection. A lost session fences the old
runner before another leader can continue.

Use direct or session-pooled PostgreSQL connections. Do not place the
destination behind transaction-pooling middleware: transaction pooling can
move transactions away from the session that owns the advisory lock and void
the leadership guarantee. The source pool has no such session-lock constraint.

### Reset and replay

`reset_transactional_projection` invokes application-owned
`PostgresProjectionReset` code and deletes matching progress in one destination
transaction under the same named leadership lock.
`reset_and_replay_transactional_projection` retains leadership from reset
through replay so another writer cannot enter between phases.

Schedule reset/replay as coordinated downtime for that read model and stop all
legacy and transactional writers first. Existing legacy UUID checkpoints are
not convertible to `DeliveryPosition`; reset the read model, choose stable new
transactional identities, and replay from the source. Baseline reset mutates the
live read model in place. It does not build, swap, or guarantee a shadow
generation, so queries may observe rebuilding state until replay completes.

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

## Legacy Real-time Updates

For continuous projection updates, configure `ProjectionConfig` in continuous
mode so `run_projection()` keeps polling for new events instead of returning
once it reaches the end of the stream:

```rust
use eventcore::{run_projection, ProjectionConfig};
use std::time::Duration;

let config = ProjectionConfig::default()
    .continuous()
    .poll_interval(Duration::from_millis(200));

run_projection(projection, &store, config).await?;
```

The default legacy `ProjectionConfig` runs in batch mode (process the currently
available events, then return). The legacy runner handles checkpointing and
resumption, but its checkpoint is not atomic with application read-model
effects. See the `projection-system` blueprint and ADR-0036 for details on this
continuous polling architecture.

## Filtering Which Events a Reader Sees

When reading events through the `EventReader` path (the cursor-based path that
backs projection runners and subscriptions), you can narrow the set of streams
with an `EventFilter` from `eventcore-types`:

```rust,ignore
use eventcore_types::{EventFilter, StreamPrefix, StreamPattern};

// Match every event in the store.
let all = EventFilter::all();

// Match streams whose ID starts with a literal prefix.
let prefix = EventFilter::prefix(StreamPrefix::try_new("account-")?);

// Match streams whose ID matches a POSIX glob pattern (`*`, `?`, `[...]`).
// `StreamPattern` supports glob metacharacters; `StreamPrefix` rejects them.
let pattern = EventFilter::pattern(StreamPattern::try_new("account-*")?);

// Optionally restrict to a single event type as well.
let typed = EventFilter::pattern(StreamPattern::try_new("order-*")?)
    .with_event_type("OrderPlaced".to_string());
```

Glob pattern filtering (`EventFilter::pattern` + `StreamPattern`) was added in
ADR-0047; literal prefix filtering remains available via `EventFilter::prefix`.
`EventFilter` lives in `eventcore-types` because it is part of the
reader/backend contract rather than the command-execution surface.

## Rebuilding Legacy Projections

One of the powerful features of event sourcing is the ability to rebuild
projections from scratch. Simply create a fresh projector instance and run
it against the store -- it will replay all events from the beginning:

```rust
use eventcore::{run_projection, ProjectionConfig};

async fn rebuild_projection(
    store: &InMemoryEventStore,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create a fresh projection (starts from the beginning)
    let projection = UserTaskListProjection::default();

    // Run it -- processes all events from the store
    run_projection(projection, store, ProjectionConfig::default()).await?;

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
    use eventcore::{execute, ProjectionConfig, RetryPolicy, run_projection};
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
        run_projection(collector, &store, ProjectionConfig::default()).await.unwrap();

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
