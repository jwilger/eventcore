# EventCore Performance Benchmarks

This document contains performance benchmarking results and optimization analysis for the EventCore library.

## Phase 11.2: Performance Optimization Results

### Optimization Summary

The following optimizations were implemented to improve EventCore's performance:

#### 1. Memory Allocation Optimizations

- **State Reconstruction** (`eventcore/src/state_reconstruction.rs`): Removed unnecessary event sorting since events are already sorted by EventId (UUIDv7) from database queries
- **Command Execution** (`eventcore/src/executor.rs`):
  - Pre-allocated vectors to avoid reallocations during event conversion
  - Added capacity hints for HashMap operations (most commands use 1-4 streams)
  - Batched event conversion to reduce temporary allocations

#### 2. Database Connection Pool Optimization

- **PostgreSQL Adapter** (`eventcore-postgres/src/lib.rs`):
  - Increased max_connections from 10 to 20 for better concurrency
  - Increased min_connections from 1 to 2 to keep connections warm
  - Reduced connect_timeout from 30s to 10s for faster failure detection
  - Disabled test_before_acquire for performance (skip connection validation)

#### 3. Query Path Optimization

- **Event Store** (`eventcore-postgres/src/event_store.rs`):
  - Optimized single vs multi-stream query paths
  - Improved query construction with proper parameterization

#### 4. Caching Infrastructure

- **Version Caching** (`eventcore-postgres/src/lib.rs`):
  - Added stream version caching with 5-second TTL
  - Implemented LRU eviction (keeps last 1000 entries)
  - Used parking_lot RwLock for efficient read-heavy operations
  - Added cache invalidation on writes

### Current Performance Results

**Single-Stream Commands**: ~167 operations/second
**Multi-Stream Commands**: ~84 operations/second

**Memory Usage**: Optimizations reduced temporary allocations in hot paths

### Performance Target Analysis

| Metric                | Current | Target       | Gap    |
| --------------------- | ------- | ------------ | ------ |
| Single-stream ops/sec | 167     | 5,000-10,000 | 30-60x |
| Multi-stream ops/sec  | 84      | 2,000-5,000  | 24-60x |

### Gap Analysis

The current performance is significantly below targets, indicating that **micro-optimizations alone are insufficient**. The bottlenecks appear to be architectural:

1. **Database Round-trips**: Each command execution involves multiple database queries
2. **Transaction Overhead**: PostgreSQL ACID transactions add latency
3. **Serialization Costs**: JSON serialization/deserialization per event
4. **Network Latency**: Database connection latency affects each operation

### Future Performance Optimization Recommendations

To achieve target performance (5k-10k ops/sec), the following architectural changes are recommended:

#### 1. Connection Pooling & Database Optimization

- **Connection Pool Tuning**: Increase pool size based on workload patterns
- **Database Tuning**: Optimize PostgreSQL configuration for event sourcing workloads
- **Query Optimization**: Analyze and optimize slow queries with proper indexing

#### 2. Batching & Caching Strategies

- **Event Batching**: Implement batch writes to reduce database round-trips
- **Stream Version Caching**: Expand caching to reduce version lookup queries
- **Connection Reuse**: Minimize connection acquisition overhead

#### 3. Serialization Optimization

- **Binary Serialization**: Consider faster serialization formats (MessagePack, bincode)
- **Schema Evolution**: Implement efficient schema versioning
- **Compression**: Add event payload compression for large events

#### 4. Asynchronous Processing

- **Async Batching**: Group multiple commands for batch processing
- **Background Processing**: Move non-critical operations to background tasks
- **Pipeline Optimization**: Overlap I/O operations where possible

#### 5. Architectural Improvements

- **Event Buffering**: Add in-memory event buffering for high-throughput scenarios
- **Read Replicas**: Use read replicas for projection building
- **Partitioning**: Implement database partitioning for large datasets

### Benchmark Configuration

Benchmarks were run using:

- **Framework**: Criterion.rs
- **Runtime**: Tokio async runtime
- **Database**: PostgreSQL with default configuration
- **Timeout**: 10 minutes for comprehensive benchmark execution
- **Environment**: Development environment (not production-tuned)

### Next Steps

1. **Establish Baseline**: Create comprehensive benchmark suite covering various scenarios
2. **Profile Hot Paths**: Use profiling tools to identify remaining bottlenecks
3. **Database Tuning**: Optimize PostgreSQL configuration for event sourcing
4. **Implement Batching**: Add batch processing capabilities for high-throughput scenarios
5. **Monitor Improvements**: Track performance impact of each optimization

## Running Benchmarks

To run the benchmark suite:

```bash
# Navigate to project root
cd /home/jwilger/projects/eventcore

# Run benchmarks with extended timeout
cargo bench --timeout 600

# Run specific benchmark
cargo bench single_stream_commands

# Generate HTML reports
cargo bench -- --output-format html
```

## Performance Testing Environment

**Hardware**: Development machine specifications
**Database**: PostgreSQL 15+ with default configuration
**Network**: Local connections (minimal latency)
**Load**: Single-threaded benchmark execution

For production performance validation, benchmarks should be run in an environment that matches the target deployment.
