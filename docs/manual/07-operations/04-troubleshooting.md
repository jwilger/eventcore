# Chapter 6.4: Troubleshooting

This chapter provides comprehensive troubleshooting guidance for EventCore applications in production. From common issues to advanced debugging techniques, you'll learn to diagnose and resolve problems quickly.

## Common Issues and Solutions

### Command Execution Failures

#### Issue: Commands timing out

**Symptoms:**

- Commands taking longer than expected
- Timeout errors in logs
- Degraded system performance

**Debugging steps:**

```rust
// Enable detailed command tracing
#[tracing::instrument(skip(command, executor), level = "debug")]
async fn debug_command_execution<C: Command>(
    command: &C,
    executor: &CommandExecutor,
) -> CommandResult<ExecutionResult> {
    let start = std::time::Instant::now();

    tracing::debug!(
        command_type = std::any::type_name::<C>(),
        "Starting command execution"
    );

    // Check stream access patterns
    let read_streams = command.read_streams(&command);
    tracing::debug!(
        stream_count = read_streams.len(),
        streams = ?read_streams,
        "Command will read from streams"
    );

    // Time each phase
    let read_start = std::time::Instant::now();
    let result = executor.execute(command).await;
    let total_duration = start.elapsed();

    match &result {
        Ok(execution_result) => {
            tracing::info!(
                total_duration_ms = total_duration.as_millis(),
                events_written = execution_result.events_written.len(),
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

   ```rust
   // Check connection pool metrics
   async fn diagnose_connection_pool(pool: &sqlx::PgPool) {
       let pool_options = pool.options();
       let pool_size = pool.size();
       let idle_connections = pool.num_idle();

       tracing::info!(
           max_connections = pool_options.get_max_connections(),
           current_size = pool_size,
           idle_connections = idle_connections,
           active_connections = pool_size - idle_connections,
           "Connection pool status"
       );

       // Alert if pool utilization is high
       let utilization = (pool_size as f64) / (pool_options.get_max_connections() as f64);
       if utilization > 0.8 {
           tracing::warn!(
               utilization_percent = utilization * 100.0,
               "High connection pool utilization"
           );
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

3. **Lock contention on streams**

   ```rust
   // Implement lock timeout and retry
   async fn execute_with_lock_retry<C: Command>(
       command: &C,
       executor: &CommandExecutor,
       max_retries: u32,
   ) -> CommandResult<ExecutionResult> {
       let mut retry_count = 0;

       loop {
           match executor.execute(command).await {
               Ok(result) => return Ok(result),
               Err(CommandError::ConcurrencyConflict(streams)) => {
                   retry_count += 1;
                   if retry_count >= max_retries {
                       return Err(CommandError::ConcurrencyConflict(streams));
                   }

                   // Exponential backoff
                   let delay = Duration::from_millis(100 * 2_u64.pow(retry_count - 1));
                   tokio::time::sleep(delay).await;

                   tracing::warn!(
                       retry_attempt = retry_count,
                       delay_ms = delay.as_millis(),
                       conflicting_streams = ?streams,
                       "Retrying command due to concurrency conflict"
                   );
               }
               Err(other_error) => return Err(other_error),
           }
       }
   }
   ```

#### Issue: Command validation failures

**Symptoms:**

- Validation errors in command processing
- Business rule violations
- Data consistency issues

**Debugging approach:**

```rust
// Enhanced validation with detailed error reporting
#[derive(Debug, thiserror::Error)]
pub enum DetailedValidationError {
    #[error("Field validation failed: {field} - {reason}")]
    FieldValidation { field: String, reason: String },

    #[error("Business rule violation: {rule} - {context}")]
    BusinessRule { rule: String, context: String },

    #[error("State precondition failed: expected {expected}, found {actual}")]
    StatePrecondition { expected: String, actual: String },

    #[error("Reference validation failed: {reference_type} {reference_id} not found")]
    ReferenceNotFound { reference_type: String, reference_id: String },
}

// Validation with detailed context
pub fn validate_transfer_command(
    command: &TransferMoney,
    state: &AccountState,
) -> Result<(), DetailedValidationError> {
    // Check amount
    if command.amount <= Money::zero() {
        return Err(DetailedValidationError::FieldValidation {
            field: "amount".to_string(),
            reason: format!("Amount must be positive, got {}", command.amount),
        });
    }

    // Check account state
    if !state.is_active {
        return Err(DetailedValidationError::StatePrecondition {
            expected: "active account".to_string(),
            actual: "inactive account".to_string(),
        });
    }

    // Check sufficient balance
    if state.balance < command.amount {
        return Err(DetailedValidationError::BusinessRule {
            rule: "sufficient_balance".to_string(),
            context: format!(
                "Balance {} insufficient for transfer {}",
                state.balance, command.amount
            ),
        });
    }

    Ok(())
}
```

### Event Store Issues

#### Issue: High event store latency

**Diagnosis tools:**

```rust
// Event store performance monitor
#[derive(Debug, Clone)]
pub struct EventStoreMonitor {
    latency_tracker: Arc<Mutex<LatencyTracker>>,
}

impl EventStoreMonitor {
    pub async fn monitor_operation<F, T>(&self, operation_name: &str, operation: F) -> Result<T, EventStoreError>
    where
        F: Future<Output = Result<T, EventStoreError>>,
    {
        let start = std::time::Instant::now();
        let result = operation.await;
        let duration = start.elapsed();

        // Record latency
        {
            let mut tracker = self.latency_tracker.lock().await;
            tracker.record_operation(operation_name, duration, result.is_ok());
        }

        // Alert on high latency
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
    operations: HashMap<String, Vec<OperationMetric>>,
}

#[derive(Debug, Clone)]
struct OperationMetric {
    duration: Duration,
    success: bool,
    timestamp: DateTime<Utc>,
}

impl LatencyTracker {
    pub fn record_operation(&mut self, operation: &str, duration: Duration, success: bool) {
        let metric = OperationMetric {
            duration,
            success,
            timestamp: Utc::now(),
        };

        self.operations
            .entry(operation.to_string())
            .or_insert_with(Vec::new)
            .push(metric);

        // Keep only recent metrics (last hour)
        let cutoff = Utc::now() - chrono::Duration::hours(1);
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
            let success_rate = metrics.iter().filter(|m| m.success).count() as f64 / metrics.len() as f64;

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

#### Issue: Event store corruption

**Detection and recovery:**

```rust
// Corruption detection
pub struct CorruptionDetector {
    event_store: Arc<dyn EventStore>,
}

impl CorruptionDetector {
    pub async fn scan_for_corruption(&self) -> Result<CorruptionReport, ScanError> {
        let mut report = CorruptionReport::default();

        // Scan all streams
        let all_streams = self.event_store.list_all_streams().await?;

        for stream_id in all_streams {
            match self.scan_stream(&stream_id).await {
                Ok(stream_report) => {
                    if stream_report.has_issues() {
                        report.corrupted_streams.push(stream_report);
                    }
                }
                Err(e) => {
                    tracing::error!(
                        stream_id = %stream_id,
                        error = %e,
                        "Failed to scan stream for corruption"
                    );
                    report.scan_errors.push(ScanError::StreamScanFailed {
                        stream_id: stream_id.clone(),
                        error: e.to_string(),
                    });
                }
            }
        }

        report.scan_completed_at = Utc::now();
        Ok(report)
    }

    async fn scan_stream(&self, stream_id: &StreamId) -> Result<StreamCorruptionReport, ScanError> {
        let mut report = StreamCorruptionReport {
            stream_id: stream_id.clone(),
            issues: Vec::new(),
        };

        let events = self.event_store.read_stream(stream_id, ReadOptions::default()).await?;

        // Check version sequence
        for (i, event) in events.events.iter().enumerate() {
            let expected_version = EventVersion::from(i as u64 + 1);
            if event.version != expected_version {
                report.issues.push(CorruptionIssue::VersionGap {
                    event_id: event.id,
                    expected_version,
                    actual_version: event.version,
                });
            }

            // Check event structure
            if let Err(e) = self.validate_event_structure(event) {
                report.issues.push(CorruptionIssue::StructuralError {
                    event_id: event.id,
                    error: e,
                });
            }
        }

        Ok(report)
    }

    fn validate_event_structure(&self, event: &StoredEvent) -> Result<(), String> {
        // Check UUID format
        if event.id.is_nil() {
            return Err("Nil event ID".to_string());
        }

        // Check payload can be deserialized
        match serde_json::from_value::<serde_json::Value>(event.payload.clone()) {
            Ok(_) => {}
            Err(e) => return Err(format!("Invalid payload JSON: {}", e)),
        }

        // Check timestamp is reasonable
        let now = Utc::now();
        if event.occurred_at > now + chrono::Duration::minutes(5) {
            return Err("Event timestamp is in the future".to_string());
        }

        if event.occurred_at < (now - chrono::Duration::days(10 * 365)) {
            return Err("Event timestamp is too old".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct CorruptionReport {
    pub corrupted_streams: Vec<StreamCorruptionReport>,
    pub scan_errors: Vec<ScanError>,
    pub scan_completed_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct StreamCorruptionReport {
    pub stream_id: StreamId,
    pub issues: Vec<CorruptionIssue>,
}

impl StreamCorruptionReport {
    pub fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }
}

#[derive(Debug)]
pub enum CorruptionIssue {
    VersionGap {
        event_id: EventId,
        expected_version: EventVersion,
        actual_version: EventVersion,
    },
    StructuralError {
        event_id: EventId,
        error: String,
    },
    DuplicateEvent {
        event_id: EventId,
        duplicate_id: EventId,
    },
}
```

### Projection Issues

#### Issue: Projection lag

**Monitoring and diagnosis:**

```rust
// Projection lag monitor
#[derive(Debug, Clone)]
pub struct ProjectionLagMonitor {
    event_store: Arc<dyn EventStore>,
    projection_manager: Arc<ProjectionManager>,
}

impl ProjectionLagMonitor {
    pub async fn check_all_projections(&self) -> Result<Vec<ProjectionLagReport>, MonitorError> {
        let mut reports = Vec::new();

        let projections = self.projection_manager.list_projections().await?;
        let latest_event_time = self.get_latest_event_time().await?;

        for projection_name in projections {
            let report = self.check_projection_lag(&projection_name, latest_event_time).await?;
            reports.push(report);
        }

        Ok(reports)
    }

    async fn check_projection_lag(
        &self,
        projection_name: &str,
        latest_event_time: DateTime<Utc>,
    ) -> Result<ProjectionLagReport, MonitorError> {
        let checkpoint = self.projection_manager
            .get_checkpoint(projection_name)
            .await?;

        let lag = match checkpoint.last_processed_at {
            Some(last_processed) => latest_event_time.signed_duration_since(last_processed),
            None => chrono::Duration::max_value(), // Never processed
        };

        let status = if lag > chrono::Duration::minutes(30) {
            ProjectionStatus::Critical
        } else if lag > chrono::Duration::minutes(5) {
            ProjectionStatus::Warning
        } else {
            ProjectionStatus::Healthy
        };

        Ok(ProjectionLagReport {
            projection_name: projection_name.to_string(),
            lag_duration: lag,
            status,
            last_processed_event: checkpoint.last_event_id,
            last_processed_at: checkpoint.last_processed_at,
            events_processed: checkpoint.events_processed,
        })
    }

    async fn get_latest_event_time(&self) -> Result<DateTime<Utc>, MonitorError> {
        // Get the timestamp of the most recent event across all streams
        self.event_store.get_latest_event_time().await
            .map_err(MonitorError::EventStoreError)
    }
}

#[derive(Debug)]
pub struct ProjectionLagReport {
    pub projection_name: String,
    pub lag_duration: chrono::Duration,
    pub status: ProjectionStatus,
    pub last_processed_event: Option<EventId>,
    pub last_processed_at: Option<DateTime<Utc>>,
    pub events_processed: u64,
}

#[derive(Debug, Clone)]
pub enum ProjectionStatus {
    Healthy,
    Warning,
    Critical,
}
```

**Projection rebuild when corrupted:**

```rust
// Safe projection rebuild
pub struct ProjectionRebuilder {
    event_store: Arc<dyn EventStore>,
    projection_manager: Arc<ProjectionManager>,
}

impl ProjectionRebuilder {
    pub async fn rebuild_projection(
        &self,
        projection_name: &str,
        strategy: RebuildStrategy,
    ) -> Result<RebuildResult, RebuildError> {
        tracing::info!(
            projection_name = projection_name,
            strategy = ?strategy,
            "Starting projection rebuild"
        );

        let start_time = Utc::now();

        // Create backup of current projection state
        let backup_id = self.backup_projection_state(projection_name).await?;

        // Reset projection state
        self.projection_manager.reset_projection(projection_name).await?;

        // Rebuild based on strategy
        let rebuild_result = match strategy {
            RebuildStrategy::Full => {
                self.rebuild_from_beginning(projection_name).await
            }
            RebuildStrategy::FromCheckpoint { checkpoint_time } => {
                self.rebuild_from_checkpoint(projection_name, checkpoint_time).await
            }
            RebuildStrategy::FromEvent { event_id } => {
                self.rebuild_from_event(projection_name, event_id).await
            }
        };

        match rebuild_result {
            Ok(stats) => {
                // Rebuild successful - clean up backup
                self.cleanup_projection_backup(backup_id).await?;

                let duration = Utc::now().signed_duration_since(start_time);

                tracing::info!(
                    projection_name = projection_name,
                    events_processed = stats.events_processed,
                    duration_seconds = duration.num_seconds(),
                    "Projection rebuild completed successfully"
                );

                Ok(RebuildResult {
                    success: true,
                    events_processed: stats.events_processed,
                    duration,
                    backup_id: Some(backup_id),
                })
            }
            Err(e) => {
                // Rebuild failed - restore from backup
                tracing::error!(
                    projection_name = projection_name,
                    error = %e,
                    "Projection rebuild failed, restoring from backup"
                );

                self.restore_projection_from_backup(projection_name, backup_id).await?;

                Err(RebuildError::RebuildFailed {
                    original_error: Box::new(e),
                    backup_restored: true,
                })
            }
        }
    }

    async fn rebuild_from_beginning(&self, projection_name: &str) -> Result<RebuildStats, RebuildError> {
        let mut stats = RebuildStats::default();

        // Get all events in chronological order
        let events = self.event_store.read_all_events_ordered().await?;

        // Process events in batches
        let batch_size = 1000;
        for chunk in events.chunks(batch_size) {
            self.projection_manager
                .process_events_batch(projection_name, chunk)
                .await?;

            stats.events_processed += chunk.len() as u64;

            // Checkpoint every batch
            self.projection_manager
                .save_checkpoint(projection_name)
                .await?;

            // Progress reporting
            if stats.events_processed % 10000 == 0 {
                tracing::info!(
                    projection_name = projection_name,
                    events_processed = stats.events_processed,
                    "Rebuild progress"
                );
            }
        }

        Ok(stats)
    }
}

#[derive(Debug)]
pub enum RebuildStrategy {
    Full,
    FromCheckpoint { checkpoint_time: DateTime<Utc> },
    FromEvent { event_id: EventId },
}

#[derive(Debug, Default)]
pub struct RebuildStats {
    pub events_processed: u64,
}

#[derive(Debug)]
pub struct RebuildResult {
    pub success: bool,
    pub events_processed: u64,
    pub duration: chrono::Duration,
    pub backup_id: Option<Uuid>,
}
```

## Debugging Tools

### Command Execution Tracer

```rust
// Detailed command execution tracer
#[derive(Debug, Clone)]
pub struct CommandTracer {
    traces: Arc<Mutex<HashMap<Uuid, CommandTrace>>>,
}

#[derive(Debug, Clone)]
pub struct CommandTrace {
    pub trace_id: Uuid,
    pub command_type: String,
    pub start_time: DateTime<Utc>,
    pub phases: Vec<TracePhase>,
    pub completed: bool,
    pub result: Option<Result<String, String>>,
}

#[derive(Debug, Clone)]
pub struct TracePhase {
    pub phase_name: String,
    pub start_time: DateTime<Utc>,
    pub duration: Option<Duration>,
    pub details: HashMap<String, String>,
}

impl CommandTracer {
    pub fn start_trace<C: Command>(&self, command: &C) -> Uuid {
        let trace_id = Uuid::new_v4();
        let trace = CommandTrace {
            trace_id,
            command_type: std::any::type_name::<C>().to_string(),
            start_time: Utc::now(),
            phases: Vec::new(),
            completed: false,
            result: None,
        };

        let mut traces = self.traces.lock().unwrap();
        traces.insert(trace_id, trace);

        tracing::info!(
            trace_id = %trace_id,
            command_type = std::any::type_name::<C>(),
            "Started command trace"
        );

        trace_id
    }

    pub fn add_phase(&self, trace_id: Uuid, phase_name: &str, details: HashMap<String, String>) {
        let mut traces = self.traces.lock().unwrap();
        if let Some(trace) = traces.get_mut(&trace_id) {
            trace.phases.push(TracePhase {
                phase_name: phase_name.to_string(),
                start_time: Utc::now(),
                duration: None,
                details,
            });
        }
    }

    pub fn complete_phase(&self, trace_id: Uuid) {
        let mut traces = self.traces.lock().unwrap();
        if let Some(trace) = traces.get_mut(&trace_id) {
            if let Some(last_phase) = trace.phases.last_mut() {
                last_phase.duration = Some(
                    Utc::now().signed_duration_since(last_phase.start_time).to_std().unwrap_or_default()
                );
            }
        }
    }

    pub fn complete_trace(&self, trace_id: Uuid, result: Result<String, String>) {
        let mut traces = self.traces.lock().unwrap();
        if let Some(trace) = traces.get_mut(&trace_id) {
            trace.completed = true;
            trace.result = Some(result);

            let total_duration = Utc::now().signed_duration_since(trace.start_time);

            tracing::info!(
                trace_id = %trace_id,
                duration_ms = total_duration.num_milliseconds(),
                phases = trace.phases.len(),
                success = trace.result.as_ref().unwrap().is_ok(),
                "Completed command trace"
            );
        }
    }

    pub fn get_trace(&self, trace_id: Uuid) -> Option<CommandTrace> {
        let traces = self.traces.lock().unwrap();
        traces.get(&trace_id).cloned()
    }

    pub fn get_recent_traces(&self, limit: usize) -> Vec<CommandTrace> {
        let traces = self.traces.lock().unwrap();
        let mut trace_list: Vec<_> = traces.values().cloned().collect();
        trace_list.sort_by(|a, b| b.start_time.cmp(&a.start_time));
        trace_list.into_iter().take(limit).collect()
    }
}

// Usage in command executor
pub async fn execute_with_tracing<C: Command>(
    command: &C,
    executor: &CommandExecutor,
    tracer: &CommandTracer,
) -> CommandResult<ExecutionResult> {
    let trace_id = tracer.start_trace(command);

    // Phase 1: Stream Reading
    tracer.add_phase(trace_id, "stream_reading", hashmap! {
        "streams_to_read".to_string() => command.read_streams(command).len().to_string(),
    });

    let result = executor.execute(command).await;

    tracer.complete_phase(trace_id);

    // Complete trace
    let trace_result = match &result {
        Ok(execution_result) => Ok(format!(
            "Events written: {}, Streams affected: {}",
            execution_result.events_written.len(),
            execution_result.affected_streams.len()
        )),
        Err(e) => Err(e.to_string()),
    };

    tracer.complete_trace(trace_id, trace_result);

    result
}
```

### Performance Profiler

```rust
// Built-in performance profiler
#[derive(Debug, Clone)]
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
    pub timestamp: DateTime<Utc>,
    pub duration: Duration,
    pub memory_before: usize,
    pub memory_after: usize,
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
    pub avg_memory_delta: i64,
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

        let memory_before = self.get_current_memory_usage();
        let start_time = Utc::now();
        let start_instant = std::time::Instant::now();

        let result = operation.await;

        let duration = start_instant.elapsed();
        let memory_after = self.get_current_memory_usage();

        let sample = PerformanceSample {
            timestamp: start_time,
            duration,
            memory_before,
            memory_after,
            success: true, // Would need to be determined by operation type
            metadata: HashMap::new(),
        };

        // Record sample
        let mut profiles = self.profiles.lock().await;
        let profile = profiles.entry(operation_name.to_string()).or_insert_with(|| {
            PerformanceProfile {
                operation_name: operation_name.to_string(),
                samples: Vec::new(),
                statistics: ProfileStatistics::default(),
            }
        });

        profile.samples.push(sample);

        // Update statistics
        self.update_statistics(profile);

        // Keep only recent samples (last hour)
        let cutoff = Utc::now() - chrono::Duration::hours(1);
        profile.samples.retain(|s| s.timestamp > cutoff);

        result
    }

    fn update_statistics(&self, profile: &mut PerformanceProfile) {
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
            avg_memory_delta: profile.samples.iter()
                .map(|s| s.memory_after as i64 - s.memory_before as i64)
                .sum::<i64>() / profile.samples.len() as i64,
        };
    }

    fn get_current_memory_usage(&self) -> usize {
        // Platform-specific memory usage detection
        // This is a simplified implementation
        0
    }

    pub async fn get_profile_report(&self) -> HashMap<String, ProfileStatistics> {
        let profiles = self.profiles.lock().await;
        profiles.iter()
            .map(|(name, profile)| (name.clone(), profile.statistics.clone()))
            .collect()
    }
}
```

### Log Analysis Tools

```rust
// Automated log analysis for common issues
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
                pattern: r"(?i)connection.*pool.*exhausted|too many connections".to_string(),
                severity: LogSeverity::Critical,
                action: "Scale up connection pool or check for connection leaks".to_string(),
            },
            LogPattern {
                name: "command_timeout".to_string(),
                pattern: r"(?i)command.*timeout|execution.*timeout".to_string(),
                severity: LogSeverity::Error,
                action: "Check database performance and query optimization".to_string(),
            },
            LogPattern {
                name: "concurrency_conflict".to_string(),
                pattern: r"(?i)concurrency.*conflict|version.*conflict".to_string(),
                severity: LogSeverity::Warning,
                action: "Consider optimizing command patterns or retry strategies".to_string(),
            },
            LogPattern {
                name: "memory_pressure".to_string(),
                pattern: r"(?i)out of memory|memory.*limit|allocation.*failed".to_string(),
                severity: LogSeverity::Critical,
                action: "Scale up memory or check for memory leaks".to_string(),
            },
            LogPattern {
                name: "projection_lag".to_string(),
                pattern: r"(?i)projection.*lag|projection.*behind".to_string(),
                severity: LogSeverity::Warning,
                action: "Check projection performance and consider scaling".to_string(),
            },
        ]
    }

    pub async fn analyze_logs(&self, log_entries: &[LogEntry]) -> LogAnalysisReport {
        let mut report = LogAnalysisReport::default();

        for entry in log_entries {
            for pattern in &self.log_patterns {
                if self.matches_pattern(&entry.message, &pattern.pattern) {
                    let issue = LogIssue {
                        pattern_name: pattern.name.clone(),
                        severity: pattern.severity.clone(),
                        message: entry.message.clone(),
                        timestamp: entry.timestamp,
                        action: pattern.action.clone(),
                        occurrences: 1,
                    };

                    // Aggregate similar issues
                    if let Some(existing) = report.issues.iter_mut()
                        .find(|i| i.pattern_name == issue.pattern_name) {
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

        // Sort by severity and occurrence count
        report.issues.sort_by(|a, b| {
            match (&a.severity, &b.severity) {
                (LogSeverity::Critical, LogSeverity::Critical) => b.occurrences.cmp(&a.occurrences),
                (LogSeverity::Critical, _) => std::cmp::Ordering::Less,
                (_, LogSeverity::Critical) => std::cmp::Ordering::Greater,
                (LogSeverity::Error, LogSeverity::Error) => b.occurrences.cmp(&a.occurrences),
                (LogSeverity::Error, _) => std::cmp::Ordering::Less,
                (_, LogSeverity::Error) => std::cmp::Ordering::Greater,
                _ => b.occurrences.cmp(&a.occurrences),
            }
        });

        report
    }

    fn matches_pattern(&self, message: &str, pattern: &str) -> bool {
        use regex::Regex;
        if let Ok(regex) = Regex::new(pattern) {
            regex.is_match(message)
        } else {
            false
        }
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
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub occurrences: u32,
}

#[derive(Debug)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub message: String,
    pub metadata: HashMap<String, String>,
}
```

## Troubleshooting Runbooks

### Common Runbooks

**Runbook 1: High Command Latency**

1. **Check connection pool status**

   ```bash
   curl http://localhost:9090/metrics | grep eventcore_connection_pool
   ```

2. **Analyze slow queries**

   ```sql
   SELECT query, mean_time, calls
   FROM pg_stat_statements
   ORDER BY mean_time DESC
   LIMIT 10;
   ```

3. **Check for lock contention**

   ```sql
   SELECT * FROM pg_locks WHERE NOT granted;
   ```

4. **Scale resources if needed**
   ```bash
   kubectl scale deployment eventcore-app --replicas=6
   ```

**Runbook 2: Projection Lag**

1. **Check projection status**

   ```bash
   curl http://localhost:8080/health/projections
   ```

2. **Identify lagging projections**

   ```bash
   curl http://localhost:9090/metrics | grep projection_lag
   ```

3. **Restart projection processing**

   ```bash
   kubectl delete pod -l app=eventcore-projections
   ```

4. **Consider projection rebuild if corruption detected**
   ```bash
   kubectl exec -it eventcore-app -- eventcore-cli projection rebuild user-summary
   ```

**Runbook 3: Memory Issues**

1. **Check memory usage**

   ```bash
   kubectl top pods -l app=eventcore
   ```

2. **Analyze memory patterns**

   ```bash
   curl http://localhost:9090/metrics | grep memory_usage
   ```

3. **Generate heap dump if needed**

   ```bash
   kubectl exec -it eventcore-app -- kill -USR1 1
   ```

4. **Scale up memory limits**
   ```yaml
   resources:
     limits:
       memory: "1Gi"
   ```

## Best Practices

1. **Comprehensive monitoring** - Monitor all system components
2. **Automated diagnostics** - Use tools to detect issues early
3. **Detailed logging** - Include context and correlation IDs
4. **Performance profiling** - Regular performance analysis
5. **Runbook maintenance** - Keep troubleshooting guides updated
6. **Incident response** - Defined escalation procedures
7. **Root cause analysis** - Learn from every incident
8. **Preventive measures** - Address issues before they become problems

## Summary

EventCore troubleshooting:

- ✅ **Systematic diagnosis** - Structured approach to problem identification
- ✅ **Comprehensive tools** - Built-in debugging and monitoring tools
- ✅ **Automated analysis** - Log analysis and pattern detection
- ✅ **Performance profiling** - Detailed performance insights
- ✅ **Runbook automation** - Standardized troubleshooting procedures

Key components:

1. Use comprehensive monitoring to detect issues early
2. Implement systematic debugging approaches for complex problems
3. Maintain detailed logs with proper correlation and context
4. Use automated tools for log analysis and pattern detection
5. Document and automate common troubleshooting procedures

Next, let's explore [Production Checklist](./05-production-checklist.md) →
