# Performance Monitoring and Alerting Guide

This guide covers comprehensive performance monitoring, alerting strategies, and observability best practices for EventCore applications.

## Overview

EventCore provides built-in observability through structured metrics, distributed tracing, and structured logging. This guide explains how to effectively monitor your event-sourced applications and set up alerting for critical issues.

## Core Metrics

### Command Execution Metrics

Monitor command performance and reliability:

```rust
use eventcore::monitoring::{MetricsRegistry, loggers};

let metrics = MetricsRegistry::new();
let logger = loggers::command_executor();

// The metrics registry automatically tracks:
// - commands_executed: Total commands processed
// - commands_succeeded: Successful command executions
// - commands_failed: Failed command executions
// - command_duration: Execution time distribution
// - concurrent_commands: Active command count
// - commands_by_type: Breakdown by command type
// - errors_by_type: Error classification
```

#### Key Performance Indicators (KPIs)

1. **Command Success Rate**: `commands_succeeded / commands_executed * 100`
   - Target: > 99.5%
   - Alert threshold: < 95%

2. **Average Command Duration**: P50 of `command_duration`
   - Target: < 50ms for simple commands, < 200ms for complex commands
   - Alert threshold: > 1000ms

3. **P95 Command Duration**: P95 of `command_duration`
   - Target: < 200ms for simple commands, < 500ms for complex commands
   - Alert threshold: > 2000ms

4. **Command Error Rate**: `commands_failed / commands_executed * 100`
   - Target: < 0.5%
   - Alert threshold: > 5%

### Event Store Metrics

Monitor event store performance and health:

```rust
// Event store metrics include:
// - reads_total: Total read operations
// - writes_total: Total write operations
// - events_written: Total events persisted
// - events_read: Total events retrieved
// - read_duration: Read operation times
// - write_duration: Write operation times
// - concurrent_operations: Active operations
// - stream_count: Number of streams
// - operations_by_stream: Per-stream activity
```

#### Key Performance Indicators

1. **Event Store Throughput**:
   - Write throughput: `events_written / time_period`
   - Read throughput: `events_read / time_period`
   - Target: > 1000 events/second write, > 10000 events/second read
   - Alert threshold: < 100 events/second sustained

2. **Event Store Latency**:
   - Write P95: < 10ms
   - Read P95: < 5ms
   - Alert threshold: > 100ms sustained

3. **Connection Pool Utilization**: `(pool_size - available) / pool_size * 100`
   - Target: < 80%
   - Alert threshold: > 95%

### Projection Metrics

Monitor projection lag and processing health:

```rust
// Projection metrics include:
// - events_processed: Total events processed by projections
// - events_skipped: Events skipped (no handler)
// - projection_errors: Processing errors
// - processing_duration: Time to process events
// - lag_by_projection: Current lag per projection
// - last_processed_event: Latest event processed
// - checkpoint_updates: Checkpoint persistence
// - active_projections: Number of running projections
```

#### Key Performance Indicators

1. **Projection Lag**: Time between event creation and projection processing
   - Target: < 1 second for real-time projections
   - Alert threshold: > 60 seconds

2. **Projection Processing Rate**: `events_processed / time_period`
   - Target: Should match event creation rate
   - Alert threshold: Processing rate < 50% of creation rate

3. **Projection Error Rate**: `projection_errors / events_processed * 100`
   - Target: < 0.1%
   - Alert threshold: > 1%

### System Metrics

Monitor overall system health:

```rust
use eventcore::monitoring::SystemMetrics;

let system_metrics = SystemMetrics::new();

// Track system performance
system_metrics.record_memory_usage(memory_bytes);
system_metrics.record_cpu_usage(cpu_percent);
system_metrics.update_connection_pool(total_connections, available_connections);
system_metrics.record_gc_collection(gc_pause_duration);
```

#### Key Performance Indicators

1. **Memory Usage**: RSS memory consumption
   - Target: < 80% of available memory
   - Alert threshold: > 90%

2. **CPU Usage**: Sustained CPU utilization
   - Target: < 70%
   - Alert threshold: > 85% for 5+ minutes

3. **Garbage Collection**: GC pause times
   - Target: < 10ms average pause
   - Alert threshold: > 100ms pause time

## Alerting Strategy

### Critical Alerts (Immediate Response Required)

