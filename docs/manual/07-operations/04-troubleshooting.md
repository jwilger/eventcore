# Chapter 6.4: Troubleshooting

This chapter provides comprehensive troubleshooting guidance for EventCore applications in production. From common issues to advanced debugging techniques, you'll learn to diagnose and resolve problems quickly.

> **A note on the code in this chapter.** EventCore is a library, not a service.
> It exposes no CLI binary, reads no environment variables, and ships no metrics
> endpoint or `monitoring` module. The diagnostic helpers below (corruption
> scanners, lag monitors, tracers, profilers, log analyzers) are
> **application-level patterns you write in your own code** — they are not part
> of EventCore's public API. Where they call EventCore, they use only the real
> public surface: `execute()`, `run_projection()`, `EventStore::read_stream` /
> `append_events`, `collect_events`, and the real domain types. Telemetry hooks
> into command execution itself are provided through the
> [`MetricsHook`](#observing-command-execution-with-metricshook) trait and
> `RetryPolicy::with_metrics_hook`; everything else (Prometheus exporters, HTTP
> health endpoints, container orchestration) is owned by your application.

## Common Issues and Solutions

### Command Execution Failures

#### Issue: Commands timing out

**Symptoms:**

- Commands taking longer than expected
- Timeout errors in logs
- Degraded system performance

**Debugging steps:**

`execute()` takes the store and command **by value** and returns an
`ExecutionResponse` whose only accessor is `attempts()`. The helper below wraps
a single execution with `tracing` so you can see how long it took and how many
attempts EventCore's built-in retry needed. Because `execute()` consumes the
command, clone it (or hold the data needed for logging) before the call if you
need it afterwards.

```rust
use eventcore::{execute, CommandError, CommandLogic, CommandStreams, ExecutionResponse, RetryPolicy};
use eventcore_types::EventStore;

// Enable detailed command tracing. `C: CommandStreams` is required to read the
// command's declared streams; `CommandLogic` extends `CommandStreams`.
#[tracing::instrument(skip(command, store), level = "debug")]
async fn debug_command_execution<C, S>(
    command: C,
    store: S,
) -> Result<ExecutionResponse, CommandError>
where
    C: CommandLogic,
    S: EventStore,
{
    let start = std::time::Instant::now();

    tracing::debug!(
        command_type = std::any::type_name::<C>(),
        "Starting command execution"
    );

    // Check stream access patterns. stream_declarations() returns a
    // StreamDeclarations value; iterate it with .iter() and count with .len().
    let stream_declarations = command.stream_declarations();
    tracing::debug!(
        stream_count = stream_declarations.len(),
        streams = ?stream_declarations.iter().collect::<Vec<_>>(),
        "Command will read from streams"
    );

    let result = execute(store, command, RetryPolicy::new()).await;
    let total_duration = start.elapsed();

    match &result {
        Ok(response) => {
            tracing::info!(
                total_duration_ms = total_duration.as_millis(),
                attempts = response.attempts(),
                "Command completed successfully"
            );
        }
        Err(error) => {
            tracing::error!(
                total_duration_ms = total_duration.as_millis(),
                error = %error,
                "Command failed"
            );
        }
    }

    result
}
```

**Common causes and solutions:**

1. **Database connection pool exhaustion**

   The PostgreSQL backend owns a `sqlx` pool internally. You configure its
   bounds through `PostgresConfig` when constructing the store; EventCore does
   not expose the live pool for inspection, so monitor it from the application
   that owns the `sqlx::Pool` (if you build one yourself via
   `PostgresEventStore::from_pool`) or rely on PostgreSQL's own statistics
   views.

   ```rust
   use eventcore::postgres::{PostgresConfig, PostgresEventStore};
   use eventcore_postgres::MaxConnections;
   use std::num::NonZeroU32;
   use std::time::Duration;

   // Tune the pool when you build the store. Defaults: 10 connections,
   // 30s acquire timeout, 600s idle timeout. MaxConnections wraps a
   // NonZeroU32 (the pool must have at least one connection).
   let config = PostgresConfig {
       max_connections: MaxConnections::new(NonZeroU32::new(20).expect("20 is non-zero")),
       acquire_timeout: Duration::from_secs(30),
       idle_timeout: Duration::from_secs(600),
   };
   let store = PostgresEventStore::with_config(database_url, config).await?;
   store.migrate().await;
   ```

   If you construct the `sqlx::Pool` yourself and pass it via
   `PostgresEventStore::from_pool(pool)`, you can monitor the pool directly:

   ```rust
   // Application-level helper: only works when YOU own the sqlx::Pool.
   async fn diagnose_connection_pool(pool: &sqlx::PgPool) {
       let max = pool.options().get_max_connections();
       let size = pool.size();
       let idle = pool.num_idle();

       tracing::info!(
           max_connections = max,
           current_size = size,
           idle_connections = idle,
           active_connections = size - idle as u32,
           "Connection pool status"
       );

       let utilization = (size as f64) / (max as f64);
       if utilization > 0.8 {
           tracing::warn!(utilization_percent = utilization * 100.0, "High connection pool utilization");
       }
   }
   ```

2. **Long-running database queries**

   ```sql
   -- PostgreSQL: Check for long-running queries
   SELECT
       pid,
       now() - pg_stat_activity.query_start AS duration,
       query,
       state
   FROM pg_stat_activity
   WHERE (now() - pg_stat_activity.query_start) > interval '5 minutes'
   AND state = 'active';
   ```

3. **Concurrency conflicts on streams**

   `execute()` already retries on optimistic-concurrency conflicts according to
   the `RetryPolicy` you pass. When all retries are exhausted it returns
   `CommandError::ConcurrencyError(attempts)`, where `attempts` is the number of
   attempts made. The example below disables EventCore's built-in retry
   (`max_retries(0)`) so an **outer** loop owns the retry behavior — useful when
   you want custom backoff or logging per attempt.

   ```rust
   use eventcore::{execute, CommandError, CommandLogic, ExecutionResponse, RetryPolicy};
   use eventcore_types::EventStore;
   use std::time::Duration;

   async fn execute_with_outer_retry<C, S>(
       command: C,
       store: &S,
       max_retries: u32,
   ) -> Result<ExecutionResponse, CommandError>
   where
       C: CommandLogic + Clone,
       S: EventStore + Clone,
   {
       // Zero built-in retries: this outer loop owns retry behavior. Most
       // applications should instead configure RetryPolicy and let execute()
       // handle conflicts.
       let no_retry = RetryPolicy::new().max_retries(0);
       let mut retry_count = 0;

       loop {
           // execute() consumes the store and command, so clone per attempt.
           match execute(store.clone(), command.clone(), no_retry.clone()).await {
               Ok(result) => return Ok(result),
               Err(CommandError::ConcurrencyError(attempts)) => {
                   retry_count += 1;
                   if retry_count >= max_retries {
                       return Err(CommandError::ConcurrencyError(attempts));
                   }

                   // Exponential backoff
                   let delay = Duration::from_millis(100 * 2_u64.pow(retry_count - 1));
                   tokio::time::sleep(delay).await;

                   tracing::warn!(
                       retry_attempt = retry_count,
                       delay_ms = delay.as_millis(),
                       inner_attempts = attempts,
                       "Retrying command due to concurrency conflict"
                   );
               }
               Err(other_error) => return Err(other_error),
           }
       }
   }
   ```

   In most cases you do **not** need an outer loop: prefer configuring the
   built-in policy with `RetryPolicy::new().max_retries(n)` and an exponential
   `BackoffStrategy` (see
   [Observing command execution](#observing-command-execution-with-metricshook)).

#### Issue: Command validation failures

**Symptoms:**

- Validation errors in command processing
- Business rule violations
- Data consistency issues

**Debugging approach:**

Command business-rule failures surface as
`CommandError::BusinessRuleViolation(Box<dyn Error + Send + Sync>)`, which is
produced from your command's `handle()` method (usually via the `require!`
macro or a typed error enum with a `From<E> for CommandError` impl). When
diagnosing failures, define a rich application error type so the wrapped error
carries enough context to act on. The type below is **your** error enum, not an
EventCore type — convert it into `CommandError` at the `handle()` boundary.

```rust
// Application-level diagnostic error. Convert into CommandError via the
// blanket `From<E: Error> ...` pattern or `CommandError::business_rule_violated`.
#[derive(Debug, thiserror::Error)]
pub enum DetailedValidationError {
    #[error("field validation failed: {field} - {reason}")]
    FieldValidation { field: String, reason: String },

    #[error("business rule violation: {rule} - {context}")]
    BusinessRule { rule: String, context: String },

    #[error("state precondition failed: expected {expected}, found {actual}")]
    StatePrecondition { expected: String, actual: String },

    #[error("reference validation failed: {reference_type} {reference_id} not found")]
    ReferenceNotFound { reference_type: String, reference_id: String },
}

// Validation with detailed context, written against your own domain types.
pub fn validate_transfer_command(
    command: &TransferMoney,
    state: &AccountState,
) -> Result<(), DetailedValidationError> {
    // Check amount
    if command.amount() <= Money::zero() {
        return Err(DetailedValidationError::FieldValidation {
            field: "amount".to_string(),
            reason: format!("amount must be positive, got {}", command.amount()),
        });
    }

    // Check account state (state queries are methods, not public fields)
    if !state.is_active() {
        return Err(DetailedValidationError::StatePrecondition {
            expected: "active account".to_string(),
            actual: "inactive account".to_string(),
        });
    }

    // Check sufficient balance
    if state.balance() < command.amount() {
        return Err(DetailedValidationError::BusinessRule {
            rule: "sufficient_balance".to_string(),
            context: format!(
                "balance {} insufficient for transfer {}",
                state.balance(), command.amount()
            ),
        });
    }

    Ok(())
}
```

Inside `handle()`, convert the failure into a `CommandError`:

```rust
validate_transfer_command(self, &state)
    .map_err(CommandError::business_rule_violated)?;
```

`CommandError::business_rule_violated` boxes any `Error + Send + Sync`,
preserving the original error chain (do not stringify the error — that discards
the source chain).

### Event Store Issues

#### Issue: High event store latency

**Diagnosis tools:**

EventCore's `EventStore` trait is **not object-safe** — `read_stream<E>` is a
generic method returning `impl Future` (RPITIT), so you cannot write
`Box<dyn EventStore>` or `Arc<dyn EventStore>`. Diagnostic wrappers must
therefore be **generic over the concrete store type** (`S: EventStore`) or hold
a concrete backend (`PostgresEventStore`, `SqliteEventStore`,
`InMemoryEventStore`). The monitor below wraps an arbitrary async operation that
returns `Result<T, EventStoreError>` and records its latency — it does not need
the store directly, which keeps it backend-agnostic.

```rust
use eventcore_types::EventStoreError;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

// Application-level latency wrapper. Pass it any future that performs a store
// operation (e.g. store.read_stream(...) or store.append_events(...)).
#[derive(Clone)]
pub struct EventStoreMonitor {
    latency_tracker: Arc<Mutex<LatencyTracker>>,
}

impl EventStoreMonitor {
    pub async fn monitor_operation<F, T>(
        &self,
        operation_name: &str,
        operation: F,
    ) -> Result<T, EventStoreError>
    where
        F: Future<Output = Result<T, EventStoreError>>,
    {
        let start = std::time::Instant::now();
        let result = operation.await;
        let duration = start.elapsed();

        {
            let mut tracker = self.latency_tracker.lock().await;
            tracker.record_operation(operation_name, duration, result.is_ok());
        }

        if duration > Duration::from_millis(1000) {
            tracing::warn!(
                operation = operation_name,
                duration_ms = duration.as_millis(),
                success = result.is_ok(),
                "High latency event store operation"
            );
        }

        result
    }

    pub async fn get_performance_report(&self) -> PerformanceReport {
        let tracker = self.latency_tracker.lock().await;
        tracker.generate_report()
    }
}

#[derive(Debug)]
pub struct LatencyTracker {
    operations: std::collections::HashMap<String, Vec<OperationMetric>>,
}

#[derive(Debug, Clone)]
struct OperationMetric {
    duration: Duration,
    success: bool,
    timestamp: chrono::DateTime<chrono::Utc>,
}

impl LatencyTracker {
    pub fn record_operation(&mut self, operation: &str, duration: Duration, success: bool) {
        let metric = OperationMetric {
            duration,
            success,
            timestamp: chrono::Utc::now(),
        };

        self.operations
            .entry(operation.to_string())
            .or_default()
            .push(metric);

        // Keep only recent metrics (last hour)
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(1);
        for metrics in self.operations.values_mut() {
            metrics.retain(|m| m.timestamp > cutoff);
        }
    }

    pub fn generate_report(&self) -> PerformanceReport {
        let mut report = PerformanceReport::default();

        for (operation, metrics) in &self.operations {
            if metrics.is_empty() {
                continue;
            }

            let durations: Vec<_> = metrics.iter().map(|m| m.duration).collect();
            let success_rate =
                metrics.iter().filter(|m| m.success).count() as f64 / metrics.len() as f64;

            let operation_stats = OperationStats {
                operation_name: operation.clone(),
                total_operations: metrics.len(),
                success_rate,
                avg_duration: durations.iter().sum::<Duration>() / durations.len() as u32,
                p95_duration: calculate_percentile(&durations, 0.95),
                p99_duration: calculate_percentile(&durations, 0.99),
            };

            report.operations.push(operation_stats);
        }

        report
    }
}

fn calculate_percentile(durations: &[Duration], percentile: f64) -> Duration {
    let mut sorted = durations.to_vec();
    sorted.sort();
    let index = ((sorted.len() as f64 - 1.0) * percentile) as usize;
    sorted[index]
}
```

Call it by wrapping a real store operation:

```rust
let stream_id = StreamId::try_new("account-123")?;
let stream = monitor
    .monitor_operation("read_stream", store.read_stream::<AccountEvent>(stream_id))
    .await?;
let events: Vec<AccountEvent> = eventcore::collect_events(stream).await?;
```

**PostgreSQL-specific debugging:**

```sql
-- Check for blocking queries
SELECT
    blocked_locks.pid AS blocked_pid,
    blocked_activity.usename AS blocked_user,
    blocking_locks.pid AS blocking_pid,
    blocking_activity.usename AS blocking_user,
    blocked_activity.query AS blocked_statement,
    blocking_activity.query AS blocking_statement
FROM pg_catalog.pg_locks blocked_locks
JOIN pg_catalog.pg_stat_activity blocked_activity
    ON blocked_activity.pid = blocked_locks.pid
JOIN pg_catalog.pg_locks blocking_locks
    ON blocking_locks.locktype = blocked_locks.locktype
    AND blocking_locks.DATABASE IS NOT DISTINCT FROM blocked_locks.DATABASE
    AND blocking_locks.relation IS NOT DISTINCT FROM blocked_locks.relation
    AND blocking_locks.pid != blocked_locks.pid
JOIN pg_catalog.pg_stat_activity blocking_activity
    ON blocking_activity.pid = blocking_locks.pid
WHERE NOT blocked_locks.GRANTED;

-- Check index usage
SELECT
    schemaname,
    tablename,
    indexname,
    idx_scan,
    idx_tup_read,
    idx_tup_fetch
FROM pg_stat_user_indexes
WHERE idx_scan < 100
ORDER BY idx_scan;

-- Check table and index sizes
SELECT
    schemaname,
    tablename,
    pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) as size
FROM pg_tables
WHERE schemaname = 'public'
ORDER BY pg_total_relation_size(schemaname||'.'||tablename) DESC;
```

#### Issue: Suspected event store corruption

The PostgreSQL backend enforces event immutability with database triggers that
reject any `UPDATE` or `DELETE` on event rows, and optimistic concurrency
control prevents version gaps from being written in the first place. EventCore
therefore does **not** ship a corruption scanner, a `list_all_streams()`
operation, or per-event version/id/timestamp accessors — the `Event` trait
exposes only `stream_id()` and `event_type_name()`. If you maintain a registry
of your own stream IDs (commands always know which streams they write), you can
build an application-level consistency check by reading each stream and
verifying that its events deserialize and fold cleanly.

```rust
use eventcore::collect_events;
use eventcore_types::{Event, EventStore, EventStoreError, StreamId};

// Application-level integrity check. `S: EventStore` because the EventStore
// trait is not object-safe (no `dyn EventStore`). You supply the list of
// stream IDs you care about — there is no "scan everything" store API.
pub struct StreamIntegrityChecker<S> {
    store: S,
}

impl<S: EventStore> StreamIntegrityChecker<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Read each named stream and confirm every event deserializes. A
    /// `DeserializationFailed` error is the real signal that an event row
    /// cannot be decoded with the current schema.
    pub async fn check_streams<E: Event>(
        &self,
        stream_ids: impl IntoIterator<Item = StreamId>,
    ) -> Vec<StreamIntegrityReport> {
        let mut reports = Vec::new();

        for stream_id in stream_ids {
            let report = match self.store.read_stream::<E>(stream_id.clone()).await {
                Ok(stream) => match collect_events::<E, _>(stream).await {
                    Ok(events) => StreamIntegrityReport {
                        stream_id,
                        event_count: events.len(),
                        issue: None,
                    },
                    Err(EventStoreError::DeserializationFailed { detail, .. }) => {
                        StreamIntegrityReport {
                            stream_id,
                            event_count: 0,
                            issue: Some(format!("deserialization failed: {detail}")),
                        }
                    }
                    Err(e) => StreamIntegrityReport {
                        stream_id,
                        event_count: 0,
                        issue: Some(format!("read failed: {e}")),
                    },
                },
                Err(e) => StreamIntegrityReport {
                    stream_id,
                    event_count: 0,
                    issue: Some(format!("could not open stream: {e}")),
                },
            };
            reports.push(report);
        }

        reports
    }
}

#[derive(Debug)]
pub struct StreamIntegrityReport {
    pub stream_id: StreamId,
    pub event_count: usize,
    pub issue: Option<String>,
}
```

If decode failures appear after a schema change, the fix is **schema
evolution**, not data repair. EventCore deliberately has no upcasting subsystem
(ADR-0035): make additive changes backwards-compatible with serde field
defaults (`#[serde(default)]`), and handle incompatible changes by adding **new
event variants** that your `handle()`/`apply()` logic and projectors match
alongside the old ones. See the schema-evolution guidance in the operations
chapters.

### Projection Issues

#### Issue: Projection lag

**Monitoring and diagnosis:**

EventCore runs projections through `run_projection(projector, &backend, config)`
where `backend` implements `EventReader`, `CheckpointStore`, and
`ProjectorCoordinator`. Checkpoints are stored as a `StreamPosition` cursor,
persisted and loaded through the
`CheckpointStore` trait. There is no built-in "lag in wall-clock minutes"
metric and no `ProjectionManager` type — lag monitoring is an application-level
concern you build on top of the backend's `CheckpointStore`. The monitor below
is **your** code; `B: CheckpointStore` keeps it generic over the concrete
backend.

```rust
use eventcore_types::{CheckpointStore, StreamPosition};

// Application-level lag monitor built on the real CheckpointStore trait.
pub struct ProjectionLagMonitor<B> {
    backend: B,
    // Names of the projectors you run. EventCore does not enumerate them for you.
    projection_names: Vec<String>,
}

impl<B: CheckpointStore> ProjectionLagMonitor<B>
where
    B::Error: std::fmt::Display,
{
    pub fn new(backend: B, projection_names: Vec<String>) -> Self {
        Self { backend, projection_names }
    }

    pub async fn check_all_projections(&self) -> Vec<ProjectionLagReport> {
        let mut reports = Vec::new();
        for name in &self.projection_names {
            reports.push(self.check_projection(name).await);
        }
        reports
    }

    async fn check_projection(&self, name: &str) -> ProjectionLagReport {
        // load() returns Option<StreamPosition>: None means "never checkpointed".
        match self.backend.load(name).await {
            Ok(position) => ProjectionLagReport {
                projection_name: name.to_string(),
                last_position: position,
                status: if position.is_some() {
                    ProjectionStatus::Healthy
                } else {
                    ProjectionStatus::NeverProcessed
                },
                error: None,
            },
            Err(e) => ProjectionLagReport {
                projection_name: name.to_string(),
                last_position: None,
                status: ProjectionStatus::Unknown,
                error: Some(e.to_string()),
            },
        }
    }
}

#[derive(Debug)]
pub struct ProjectionLagReport {
    pub projection_name: String,
    pub last_position: Option<StreamPosition>,
    pub status: ProjectionStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ProjectionStatus {
    Healthy,
    NeverProcessed,
    Unknown,
}
```

Within one reader's delivery sequence, you can compare a projector's last
checkpoint against the most recent event position your application has observed
to estimate how far behind it is. For filesystem stores after Git merges, make
that comparison on the same replica because cursors are replica-local. In every
case, the "latest position" must come from your own bookkeeping; EventCore does
not provide a store-wide query.

**Projection rebuild when state is corrupted:**

A projection's read model is rebuilt by **resetting its checkpoint and replaying
events through `run_projection` in batch mode** (the default). Batch mode
processes all currently-available events and then returns, which is exactly what
a rebuild needs. There is no `ProjectionManager.reset_projection()` or
`process_events_batch()` API — you reset the checkpoint via your
`CheckpointStore` implementation (and clear your read-model storage), then run
the projector.

```rust
use eventcore::{run_projection, ProjectionConfig};
use eventcore_types::{CheckpointStore, Projector};

// Application-level rebuild orchestration. The "reset" step is specific to
// YOUR CheckpointStore + read-model storage; only run_projection is EventCore.
pub async fn rebuild_projection<P, B>(
    projector: P,
    backend: &B,
    reset_read_model: impl AsyncFnOnce() -> Result<(), RebuildError>,
) -> Result<(), RebuildError>
where
    P: Projector,
    P::Event: eventcore_types::Event + Clone,
    P::Context: Default,
    P::Error: std::fmt::Debug,
    B: eventcore_types::EventReader
        + CheckpointStore
        + eventcore_types::ProjectorCoordinator,
    <B as eventcore_types::EventReader>::Error: std::fmt::Display,
{
    tracing::info!(projector = projector.name(), "Starting projection rebuild");

    // 1. Clear the existing read-model state (your storage) and checkpoint.
    //    How you reset the checkpoint depends on your CheckpointStore impl.
    reset_read_model().await?;

    // 2. Replay everything in batch mode (the default). run_projection returns
    //    once all currently-available events are processed.
    run_projection(projector, backend, ProjectionConfig::default())
        .await
        .map_err(RebuildError::Projection)?;

    tracing::info!("Projection rebuild completed");
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum RebuildError {
    #[error("read-model reset failed: {0}")]
    ReadModelReset(String),
    #[error("projection failed: {0}")]
    Projection(eventcore::ProjectionError),
}
```

> **Safety note.** Take a backup of the read-model store before resetting it.
> Because the event log is immutable and append-only, the source of truth is
> never at risk during a rebuild — only the derived read model is rewritten, so
> a failed rebuild can be retried by replaying again.

## Debugging Tools

### Command Execution Tracer

The tracer below is an **application-level** structure for recording per-command
diagnostic spans. The only EventCore APIs it uses are `execute()`,
`ExecutionResponse`, and the command's `stream_declarations()`. Note the generic
bound is `C: CommandLogic` — there is no trait named `Command` (that name is the
`#[derive(Command)]` macro); the command traits are `CommandStreams` and
`CommandLogic`.

```rust
use eventcore::{execute, CommandError, CommandLogic, ExecutionResponse, RetryPolicy};
use eventcore_types::EventStore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone)]
pub struct CommandTracer {
    traces: Arc<Mutex<HashMap<Uuid, CommandTrace>>>,
}

#[derive(Debug, Clone)]
pub struct CommandTrace {
    pub trace_id: Uuid,
    pub command_type: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub phases: Vec<TracePhase>,
    pub completed: bool,
    pub result: Option<Result<String, String>>,
}

#[derive(Debug, Clone)]
pub struct TracePhase {
    pub phase_name: String,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub duration: Option<std::time::Duration>,
    pub details: HashMap<String, String>,
}

impl CommandTracer {
    // Bound on CommandLogic (the real command trait), not a nonexistent
    // `Command` trait.
    pub fn start_trace<C: CommandLogic>(&self, _command: &C) -> Uuid {
        let trace_id = Uuid::new_v4();
        let trace = CommandTrace {
            trace_id,
            command_type: std::any::type_name::<C>().to_string(),
            start_time: chrono::Utc::now(),
            phases: Vec::new(),
            completed: false,
            result: None,
        };

        if let Ok(mut traces) = self.traces.lock() {
            traces.insert(trace_id, trace);
        }

        tracing::info!(
            trace_id = %trace_id,
            command_type = std::any::type_name::<C>(),
            "Started command trace"
        );

        trace_id
    }

    pub fn add_phase(&self, trace_id: Uuid, phase_name: &str, details: HashMap<String, String>) {
        if let Ok(mut traces) = self.traces.lock() {
            if let Some(trace) = traces.get_mut(&trace_id) {
                trace.phases.push(TracePhase {
                    phase_name: phase_name.to_string(),
                    start_time: chrono::Utc::now(),
                    duration: None,
                    details,
                });
            }
        }
    }

    pub fn complete_phase(&self, trace_id: Uuid) {
        if let Ok(mut traces) = self.traces.lock() {
            if let Some(trace) = traces.get_mut(&trace_id) {
                if let Some(last_phase) = trace.phases.last_mut() {
                    last_phase.duration = chrono::Utc::now()
                        .signed_duration_since(last_phase.start_time)
                        .to_std()
                        .ok();
                }
            }
        }
    }

    pub fn complete_trace(&self, trace_id: Uuid, result: Result<String, String>) {
        if let Ok(mut traces) = self.traces.lock() {
            if let Some(trace) = traces.get_mut(&trace_id) {
                trace.completed = true;
                let success = result.is_ok();
                trace.result = Some(result);

                let total_duration =
                    chrono::Utc::now().signed_duration_since(trace.start_time);

                tracing::info!(
                    trace_id = %trace_id,
                    duration_ms = total_duration.num_milliseconds(),
                    phases = trace.phases.len(),
                    success,
                    "Completed command trace"
                );
            }
        }
    }

    pub fn get_trace(&self, trace_id: Uuid) -> Option<CommandTrace> {
        self.traces.lock().ok()?.get(&trace_id).cloned()
    }

    pub fn get_recent_traces(&self, limit: usize) -> Vec<CommandTrace> {
        let Ok(traces) = self.traces.lock() else {
            return Vec::new();
        };
        let mut trace_list: Vec<_> = traces.values().cloned().collect();
        trace_list.sort_by(|a, b| b.start_time.cmp(&a.start_time));
        trace_list.into_iter().take(limit).collect()
    }
}

// Usage with execute(). Both store and command are consumed by execute(), so
// capture any logging data before the call.
pub async fn execute_with_tracing<C, S>(
    command: C,
    store: S,
    tracer: &CommandTracer,
) -> Result<ExecutionResponse, CommandError>
where
    C: CommandLogic,
    S: EventStore,
{
    let trace_id = tracer.start_trace(&command);

    // Phase 1: Stream Reading
    tracer.add_phase(
        trace_id,
        "stream_reading",
        HashMap::from([(
            "streams_to_read".to_string(),
            command.stream_declarations().len().to_string(),
        )]),
    );

    let result = execute(store, command, RetryPolicy::new()).await;

    tracer.complete_phase(trace_id);

    let trace_result = match &result {
        Ok(response) => Ok(format!(
            "command executed successfully in {} attempt(s)",
            response.attempts()
        )),
        Err(e) => Err(e.to_string()),
    };

    tracer.complete_trace(trace_id, trace_result);

    result
}
```

### Observing command execution with `MetricsHook`

EventCore exposes a single, first-class hook into command execution for
telemetry: the `MetricsHook` trait combined with
`RetryPolicy::with_metrics_hook`. This is the **only** EventCore-provided
metrics surface — there is no `eventcore::monitoring` module, no metrics
registry, and no exporter. Implement the trait to forward EventCore's retry
events to whatever metrics/telemetry system your application already uses
(`metrics`, OpenTelemetry, Prometheus client, logs, etc.).

```rust
use eventcore::{BackoffStrategy, MetricsHook, RetryContext, RetryPolicy};
use eventcore_types::DelayMilliseconds;

// Application-level metrics hook. Wire it into whatever telemetry you own.
struct TracingMetricsHook;

impl MetricsHook for TracingMetricsHook {
    // Called before each retry attempt. RetryContext exposes the streams being
    // retried, the 1-based attempt number, and the delay before this attempt.
    fn on_retry_attempt(&self, ctx: &RetryContext) {
        tracing::warn!(
            streams = ?ctx.streams,
            attempt = ?ctx.attempt,
            delay_ms = ?ctx.delay_ms,
            "command retried"
        );
    }
}

// Build a policy with exponential backoff and the hook attached.
let policy = RetryPolicy::new()
    .max_retries(5)
    .backoff_strategy(BackoffStrategy::Exponential {
        base_ms: DelayMilliseconds::new(50),
    })
    .with_metrics_hook(TracingMetricsHook);

// Pass `policy` to execute(store, command, policy).
```

> Inspect the exact methods on `MetricsHook` and the fields of `RetryContext`
> with `cargo doc -p eventcore --open` — they evolve with the crate, and the
> hook signature is the authoritative contract.

### Performance Profiler

This profiler is **application-level** scaffolding for timing arbitrary async
operations (including calls to `execute()` or `read_stream`). It uses no
EventCore types; it is included as a reusable diagnostic pattern.

```rust
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct PerformanceProfiler {
    profiles: Arc<Mutex<HashMap<String, PerformanceProfile>>>,
    enabled: bool,
}

#[derive(Debug, Clone)]
pub struct PerformanceProfile {
    pub operation_name: String,
    pub samples: Vec<PerformanceSample>,
    pub statistics: ProfileStatistics,
}

#[derive(Debug, Clone)]
pub struct PerformanceSample {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub duration: Duration,
    pub success: bool,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProfileStatistics {
    pub total_samples: usize,
    pub success_rate: f64,
    pub avg_duration: Duration,
    pub min_duration: Duration,
    pub max_duration: Duration,
    pub p95_duration: Duration,
}

impl PerformanceProfiler {
    pub fn new(enabled: bool) -> Self {
        Self {
            profiles: Arc::new(Mutex::new(HashMap::new())),
            enabled,
        }
    }

    pub async fn profile_operation<F, T>(&self, operation_name: &str, operation: F) -> T
    where
        F: Future<Output = T>,
    {
        if !self.enabled {
            return operation.await;
        }

        let start_time = chrono::Utc::now();
        let start_instant = std::time::Instant::now();

        let result = operation.await;

        let duration = start_instant.elapsed();

        let sample = PerformanceSample {
            timestamp: start_time,
            duration,
            success: true, // Refine per operation if the result encodes success
            metadata: HashMap::new(),
        };

        let mut profiles = self.profiles.lock().await;
        let profile = profiles
            .entry(operation_name.to_string())
            .or_insert_with(|| PerformanceProfile {
                operation_name: operation_name.to_string(),
                samples: Vec::new(),
                statistics: ProfileStatistics::default(),
            });

        profile.samples.push(sample);
        Self::update_statistics(profile);

        // Keep only recent samples (last hour)
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(1);
        profile.samples.retain(|s| s.timestamp > cutoff);

        result
    }

    fn update_statistics(profile: &mut PerformanceProfile) {
        if profile.samples.is_empty() {
            return;
        }

        let mut durations: Vec<_> = profile.samples.iter().map(|s| s.duration).collect();
        durations.sort();

        let success_count = profile.samples.iter().filter(|s| s.success).count();

        profile.statistics = ProfileStatistics {
            total_samples: profile.samples.len(),
            success_rate: success_count as f64 / profile.samples.len() as f64,
            avg_duration: durations.iter().sum::<Duration>() / durations.len() as u32,
            min_duration: durations[0],
            max_duration: durations[durations.len() - 1],
            p95_duration: durations[(durations.len() as f64 * 0.95) as usize],
        };
    }

    pub async fn get_profile_report(&self) -> HashMap<String, ProfileStatistics> {
        let profiles = self.profiles.lock().await;
        profiles
            .iter()
            .map(|(name, profile)| (name.clone(), profile.statistics.clone()))
            .collect()
    }
}
```

### Log Analysis Tools

This log analyzer is **application-level** — it scans your application's own log
output for recurring failure signatures. It deliberately keys off the error
messages EventCore actually produces (concurrency conflicts, store failures,
deserialization failures) so the patterns stay accurate.

```rust
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct LogAnalyzer {
    log_patterns: Vec<LogPattern>,
}

#[derive(Debug, Clone)]
pub struct LogPattern {
    pub name: String,
    pub pattern: String,
    pub severity: LogSeverity,
    pub action: String,
}

#[derive(Debug, Clone)]
pub enum LogSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

impl LogAnalyzer {
    pub fn new() -> Self {
        Self {
            log_patterns: Self::default_patterns(),
        }
    }

    fn default_patterns() -> Vec<LogPattern> {
        vec![
            LogPattern {
                name: "connection_pool_exhaustion".to_string(),
                pattern: r"(?i)connection.*pool.*exhausted|too many connections|acquire.*timeout".to_string(),
                severity: LogSeverity::Critical,
                action: "Increase PostgresConfig.max_connections or check for connection leaks".to_string(),
            },
            LogPattern {
                // Matches CommandError::ConcurrencyError and EventStoreError::VersionConflict.
                name: "concurrency_conflict".to_string(),
                pattern: r"(?i)concurrency conflict|version.*conflict".to_string(),
                severity: LogSeverity::Warning,
                action: "Tune RetryPolicy backoff or reduce contention on hot streams".to_string(),
            },
            LogPattern {
                // Matches EventStoreError::StoreFailure.
                name: "store_failure".to_string(),
                pattern: r"(?i)event store error|store failure".to_string(),
                severity: LogSeverity::Error,
                action: "Check database connectivity and PostgreSQL health".to_string(),
            },
            LogPattern {
                // Matches EventStoreError::Serialization/DeserializationFailed.
                name: "serialization_failure".to_string(),
                pattern: r"(?i)serialization failed|deserialization failed".to_string(),
                severity: LogSeverity::Error,
                action: "Review event schema evolution: add #[serde(default)] or a new event variant (ADR-0035)".to_string(),
            },
            LogPattern {
                name: "memory_pressure".to_string(),
                pattern: r"(?i)out of memory|memory.*limit|allocation.*failed".to_string(),
                severity: LogSeverity::Critical,
                action: "Scale up memory or check for memory leaks".to_string(),
            },
        ]
    }

    pub fn analyze_logs(&self, log_entries: &[LogEntry]) -> LogAnalysisReport {
        let mut report = LogAnalysisReport::default();

        for entry in log_entries {
            for pattern in &self.log_patterns {
                if Self::matches_pattern(&entry.message, &pattern.pattern) {
                    let issue = LogIssue {
                        pattern_name: pattern.name.clone(),
                        severity: pattern.severity.clone(),
                        message: entry.message.clone(),
                        timestamp: entry.timestamp,
                        action: pattern.action.clone(),
                        occurrences: 1,
                    };

                    if let Some(existing) = report
                        .issues
                        .iter_mut()
                        .find(|i| i.pattern_name == issue.pattern_name)
                    {
                        existing.occurrences += 1;
                        if entry.timestamp > existing.timestamp {
                            existing.timestamp = entry.timestamp;
                            existing.message = entry.message.clone();
                        }
                    } else {
                        report.issues.push(issue);
                    }
                }
            }
        }

        report.issues.sort_by(|a, b| match (&a.severity, &b.severity) {
            (LogSeverity::Critical, LogSeverity::Critical) => b.occurrences.cmp(&a.occurrences),
            (LogSeverity::Critical, _) => std::cmp::Ordering::Less,
            (_, LogSeverity::Critical) => std::cmp::Ordering::Greater,
            (LogSeverity::Error, LogSeverity::Error) => b.occurrences.cmp(&a.occurrences),
            (LogSeverity::Error, _) => std::cmp::Ordering::Less,
            (_, LogSeverity::Error) => std::cmp::Ordering::Greater,
            _ => b.occurrences.cmp(&a.occurrences),
        });

        report
    }

    fn matches_pattern(message: &str, pattern: &str) -> bool {
        regex::Regex::new(pattern)
            .map(|re| re.is_match(message))
            .unwrap_or(false)
    }
}

#[derive(Debug, Default)]
pub struct LogAnalysisReport {
    pub issues: Vec<LogIssue>,
}

#[derive(Debug)]
pub struct LogIssue {
    pub pattern_name: String,
    pub severity: LogSeverity,
    pub message: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub action: String,
    pub occurrences: u32,
}

#[derive(Debug)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: String,
    pub message: String,
    pub metadata: HashMap<String, String>,
}
```

## Troubleshooting Runbooks

These runbooks describe **application- and infrastructure-level** procedures.
EventCore ships no CLI binary and no metrics endpoint, so the `curl`/`kubectl`
steps below assume your application exposes its own health and metrics
endpoints and runs under your own orchestration. Adjust them to your deployment.

**Runbook 1: High Command Latency**

1. **Check connection pool / database health** — query PostgreSQL directly:

   ```sql
   SELECT query, mean_exec_time, calls
   FROM pg_stat_statements
   ORDER BY mean_exec_time DESC
   LIMIT 10;
   ```

2. **Check for lock contention**

   ```sql
   SELECT * FROM pg_locks WHERE NOT granted;
   ```

3. **Tune the store and retry policy in your application code** — raise
   `PostgresConfig.max_connections`, or adjust `RetryPolicy` backoff for hot
   streams.

4. **Scale your application's compute** (your orchestration, e.g.):

   ```bash
   kubectl scale deployment my-eventcore-app --replicas=6
   ```

**Runbook 2: Projection Lag**

1. **Check projection checkpoints** via your application's health endpoint or
   the application-level `ProjectionLagMonitor` shown above (reads
   `CheckpointStore::load`).

2. **Confirm the projector is running in the intended mode** — continuous mode
   (`ProjectionConfig::default().continuous()`) keeps polling; batch mode (the
   default) returns after draining available events.

3. **Restart projection processing** (your orchestration):

   ```bash
   kubectl delete pod -l app=my-eventcore-projections
   ```

4. **Rebuild if the read model is corrupted** — reset the projector's checkpoint
   and read-model storage, then replay with `run_projection(..., ProjectionConfig::default())`
   in batch mode (see [Projection rebuild](#issue-projection-lag) above). There
   is no EventCore CLI; the rebuild is invoked from your own application code or
   an admin task you write.

**Runbook 3: Memory Issues**

1. **Check memory usage** (your orchestration):

   ```bash
   kubectl top pods -l app=my-eventcore-app
   ```

2. **Reduce in-flight working set** — projection batch size and poll cadence are
   controlled through `ProjectionConfig`; large `collect_events` materializations
   (used in tests/ad-hoc inspection) load whole streams into memory, so prefer
   incremental processing for large streams.

3. **Scale up memory limits** (your orchestration):

   ```yaml
   resources:
     limits:
       memory: "1Gi"
   ```

## Best Practices

1. **Comprehensive monitoring** - Monitor all system components from your own
   application telemetry; wire EventCore retries in through `MetricsHook`.
2. **Automated diagnostics** - Use application-level tools to detect issues early.
3. **Detailed logging** - Include context and correlation IDs.
4. **Performance profiling** - Regular performance analysis.
5. **Runbook maintenance** - Keep troubleshooting guides updated.
6. **Incident response** - Defined escalation procedures.
7. **Root cause analysis** - Learn from every incident.
8. **Preventive measures** - Address issues before they become problems.

## Summary

EventCore troubleshooting:

- ✅ **Systematic diagnosis** - Structured approach to problem identification
- ✅ **Real API surface** - Diagnostics built on `execute()`, `run_projection()`,
  `read_stream`/`append_events`, and the `MetricsHook` trait
- ✅ **Application-owned telemetry** - You own metrics, logging, and orchestration;
  EventCore provides the execution hook
- ✅ **Schema evolution by design** - Decode failures are resolved with serde
  defaults and new event variants (ADR-0035), not data repair

Key components:

1. Build application-level monitoring on top of the real EventCore APIs to detect
   issues early.
2. Implement systematic debugging approaches for complex problems.
3. Maintain detailed logs with proper correlation and context.
4. Use application-level tools for log analysis and pattern detection.
5. Document and automate common troubleshooting procedures.

Next, let's explore [Production Checklist](./05-production-checklist.md) →
