# PostgreSQL Connection Pool Tuning Guide

This guide provides detailed information on tuning EventCore's PostgreSQL connection pool for optimal performance in production environments.

## Understanding Connection Pools

Connection pooling is critical for PostgreSQL performance because:

- Creating new database connections is expensive (~50-100ms)
- PostgreSQL has a per-connection memory overhead (~10MB)
- Connection limits prevent database overload

## Key Configuration Parameters

### Pool Size Configuration

#### `max_connections` (default: 20)

The maximum number of connections in the pool. This should be set based on:

- Number of concurrent requests your application handles
- PostgreSQL's `max_connections` setting (default: 100)
- Available memory on database server

**Formula**: `max_connections = min(expected_concurrent_requests * 1.2, postgres_max_connections * 0.8)`

#### `min_connections` (default: 2)

Minimum idle connections to maintain. Benefits:

- Eliminates connection startup latency for initial requests
- Provides instant capacity for traffic spikes
- Costs: Idle connection memory on database server

**Recommendation**: Set to 10-25% of `max_connections`

### Timeout Configuration

#### `connect_timeout` (default: 10s)

How long to wait when acquiring a connection from the pool.

- **Short timeouts (1-5s)**: Fail fast, good for user-facing APIs
- **Long timeouts (10-30s)**: Better for batch jobs, background workers

#### `query_timeout` (default: 30s)

Maximum time for individual query execution.

- **OLTP workloads**: 1-10 seconds
- **Analytical queries**: 30-300 seconds
- **Batch operations**: May need no timeout

#### `idle_timeout` (default: 600s)

How long a connection can remain idle before being closed.

- **High traffic**: 300-600 seconds (5-10 minutes)
- **Bursty traffic**: 60-300 seconds (1-5 minutes)
- **Cost-sensitive**: 30-60 seconds

#### `max_lifetime` (default: 1800s)

Maximum lifetime of a connection regardless of activity.

Benefits of connection recycling:

- Prevents memory leaks in long-lived connections
- Distributes load after database failovers
- Clears connection-specific state

## Workload-Specific Configurations

### High-Throughput OLTP

```rust
let config = PostgresConfigBuilder::new()
    .database_url(url)
    .max_connections(50)
    .min_connections(10)
    .connect_timeout(Duration::from_secs(2))
    .query_timeout(Some(Duration::from_secs(5)))
    .idle_timeout(Some(Duration::from_secs(300)))
    .test_before_acquire(false)  // Skip for performance
    .build();
```

### Batch Processing

```rust
let config = PostgresConfigBuilder::new()
    .database_url(url)
    .max_connections(10)  // Fewer connections for long operations
    .min_connections(2)
    .connect_timeout(Duration::from_secs(30))
    .query_timeout(None)  // No timeout for batch jobs
    .max_lifetime(Some(Duration::from_secs(7200)))  // 2 hours
    .build();
```

### Microservices

```rust
let config = PostgresConfigBuilder::new()
    .database_url(url)
    .max_connections(5)   // Conservative for many instances
    .min_connections(1)
    .connect_timeout(Duration::from_secs(3))
    .query_timeout(Some(Duration::from_secs(10)))
    .idle_timeout(Some(Duration::from_secs(60)))  // Aggressive cleanup
    .build();
```

## Monitoring and Health Checks

### Built-in Health Checks

EventCore performs automatic health checks including:

- Basic connectivity tests
- Connection pool status monitoring
- Query performance benchmarking
- Schema verification

```rust
// Comprehensive health check
let health = store.health_check().await?;

// Check specific aspects
if health.pool_status.idle == 0 && health.pool_status.size == config.max_connections {
    warn!("Connection pool exhausted!");
}

if health.performance_status.query_latency > Duration::from_millis(100) {
    warn!("Database performance degraded");
}
```

### Continuous Monitoring

```rust
// Start background monitoring
let (task, stop_signal) = store.start_pool_monitoring();

// The monitor tracks:
// - Connection acquisition times
// - Pool utilization trends
// - Connection lifecycle events
// - Error rates and timeouts
```

### Key Metrics to Track

1. **Pool Utilization** (`utilization_percent`)
   - Healthy: < 70%
   - Warning: 70-85%
   - Critical: > 85%

