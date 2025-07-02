# PostgreSQL Index Optimization for Multi-Stream Reads

## Overview

EventCore's multi-stream event sourcing pattern requires careful index optimization to achieve high performance when reading from multiple streams simultaneously.

## Key Query Patterns

### 1. Multi-Stream Read with ANY($1)

The most common query pattern in EventCore uses PostgreSQL's `ANY($1)` syntax:

```sql
SELECT event_id, stream_id, event_version, event_type, event_data, metadata, 
       causation_id, correlation_id, user_id, created_at
FROM events
WHERE stream_id = ANY($1)
ORDER BY event_id
LIMIT $2
```

This pattern is used by:
- Command execution (reading multiple streams atomically)
- Projection building (reading related streams)
- Saga coordination (reading participating streams)

### 2. Optimized Indexes

EventCore creates the following indexes automatically:

#### Primary Indexes
- `idx_events_stream_id` - Single column index on stream_id
- `idx_events_created_at` - For temporal queries
- `idx_events_correlation_id` - For saga coordination

#### Multi-Stream Optimization
- `idx_events_multistream_any` - Composite index on (stream_id, event_id)
  - Optimizes the `WHERE stream_id = ANY($1) ORDER BY event_id` pattern
  - Allows index-only scans for event ID ordering
  - Reduces random I/O by keeping related data together

## Performance Tuning

### 1. Query Planning

PostgreSQL's query planner handles `ANY($1)` efficiently when:
- The array size is reasonable (< 1000 streams)
- Proper indexes exist on stream_id
- Statistics are up to date

Run `ANALYZE events;` periodically to ensure optimal query plans.

### 2. Large Stream Arrays

For commands that read many streams (> 100), consider:

```sql
-- Force index usage with hint
SET enable_seqscan = OFF;
-- Run your multi-stream query
SET enable_seqscan = ON;
```

### 3. Monitoring Index Usage

Check index effectiveness:

```sql
-- View index usage statistics
SELECT 
    schemaname,
    tablename,
    indexname,
    idx_scan,
    idx_tup_read,
    idx_tup_fetch
FROM pg_stat_user_indexes
WHERE tablename = 'events'
ORDER BY idx_scan DESC;

-- Check for missing indexes
SELECT 
    schemaname,
    tablename,
    attname,
    n_distinct,
    correlation
FROM pg_stats
WHERE tablename = 'events'
AND attname IN ('stream_id', 'event_id', 'created_at');
```

### 4. Batch Size Optimization

The `read_batch_size` configuration affects performance:

- **Small batches (100-500)**: Lower memory usage, faster first results
- **Medium batches (1000-5000)**: Balanced performance for most workloads
- **Large batches (10000+)**: Better throughput for bulk operations

Configure based on your workload:

```rust
let config = PostgresConfigBuilder::new()
    .database_url(url)
    .read_batch_size(2000)  // Adjust based on testing
    .build();
```

## Index Maintenance

### 1. Bloat Prevention

Multi-stream workloads can cause index bloat. Monitor with:

```sql
SELECT 
    schemaname,
    tablename,
    indexname,
    pg_size_pretty(pg_relation_size(indexrelid)) AS index_size,
    idx_scan
FROM pg_stat_user_indexes
WHERE tablename = 'events'
ORDER BY pg_relation_size(indexrelid) DESC;
```

### 2. Reindexing Strategy

For production systems:

```sql
-- Create new index concurrently
CREATE INDEX CONCURRENTLY idx_events_multistream_any_new 
ON events(stream_id, event_id);

-- Drop old index
DROP INDEX idx_events_multistream_any;

-- Rename new index
ALTER INDEX idx_events_multistream_any_new 
RENAME TO idx_events_multistream_any;
```

## Advanced Optimizations

### 1. Partitioning for Scale

For very large event stores (> 100M events), consider partitioning:

```sql
-- Partition by created_at for time-based archival
CREATE TABLE events_2024_01 PARTITION OF events
FOR VALUES FROM ('2024-01-01') TO ('2024-02-01');
```

### 2. Partial Indexes

For specific access patterns:

```sql
-- Index only active streams
CREATE INDEX idx_active_streams ON events(stream_id, event_id)
WHERE created_at > NOW() - INTERVAL '30 days';
```

### 3. BRIN Indexes for Analytics

For large sequential scans:

```sql
CREATE INDEX idx_events_created_brin ON events
USING BRIN(created_at);
```

## Conclusion

Proper indexing is crucial for EventCore's multi-stream performance. The default indexes handle most workloads well, but understanding these patterns helps optimize for specific use cases.