1. **Service Down**: EventCore service is not responding
   - Trigger: Health check failures for > 1 minute
   - Response: Immediate investigation

2. **Command Failure Spike**: Sudden increase in command failures
   - Trigger: Error rate > 10% for > 2 minutes
   - Response: Check logs for error patterns

3. **Event Store Unavailable**: Cannot read/write events
   - Trigger: Event store operations failing for > 30 seconds
   - Response: Check database connectivity and health

4. **Memory Leak**: Continuous memory growth
   - Trigger: Memory usage increasing > 10% per hour
   - Response: Investigate memory allocations

### Warning Alerts (Response Within Hours)

1. **High Latency**: Operations taking longer than expected
   - Trigger: P95 latency > 2x normal for > 5 minutes
   - Response: Performance analysis

2. **Projection Lag**: Projections falling behind
   - Trigger: Lag > 5 minutes for any projection
   - Response: Check projection processing performance

3. **Resource Exhaustion**: System resources running low
   - Trigger: CPU > 80% or Memory > 85% for > 10 minutes
   - Response: Capacity planning

4. **Error Rate Increase**: Higher than normal error rates
   - Trigger: Error rate > 2x baseline for > 10 minutes
   - Response: Log analysis and investigation

### Information Alerts (Response Within Days)

1. **Performance Degradation**: Gradual performance decrease
   - Trigger: Average latency 50% higher than baseline
   - Response: Performance optimization

2. **Capacity Planning**: Resource usage trends
   - Trigger: Usage growth rate indicates capacity issues in 30 days
   - Response: Capacity planning

## Monitoring Setup

### Prometheus Integration

Export metrics to Prometheus for monitoring:

```rust
use eventcore::monitoring::MetricsRegistry;
use prometheus::{Encoder, TextEncoder, Registry};

pub struct PrometheusExporter {
    registry: prometheus::Registry,
    metrics: MetricsRegistry,
}

impl PrometheusExporter {
    pub fn new() -> Self {
        let registry = Registry::new();
        let metrics = MetricsRegistry::new();

        // Register EventCore metrics with Prometheus
        // (Implementation would register gauge, counter, and histogram metrics)

        Self { registry, metrics }
    }

    pub fn export_metrics(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}
```

### Grafana Dashboards

Create comprehensive dashboards for visual monitoring:

#### Command Execution Dashboard

```json
{
  "dashboard": {
    "title": "EventCore - Command Execution",
    "panels": [
      {
        "title": "Command Success Rate",
        "type": "stat",
        "targets": [
          {
            "expr": "rate(eventcore_commands_succeeded[5m]) / rate(eventcore_commands_executed[5m]) * 100"
          }
        ]
      },
      {
        "title": "Command Duration P95",
        "type": "graph",
        "targets": [
          {
            "expr": "histogram_quantile(0.95, rate(eventcore_command_duration_bucket[5m]))"
          }
        ]
      },
      {
        "title": "Commands by Type",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(eventcore_commands_by_type[5m])"
          }
        ]
      }
    ]
  }
}
```

#### Event Store Dashboard

```json
{
  "dashboard": {
    "title": "EventCore - Event Store",
    "panels": [
      {
        "title": "Event Store Throughput",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(eventcore_events_written[5m])"
          },
          {
            "expr": "rate(eventcore_events_read[5m])"
          }
        ]
      },
      {
        "title": "Event Store Latency",
        "type": "graph",
        "targets": [
          {
            "expr": "histogram_quantile(0.95, rate(eventcore_read_duration_bucket[5m]))"
          },
          {
            "expr": "histogram_quantile(0.95, rate(eventcore_write_duration_bucket[5m]))"
          }
        ]
      }
    ]
  }
}
```

### Tracing Integration

Integrate with distributed tracing systems:

```rust
use eventcore::monitoring::tracing::{CommandTracer, TraceContext};
use opentelemetry::{global, trace::Tracer};

// Configure OpenTelemetry
global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());
let tracer = opentelemetry_jaeger::new_pipeline()
    .with_service_name("eventcore-app")
    .install_simple()
    .unwrap();

// Use with EventCore
let mut command_tracer = CommandTracer::new("CreateUser", stream_ids);
let span = command_tracer.context().create_span();
let _guard = span.entered();

// Your command execution...
command_tracer.record_completion(true, 1);
```

