# PostgreSQL Prepared Statement Performance

## Overview

SQLx automatically manages prepared statement caching internally when using the connection pool. This document explains how EventCore leverages this built-in functionality for optimal performance.

## How SQLx Handles Prepared Statements

When you execute a query through SQLx with a PostgreSQL connection pool:

1. **First Execution**: SQLx prepares the statement on the PostgreSQL server and caches it in the connection
2. **Subsequent Executions**: The cached prepared statement is reused, avoiding re-parsing overhead
3. **Connection-Scoped**: Each connection in the pool maintains its own prepared statement cache

## EventCore Query Patterns

EventCore benefits from prepared statement caching for these frequently-executed queries:

### Simple Parameterized Queries

These queries have fixed SQL with only parameter values changing:

- **Stream Existence Check**: `SELECT EXISTS(SELECT 1 FROM event_streams WHERE stream_id = $1)`
- **Get Stream Version**: `SELECT MAX(event_version) FROM events WHERE stream_id = $1)`
- **Subscription Checkpoint Updates**: `INSERT INTO subscription_checkpoints ... ON CONFLICT DO UPDATE`
- **Stream ID Listing**: `SELECT DISTINCT stream_id FROM events LIMIT 1000`

SQLx automatically caches these as prepared statements on each connection.

### Dynamic Queries

Some queries have dynamic SQL that varies based on input:

- **Multi-stream reads** with variable stream counts
- **Event queries** with optional version filtering
- **Batch insertions** with variable batch sizes

These queries cannot benefit from prepared statement caching as each variation requires a different statement.

## Performance Considerations

### Benefits of SQLx's Automatic Caching

1. **No Manual Management**: SQLx handles the lifecycle of prepared statements
2. **Connection Affinity**: Each connection maintains its own cache, avoiding contention
3. **Automatic Cleanup**: Statements are cleaned up when connections are closed
4. **Memory Efficiency**: Only frequently-used statements remain cached

### Optimization Tips

1. **Connection Pool Size**: Ensure adequate connections to benefit from caching
   ```rust
   let config = PostgresConfig {
       max_connections: 25,  // Adjust based on workload
       min_connections: 5,   // Keep warm connections ready
       // ...
   };
   ```

2. **Connection Lifetime**: Longer-lived connections improve cache hit rates
   ```rust
   let config = PostgresConfig {
       max_lifetime: Duration::from_secs(3600),  // 1 hour
       idle_timeout: Duration::from_secs(600),   // 10 minutes
       // ...
   };
   ```

3. **Query Patterns**: Use consistent query patterns where possible to maximize reuse

## Monitoring Performance

While EventCore doesn't explicitly track prepared statement metrics (as they're managed by SQLx), you can monitor overall query performance through:

1. **PostgreSQL Statistics**:
   ```sql
   -- View prepared statement usage
   SELECT * FROM pg_prepared_statements;
   
   -- Monitor query performance
   SELECT * FROM pg_stat_statements 
   WHERE query LIKE '%events%' 
   ORDER BY total_time DESC;
   ```

2. **Connection Pool Metrics**: Monitor via EventCore's `PoolMonitor`
3. **Application Tracing**: Use EventCore's tracing integration

## Future Optimizations

While SQLx's automatic prepared statement caching provides good performance, potential future optimizations include:

1. **Query Pattern Analysis**: Identify queries that would benefit from manual optimization
2. **Batch Size Optimization**: Find optimal batch sizes for event insertion
3. **Custom Caching Layer**: For complex dynamic queries that can't use prepared statements

## Conclusion

EventCore leverages SQLx's built-in prepared statement caching to achieve good performance without manual statement management. The combination of:

- Simple, parameterized queries where possible
- Proper connection pool configuration
- SQLx's automatic caching

Provides efficient query execution for most event sourcing workloads.