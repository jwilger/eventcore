# eventcore-postgres

PostgreSQL adapter for EventCore - production-ready event store with ACID guarantees.

## Features

- **Multi-stream atomicity** via PostgreSQL transactions
- **Optimistic concurrency control** with version checking
- **Type-safe serialization** - your events, not JSON blobs
- **Connection pooling** with sqlx
- **Automatic retries** on transient failures
- **Concurrent schema initialization** with advisory locks

## Installation

```toml
[dependencies]
eventcore = "0.1"
eventcore-postgres = "0.1"
```

## Quick Start

```rust
use eventcore_postgres::{PostgresEventStore, PostgresConfig};
use eventcore::CommandExecutor;
use std::time::Duration;

// Configure with basic settings
let config = PostgresConfig::new("postgres://user:pass@localhost/eventcore");

// Or use the builder for advanced configuration
let config = PostgresConfigBuilder::new()
    .database_url("postgres://user:pass@localhost/eventcore")
    .max_connections(20)
    .min_connections(2)
    .connect_timeout(Duration::from_secs(10))
    .idle_timeout(Some(Duration::from_secs(600)))
    .max_lifetime(Some(Duration::from_secs(1800)))
    .build();

// Initialize
let store = PostgresEventStore::new(config).await?;
store.initialize().await?; // Creates schema (safe to call multiple times)

// Use with commands
let executor = CommandExecutor::new(store);
```

## Configuration

### Connection Pool Options

PostgresConfig provides comprehensive connection pool configuration:

- `max_connections` (default: 20) - Maximum connections in the pool
- `min_connections` (default: 2) - Minimum idle connections to maintain
- `connect_timeout` (default: 10s) - Connection acquisition timeout
- `idle_timeout` (default: 600s) - How long connections can remain idle
- `max_lifetime` (default: 1800s) - Maximum lifetime of a connection
- `query_timeout` (default: 30s) - Timeout for individual queries
- `test_before_acquire` (default: false) - Test connections before use

### Code Configuration

```rust
use eventcore_postgres::{PostgresConfigBuilder, PostgresEventStore};
use std::time::Duration;

// Basic configuration
let config = PostgresConfig::new("postgres://localhost/eventcore");

// Advanced configuration with builder
let config = PostgresConfigBuilder::new()
    .database_url("postgres://localhost/eventcore")
    .max_connections(30)
    .min_connections(5)
    .connect_timeout(Duration::from_secs(5))
    .idle_timeout(Some(Duration::from_secs(300)))
    .max_lifetime(Some(Duration::from_secs(1800)))
    .query_timeout(Some(Duration::from_secs(10)))
    .test_before_acquire(false)  // Skip for better performance
    .build();

// Performance-optimized preset
let config = PostgresConfigBuilder::new()
    .database_url("postgres://localhost/eventcore")
    .performance_optimized()  // Applies optimized settings
    .build();
```

### Environment Variables

```bash
DATABASE_URL=postgres://localhost/eventcore
# Note: Connection pool settings are configured in code, not via environment variables
```

## Schema

EventCore uses two tables with optimized indexes:

```sql
-- Event streams with version tracking
CREATE TABLE event_streams (
    stream_id VARCHAR(255) PRIMARY KEY,
    version BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Events with efficient ordering
CREATE TABLE events (
    event_id UUID PRIMARY KEY,
    stream_id VARCHAR(255) NOT NULL REFERENCES event_streams(stream_id),
    event_type VARCHAR(255) NOT NULL,
    event_data JSONB NOT NULL,
    metadata JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes for performance
CREATE INDEX idx_events_stream_created ON events(stream_id, created_at);
CREATE INDEX idx_events_created_at ON events(created_at);
```

## Production Considerations

### Connection Pool Sizing

```rust
// For high-throughput systems
let config = PostgresConfigBuilder::new()
    .database_url(database_url)
    .max_connections(num_cpus::get() as u32 * 2)  // Rule of thumb: 2x CPU cores
    .min_connections(num_cpus::get() as u32)      // Keep connections warm
    .idle_timeout(Some(Duration::from_secs(300))) // 5 minutes
    .max_lifetime(Some(Duration::from_secs(3600))) // 1 hour
    .build();

// For bursty workloads
let config = PostgresConfigBuilder::new()
    .database_url(database_url)
    .max_connections(50)                          // Higher max for bursts
    .min_connections(5)                           // Lower minimum
    .connect_timeout(Duration::from_secs(2))      // Fast timeout
    .idle_timeout(Some(Duration::from_secs(60)))  // Aggressive cleanup
    .build();

// For long-running operations
let config = PostgresConfigBuilder::new()
    .database_url(database_url)
    .max_connections(15)
    .query_timeout(Some(Duration::from_secs(300))) // 5 minute queries
    .max_lifetime(None)                            // No lifetime limit
    .build();
```

