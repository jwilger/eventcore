# EventCore PostgreSQL Migrations

This directory contains SQL migrations for setting up the EventCore PostgreSQL event store schema.

## Migration Overview

### Migration Files

1. **001_create_event_streams.sql** - Creates the `event_streams` table for tracking stream metadata and versions
2. **002_create_events.sql** - Creates the main `events` table with comprehensive indexing
3. **003_create_performance_indexes.sql** - Adds additional performance-oriented indexes
4. **004_create_partitioning_strategy.sql** - Implements monthly partitioning for large-scale deployments
5. **005_create_simple_schema.sql** - Alternative non-partitioned schema for smaller deployments

## Schema Options

### Option 1: Simple Schema (Recommended for Development/Small Deployments)

For development environments or deployments expecting < 1M events per month:

```sql
-- Run migrations in order:
\i 001_create_event_streams.sql
\i 005_create_simple_schema.sql
```

This creates a simple, non-partitioned `events_simple` table with all necessary indexes.

### Option 2: Partitioned Schema (Recommended for Production)

For production environments expecting high event volumes:

```sql
-- Run migrations in order:
\i 001_create_event_streams.sql
\i 002_create_events.sql
\i 003_create_performance_indexes.sql
\i 004_create_partitioning_strategy.sql
```

This creates a monthly-partitioned `events` table optimized for high-throughput scenarios.

## Schema Features

### Event Streams Table

- Tracks stream metadata and current versions
- Supports optimistic concurrency control
- Automatic timestamp updates
- Efficient version checking indexes

### Events Table

- Stores all events with JSONB payloads
- UUIDv7 event IDs for global chronological ordering
- Comprehensive indexing for common query patterns
- Support for causation/correlation tracking
- Optional monthly partitioning for scale

### Indexing Strategy

The schema includes optimized indexes for:

- **Stream reading**: Fast retrieval of events by stream
- **Multi-stream operations**: Efficient aggregate-per-command pattern support
- **Projections**: Event type and temporal queries
- **Sagas**: Correlation and causation tracking
- **Monitoring**: Recent event analysis
- **Full-text search**: JSONB metadata and payload queries

### Partitioning Strategy

The partitioned schema provides:

- **Monthly partitions**: Automatically created and managed
- **Automatic partition creation**: Functions to create new partitions
- **Data retention**: Functions to drop old partitions
- **Performance optimization**: Partition pruning for temporal queries

## Performance Characteristics

### Simple Schema
- **Recommended for**: < 1M events/month
- **Query performance**: Excellent for moderate loads
- **Maintenance**: Minimal
- **Storage**: Standard table storage

### Partitioned Schema
- **Recommended for**: > 1M events/month
- **Query performance**: Excellent with partition pruning
- **Maintenance**: Requires partition management
- **Storage**: Optimized with partition-wise operations

## Maintenance Operations

### For Partitioned Schema

#### Create new partitions:
```sql
SELECT create_monthly_partition('2024-03-01'::DATE);
```

#### Drop old partitions (retain 24 months):
```sql
SELECT drop_old_partitions(24);
```

#### List existing partitions:
```sql
SELECT schemaname, tablename 
FROM pg_tables 
WHERE tablename LIKE 'events_%' 
ORDER BY tablename;
```

### General Maintenance

#### Reindex for performance:
```sql
REINDEX TABLE CONCURRENTLY events;  -- or events_simple
REINDEX TABLE CONCURRENTLY event_streams;
```

#### Analyze for query planning:
```sql
ANALYZE events;  -- or events_simple
ANALYZE event_streams;
```

#### Check index usage:
```sql
SELECT schemaname, tablename, indexname, idx_tup_read, idx_tup_fetch
FROM pg_stat_user_indexes 
WHERE schemaname = 'public'
ORDER BY idx_tup_read DESC;
```

## Connection Configuration

When configuring the EventCore PostgreSQL adapter, ensure your connection pool settings accommodate your expected load:

```rust
// Example configuration for high-throughput scenarios
let config = PostgresConfig {
    max_connections: 20,
    min_connections: 5,
    acquire_timeout: Duration::from_secs(30),
    idle_timeout: Some(Duration::from_secs(600)),
};
```

## Schema Evolution

### Adding New Indexes

Always use `CREATE INDEX CONCURRENTLY` to avoid blocking writes:

```sql
CREATE INDEX CONCURRENTLY idx_custom_name 
ON events (your_column) 
WHERE your_condition;
```

### Adding New Columns

Add new columns as nullable first, then populate and add constraints:

```sql
-- Step 1: Add nullable column
ALTER TABLE events ADD COLUMN new_column TEXT;

-- Step 2: Populate existing rows (if needed)
UPDATE events SET new_column = 'default_value' WHERE new_column IS NULL;

-- Step 3: Add constraint (if needed)
ALTER TABLE events ALTER COLUMN new_column SET NOT NULL;
```

## Monitoring Queries

### Check partition sizes:
```sql
SELECT 
    schemaname,
    tablename,
    pg_size_pretty(pg_total_relation_size(schemaname||'.'||tablename)) as size
FROM pg_tables 
WHERE tablename LIKE 'events_%'
ORDER BY pg_total_relation_size(schemaname||'.'||tablename) DESC;
```

### Check recent activity:
```sql
SELECT 
    DATE_TRUNC('hour', created_at) as hour,
    COUNT(*) as event_count,
    COUNT(DISTINCT stream_id) as unique_streams
FROM events 
WHERE created_at > NOW() - INTERVAL '24 hours'
GROUP BY DATE_TRUNC('hour', created_at)
ORDER BY hour DESC;
```

### Check index effectiveness:
```sql
SELECT 
    idx.indexrelname as index_name,
    idx.idx_tup_read as tuples_read,
    idx.idx_tup_fetch as tuples_fetched,
    ROUND(idx.idx_tup_fetch::NUMERIC / NULLIF(idx.idx_tup_read, 0) * 100, 2) as efficiency_percent
FROM pg_stat_user_indexes idx
JOIN pg_stat_user_tables tbl ON idx.relid = tbl.relid
WHERE tbl.relname IN ('events', 'events_simple', 'event_streams')
ORDER BY idx.idx_tup_read DESC;
```