## Performance Optimization

### Command Optimization

1. **Reduce Stream Access**: Minimize streams read per command

   ```rust
   // Good: Read only required streams
   fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
       vec![input.account_stream_id().clone()]
   }

   // Bad: Read unnecessary streams
   fn read_streams(&self, input: &Self::Input) -> Vec<StreamId> {
       vec![
           input.account_stream_id().clone(),
           input.audit_stream_id().clone(), // Only needed for audit commands
           input.notification_stream_id().clone(), // Only needed for notifications
       ]
   }
   ```

2. **Optimize Event Size**: Keep events focused and minimal

   ```rust
   // Good: Focused event
   #[derive(Serialize, Deserialize)]
   pub struct AccountDebited {
       pub account_id: AccountId,
       pub amount: Money,
       pub transaction_id: TransactionId,
   }

   // Bad: Bloated event
   #[derive(Serialize, Deserialize)]
   pub struct AccountDebited {
       pub account_id: AccountId,
       pub amount: Money,
       pub transaction_id: TransactionId,
       pub account_holder: FullAccountDetails, // Too much data
       pub audit_trail: Vec<AuditEntry>,       // Not needed in event
   }
   ```

3. **Batch Related Operations**: Group related commands when possible
   ```rust
   // Use batch APIs for multiple operations
   let events = vec![
       EventToWrite::new(stream1, event1),
       EventToWrite::new(stream2, event2),
   ];
   event_store.write_events_multi(events).await?;
   ```

### Event Store Optimization

1. **Connection Pooling**: Optimize database connections

   ```rust
   use sqlx::postgres::PgPoolOptions;

   let pool = PgPoolOptions::new()
       .max_connections(20)
       .min_connections(5)
       .acquire_timeout(Duration::from_secs(3))
       .idle_timeout(Duration::from_secs(600))
       .max_lifetime(Duration::from_secs(1800))
       .connect("postgresql://...").await?;
   ```

2. **Query Optimization**: Use efficient SQL queries

   ```sql
   -- Good: Use indexes effectively
   SELECT event_data, version
   FROM events
   WHERE stream_id = $1 AND version > $2
   ORDER BY version ASC
   LIMIT 1000;

   -- Bad: Full table scan
   SELECT event_data, version
   FROM events
   WHERE event_data LIKE '%account_id%'
   ORDER BY created_at DESC;
   ```

3. **Batch Writes**: Write multiple events atomically

   ```rust
   // Batch multiple events in single transaction
   async fn write_events_batch(&self, events: Vec<EventToWrite>) -> Result<()> {
       let mut tx = self.pool.begin().await?;

       for event in events {
           sqlx::query!(
               "INSERT INTO events (stream_id, event_data, version) VALUES ($1, $2, $3)",
               event.stream_id.as_ref(),
               event.data,
               event.version as i64
           )
           .execute(&mut tx)
           .await?;
       }

       tx.commit().await?;
       Ok(())
   }
   ```

### Projection Optimization

1. **Efficient State Updates**: Use incremental updates

   ```rust
   // Good: Incremental update
   impl ProjectionHandler<AccountDebited> for AccountBalanceProjection {
       async fn handle(&mut self, event: &AccountDebited) -> Result<()> {
           let current = self.get_balance(&event.account_id).await?;
           let new_balance = current - event.amount;
           self.update_balance(&event.account_id, new_balance).await?;
           Ok(())
       }
   }

   // Bad: Full recalculation
   impl ProjectionHandler<AccountDebited> for AccountBalanceProjection {
       async fn handle(&mut self, event: &AccountDebited) -> Result<()> {
           let all_events = self.load_all_events(&event.account_id).await?;
           let balance = self.calculate_balance_from_events(all_events)?;
           self.update_balance(&event.account_id, balance).await?;
           Ok(())
       }
   }
   ```

2. **Parallel Processing**: Process independent projections concurrently

   ```rust
   use tokio::task::JoinSet;

   async fn process_projections_parallel(&self, event: &Event) -> Result<()> {
       let mut join_set = JoinSet::new();

       for projection in &self.projections {
           if projection.handles_event_type(&event.event_type) {
               let projection = projection.clone();
               let event = event.clone();
               join_set.spawn(async move {
                   projection.handle(&event).await
               });
           }
       }

       while let Some(result) = join_set.join_next().await {
           result??; // Handle errors appropriately
       }

       Ok(())
   }
   ```