### Transaction Isolation

EventCore uses `SERIALIZABLE` isolation for multi-stream writes, ensuring:
- No phantom reads
- No write skew
- Full ACID compliance

### Performance Tuning

```sql
-- Increase shared_buffers for better caching
ALTER SYSTEM SET shared_buffers = '256MB';

-- Enable parallel queries
ALTER SYSTEM SET max_parallel_workers_per_gather = 4;

-- Tune for SSDs
ALTER SYSTEM SET random_page_cost = 1.1;
```

### Monitoring

The adapter provides comprehensive connection pool monitoring:

```rust
// Get current pool metrics
let metrics = store.get_pool_metrics();
println!("Active connections: {}", metrics.active_connections);
println!("Idle connections: {}", metrics.idle_connections);
println!("Pool utilization: {:.1}%", metrics.utilization_percent);

// Start background monitoring
let (monitor_task, stop_tx) = store.start_pool_monitoring();

// Access detailed health information
let health = store.health_check().await?;
println!("Pool healthy: {}", health.pool_status.is_closed == false);
println!("Query latency: {:?}", health.performance_status.query_latency);

// Enable detailed tracing
tracing_subscriber::fmt()
    .with_env_filter("eventcore_postgres=debug")
    .init();
```

Key metrics to monitor:
- `current_connections` - Active connections in pool
- `idle_connections` - Available connections
- `utilization_percent` - Pool usage (0-100%)
- `connection_timeouts` - Failed connection attempts
- `avg_acquisition_time` - Time to get a connection
- `peak_connections` - Historical maximum

## Testing

For integration tests, use Docker:

```yaml
# docker-compose.yml
services:
  postgres:
    image: postgres:15
    environment:
      POSTGRES_DB: eventcore_test
      POSTGRES_PASSWORD: postgres
    ports:
      - "5433:5432"
```

```rust
#[tokio::test]
async fn test_with_postgres() {
    let config = PostgresConfig::test_default(); // Uses TEST_DATABASE_URL
    let store = PostgresEventStore::new(config).await.unwrap();
    store.initialize().await.unwrap();
    
    // Run your tests...
}
```

## Migration from Other Event Stores

### From EventStoreDB

```rust
// EventCore handles projections differently
// Instead of catch-up subscriptions, use:
let projection = MyProjection::new();
let runner = ProjectionRunner::new(store, projection);
runner.run_continuous().await?;
```

### From Axon

```rust
// No more aggregate classes!
// Commands handle their own state:
impl Command for MyCommand {
    fn apply(&self, state: &mut State, event: &StoredEvent<Event>) {
        // Direct state manipulation
    }
}
```

## Troubleshooting

### "Version conflict" errors

This is optimistic concurrency control working correctly. EventCore will automatically retry with exponential backoff.

### "Connection pool timeout"

This indicates all connections are busy. Solutions:

```rust
// 1. Increase pool size
let config = PostgresConfigBuilder::new()
    .database_url(database_url)
    .max_connections(30)  // Increase from default 20
    .connect_timeout(Duration::from_secs(10))  // Give more time
    .build();

// 2. Check pool health
let metrics = store.get_pool_metrics();
if metrics.utilization_percent > 80.0 {
    warn!("Pool utilization high: {:.1}%", metrics.utilization_percent);
}

// 3. Enable connection recovery
let config = PostgresConfig {
    enable_recovery: true,  // Automatic recovery on failures
    max_retries: 5,        // More retry attempts
    ..PostgresConfig::new(database_url)
};
```

### Schema initialization hangs

Another process might be initializing. This is normal - EventCore uses advisory locks to prevent race conditions.

## See Also

- [Connection Tuning Guide](docs/CONNECTION_TUNING.md) - Detailed connection pool tuning
- [EventCore Core](../eventcore/) - Core library documentation
- [Examples](../eventcore-examples/) - Complete applications
- [Memory Adapter](../eventcore-memory/) - For testing