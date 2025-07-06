# Benchmark Results Summary

## Date: 2025-07-06
## After: Executor refactoring (PR #10)

### Validation Optimization Benchmarks
- **single_validation_no_cache**: ~406 ns (−1.19% change, within noise)
- **single_validation_with_cache**: ~144 ns (no change detected)
- **batch_validation_sizes/1**: ~181 ns (no change detected)
- **batch_validation_sizes/5**: ~863 ns (−1.14% change, within noise)
- **batch_validation_sizes/10**: ~1.74 µs (no change detected)
- **batch_validation_sizes/25**: ~4.41 µs (+0.70% change, within noise)

### Projection Processing Benchmarks
- **single_event_processing**: ~2.38 µs (421 Kelem/s throughput)
- **batch_event_processing/10**: ~12.56 µs (796 Kelem/s throughput)
- **batch_event_processing/50**: ~61.81 µs (809 Kelem/s throughput)
- **batch_event_processing/100**: ~122.98 µs (813 Kelem/s throughput)
- **batch_event_processing/500**: ~626.20 µs (798 Kelem/s throughput)
- **get_projection_state**: ~73 ns (13.69 Melem/s throughput)

### Event Store Benchmarks
- **single_stream_reads/10**: ~954 ns (10.48 Melem/s throughput)
- **single_stream_reads/100**: ~9.75 µs (10.25 Melem/s throughput)
- **single_stream_reads/1000**: ~94.06 µs (10.63 Melem/s throughput)

## Summary
No significant performance regressions detected. All changes are either improvements or within noise threshold. The refactoring of executor.rs has maintained the same performance characteristics while improving code maintainability.