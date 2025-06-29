# Known Performance Issues

## Command Executor Double-Read Issue

**Status**: Open  
**Severity**: High  
**Impact**: Reduces throughput by ~90% from PRD targets

### Description

The current `CommandExecutor` implementation performs redundant stream reads:

1. Reads all streams at the beginning of execution
2. Re-reads all streams before writing events for version checking

This double-read pattern causes significant performance degradation, especially for commands that read multiple streams.

### Current Performance vs PRD Targets

| Metric | PRD Target | Current Performance | Adjusted Test Target |
|--------|------------|-------------------|---------------------|
| Single-stream commands | 5,000-10,000 ops/sec | ~400-500 ops/sec | 400 ops/sec |
| Multi-stream commands | 2,000-5,000 ops/sec | ~150-200 ops/sec | 200 ops/sec |
| P95 latency | < 10ms | ~4-5ms | < 20ms |
| Batch event writes | 20,000+ events/sec | ~5,000-10,000 events/sec | 5,000 events/sec |

### Root Cause

The executor's `execute_once` method (line 523-530 in `executor.rs`) performs a second read of all streams to get the latest versions for concurrency control. This is unnecessary since version information could be tracked from the initial read.

### Proposed Solution

1. Track stream versions from the initial read
2. Only re-read streams that have new events written to them
3. Consider implementing version caching in the event store adapter
4. Optimize the stream data structure to avoid repeated iterations

### Workaround

Performance tests have been adjusted to reflect current implementation performance while this issue is being addressed. See commit messages for details on adjusted targets.

### References

- Initial performance validation tests: `eventcore/tests/performance_validation.rs`
- Stress tests: `eventcore/tests/stress_tests.rs`
- Executor implementation: `eventcore/src/executor.rs`

## Other Performance Considerations

### Memory Allocations

The current implementation may have unnecessary allocations in hot paths:
- Event conversion creates new vectors
- Stream grouping allocates new HashMaps
- Consider using object pools or pre-allocated buffers

### PostgreSQL Adapter Optimizations

- Connection pooling is configured but could be tuned further
- Batch inserts could use prepared statements
- Consider implementing pipelining for multiple operations