2. **Connection Wait Time** (`avg_acquisition_time`)
   - Excellent: < 1ms
   - Good: 1-10ms
   - Poor: > 10ms

3. **Connection Churn** (`total_connections_created`)
   - High churn indicates connections dying prematurely
   - Check `max_lifetime` and `idle_timeout` settings

4. **Timeout Rate** (`connection_timeouts`)
   - Should be < 0.1% of requests
   - High rate indicates undersized pool

## PostgreSQL Server Tuning

### Database Configuration

```sql
-- Increase connection limit if needed
ALTER SYSTEM SET max_connections = 200;

-- Reserve connections for superuser
ALTER SYSTEM SET superuser_reserved_connections = 3;

-- Connection memory settings
ALTER SYSTEM SET work_mem = '4MB';
ALTER SYSTEM SET maintenance_work_mem = '64MB';

-- Connection timeout settings
ALTER SYSTEM SET idle_in_transaction_session_timeout = '5min';
ALTER SYSTEM SET statement_timeout = '30s';
```

### Connection Limits by Tier

- **Development**: 20-50 connections
- **Small Production**: 50-100 connections
- **Medium Production**: 100-300 connections
- **Large Production**: 300-1000 connections (use pgBouncer)

## Troubleshooting

### Problem: "Too many connections" errors

**Solutions**:

1. Reduce `max_connections` in EventCore config
2. Check for connection leaks (connections not returned to pool)
3. Implement connection pooling at database level (pgBouncer)
4. Scale database vertically or use read replicas

### Problem: High connection acquisition time

**Solutions**:

1. Increase `min_connections` to maintain warm connections
2. Reduce `idle_timeout` to free connections faster
3. Increase `max_connections` if utilization is high
4. Check for long-running queries blocking connections

### Problem: Intermittent connection failures

**Solutions**:

1. Enable `test_before_acquire` (impacts performance)
2. Reduce `max_lifetime` to refresh connections more often
3. Check network stability between application and database
4. Review PostgreSQL logs for connection errors

## Best Practices

1. **Start Conservative**: Begin with lower connection limits and increase based on monitoring
2. **Monitor Continuously**: Use EventCore's built-in monitoring to track pool health
3. **Test Under Load**: Verify settings with realistic traffic patterns
4. **Plan for Failures**: Ensure settings allow graceful degradation
5. **Document Changes**: Track configuration changes and their impact

## Example: Production Configuration

```rust
use eventcore_postgres::{PostgresConfigBuilder, PostgresEventStore};
use std::time::Duration;

// Production configuration for a high-traffic event sourcing system
let config = PostgresConfigBuilder::new()
    .database_url(std::env::var("DATABASE_URL")?)
    // Pool sizing based on expected load
    .max_connections(40)  // Support 30-35 concurrent operations
    .min_connections(5)   // Keep 5 connections warm

    // Timeouts for reliability
    .connect_timeout(Duration::from_secs(5))     // Fail fast if overloaded
    .query_timeout(Some(Duration::from_secs(10))) // Prevent runaway queries

    // Connection lifecycle
    .idle_timeout(Some(Duration::from_secs(300)))   // 5 minute idle timeout
    .max_lifetime(Some(Duration::from_secs(3600)))  // Recycle hourly

    // Performance optimizations
    .test_before_acquire(false)  // Trust connection state

    // Retry configuration
    .max_retries(3)
    .retry_base_delay(Duration::from_millis(100))
    .retry_max_delay(Duration::from_secs(2))

    .build();

// Initialize with monitoring
let store = PostgresEventStore::new(config).await?;
let (monitor_task, _) = store.start_pool_monitoring();

// Log pool metrics periodically
tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        let metrics = store.get_pool_metrics();
        info!(
            "Pool health: {} active, {} idle, {:.1}% utilization",
            metrics.active_connections,
            metrics.idle_connections,
            metrics.utilization_percent
        );
    }
});
```

## Additional Resources

- [PostgreSQL Connection Pooling](https://www.postgresql.org/docs/current/runtime-config-connection.html)
- [PgBouncer Documentation](https://www.pgbouncer.org/)
- [Connection Pool Sizing Calculator](https://github.com/brettwooldridge/HikariCP/wiki/About-Pool-Sizing)