## Troubleshooting Common Issues

### High Command Latency

**Symptoms**: Command duration P95 > 1000ms

**Investigation Steps**:

1. Check database query performance
2. Analyze stream access patterns
3. Review concurrent command execution
4. Examine event store connection health

**Solutions**:

- Optimize database indexes
- Reduce streams accessed per command
- Implement command queuing
- Scale database resources

### Memory Leaks

**Symptoms**: Continuous memory growth, eventual OOM

**Investigation Steps**:

1. Profile memory allocations
2. Check for circular references
3. Review caching strategies
4. Analyze event retention

**Solutions**:

- Implement proper cleanup in Drop impls
- Use weak references where appropriate
- Configure cache eviction policies
- Archive old events

### Projection Lag

**Symptoms**: Projections falling behind event creation

**Investigation Steps**:

1. Check projection processing performance
2. Analyze event volume patterns
3. Review resource constraints
4. Examine error rates

**Solutions**:

- Optimize projection handlers
- Implement parallel processing
- Scale projection infrastructure
- Batch projection updates

### Event Store Contention

**Symptoms**: High write latency, timeout errors

**Investigation Steps**:

1. Check database locks and conflicts
2. Analyze concurrent write patterns
3. Review transaction sizes
4. Examine connection pool utilization

**Solutions**:

- Optimize transaction scope
- Implement connection pooling
- Use read replicas for queries
- Scale database infrastructure

## Sample Monitoring Implementation

```rust
use eventcore::monitoring::{MetricsRegistry, loggers, SystemMetrics};
use std::time::Duration;
use tokio::time::interval;

pub struct MonitoringService {
    metrics: MetricsRegistry,
    logger: StructuredLogger,
}

impl MonitoringService {
    pub fn new() -> Self {
        Self {
            metrics: MetricsRegistry::new(),
            logger: loggers::health_monitor(),
        }
    }

    pub async fn start_monitoring(&self) {
        let mut interval = interval(Duration::from_secs(60));

        loop {
            interval.tick().await;

            self.collect_system_metrics().await;
            self.check_health_thresholds().await;
            self.update_dashboards().await;
        }
    }

    async fn collect_system_metrics(&self) {
        // Collect system metrics
        let memory_usage = self.get_memory_usage();
        let cpu_usage = self.get_cpu_usage();
        let pool_stats = self.get_connection_pool_stats();

        self.metrics.system_metrics.record_memory_usage(memory_usage);
        self.metrics.system_metrics.record_cpu_usage(cpu_usage);
        self.metrics.system_metrics.update_connection_pool(
            pool_stats.total,
            pool_stats.available
        );

        self.logger.log_health_metrics(
            memory_usage / 1_000_000.0, // Convert to MB
            cpu_usage,
            pool_stats.utilization(),
            self.metrics.error_metrics.calculate_error_rate(
                self.metrics.command_metrics.commands_executed.get()
            )
        );
    }

    async fn check_health_thresholds(&self) {
        let error_rate = self.metrics.error_metrics.calculate_error_rate(
            self.metrics.command_metrics.commands_executed.get()
        );

        if error_rate > 5.0 {
            self.trigger_alert("high_error_rate", error_rate).await;
        }

        if let Some(p95_duration) = self.metrics.command_metrics.command_duration.p95() {
            if p95_duration > Duration::from_millis(1000) {
                self.trigger_alert("high_latency", p95_duration.as_millis() as f64).await;
            }
        }
    }

    async fn trigger_alert(&self, alert_type: &str, value: f64) {
        self.logger.log_performance_alert(
            alert_type,
            self.get_threshold(alert_type),
            value,
            &HashMap::new()
        );

        // Send to alerting system (PagerDuty, Slack, etc.)
        self.send_alert_notification(alert_type, value).await;
    }
}
```

## Conclusion

Effective monitoring of EventCore applications requires:

1. **Comprehensive Metrics**: Track command, event store, and projection performance
2. **Proactive Alerting**: Set up alerts for critical issues before they impact users
3. **Performance Optimization**: Continuously optimize based on monitoring data
4. **Troubleshooting Procedures**: Have clear runbooks for common issues

By following these practices, you can maintain high-performance, reliable event-sourced applications with EventCore.
