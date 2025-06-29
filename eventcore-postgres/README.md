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

// Configure
let config = PostgresConfig {
    url: "postgres://user:pass@localhost/eventcore".into(),
    max_connections: 10,
    acquire_timeout: Duration::from_secs(5),
};

// Initialize
let store = PostgresEventStore::new(config).await?;
store.initialize().await?; // Creates schema (safe to call multiple times)

// Use with commands
let executor = CommandExecutor::new(store);
```

## Configuration

### Environment Variables

```bash
DATABASE_URL=postgres://localhost/eventcore
DATABASE_MAX_CONNECTIONS=10
DATABASE_ACQUIRE_TIMEOUT=5
```

### Code Configuration

```rust
let config = PostgresConfig::builder()
    .url("postgres://localhost/eventcore")
    .max_connections(10)
    .acquire_timeout(Duration::from_secs(5))
    .build()?;
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
let config = PostgresConfig::builder()
    .url(database_url)
    .max_connections(num_cpus::get() * 2)  // Rule of thumb
    .min_connections(num_cpus::get())      // Keep connections warm
    .build()?;
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

The adapter exposes metrics via the `tracing` crate:

```rust
// Enable detailed tracing
tracing_subscriber::fmt()
    .with_env_filter("eventcore_postgres=debug")
    .init();
```

Key metrics to monitor:
- Connection pool utilization
- Transaction retry rates
- Event write latency
- Version conflict frequency

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

Increase pool size or connection timeout:
```rust
.max_connections(20)
.acquire_timeout(Duration::from_secs(10))
```

### Schema initialization hangs

Another process might be initializing. This is normal - EventCore uses advisory locks to prevent race conditions.

## See Also

- [EventCore Core](../eventcore/) - Core library documentation
- [Examples](../eventcore-examples/) - Complete applications
- [Memory Adapter](../eventcore-memory/) - For testing