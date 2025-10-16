# Performance Test Database Schema Issues

## Issue Summary

The performance validation tests have been implemented correctly but cannot execute due to database schema compatibility issues between the migrations and the actual database state.

## Problems Identified

1. **Missing Columns**: The database `events` table is missing expected columns:
   - `correlation_id`
   - `causation_id`
   - `current_version` (expected by the PostgreSQL adapter)

2. **Type Mismatches**: Schema inconsistencies between migrations and actual implementation:
   - Migration expects `event_version` but table has `stream_position`
   - Migration expects `VARCHAR` stream_id but table has `UUID`

3. **Migration State**: The sqlx migration tracking is not properly set up
   - `_sqlx_migrations` table doesn't exist
   - Unclear which migrations have been applied

## Test Implementation Status

✅ **Fixed Issues**:

- Removed buggy shared metrics collection causing impossible success rates (134.6%, 0.1%)
- Fixed overflow errors in metrics calculation
- Converted tests to use PostgreSQL instead of meaningless in-memory benchmarks
- Implemented proper performance measurement with realistic workloads
- Created comprehensive reporting framework

❌ **Blocked By Schema Issues**:

- Tests cannot execute due to missing database columns
- PostgreSQL adapter expects schema that doesn't match actual database

## Next Steps Required

1. **Schema Resolution** (High Priority):
   - Determine correct schema for production use
   - Apply proper migrations to align database with code expectations
   - Verify migration state and apply missing migrations

2. **After Schema Fix**:
   - Execute performance tests against real PostgreSQL
   - Generate accurate performance report
   - Validate against PRD targets with real-world data

## Performance Test Framework

The test framework is now correct and ready to execute once schema issues are resolved:

- **Single-stream test**: 1,000 financial transactions
- **Multi-stream test**: 500 e-commerce orders (2-5 streams each)
- **Batch write test**: 20 batches of 100 events each
- **Metrics**: Throughput, latency percentiles (P50, P95, P99), success rates
- **Validation**: Against PRD targets (5K-10K single-stream, 2K-5K multi-stream, 20K+ batch, <10ms P95)

## Files Modified

- `eventcore/tests/performance_target_validation.rs`: Fixed implementation bugs, converted to PostgreSQL
- `docs/performance-report.md`: Updated with corrected approach (needs real data after schema fix)

## Test Execution

Once schema is fixed, run:

```bash
cargo test --package eventcore test_performance_targets --features testing -- --ignored --nocapture
```
