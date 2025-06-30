# EventCore Testing Strategy

## Test Categories

### Unit Tests
- Run in pre-commit hooks
- Fast, isolated tests of individual components
- Run with: `cargo test --lib --bins` or `cargo test-unit`

### Integration Tests
- Run in CI
- Test interactions between components
- Include database integration tests
- Run with: `cargo test --workspace`

### Performance Tests
- **NOT run in CI or pre-commit hooks**
- Require controlled environment for accurate results
- Must be run explicitly
- Located in:
  - `eventcore/tests/performance_validation.rs`
  - `eventcore/tests/stress_tests.rs` (performance-related tests)

## Running Tests

### Pre-commit Hook
Automatically runs: `cargo test --workspace --lib --bins`
- Only unit tests
- Fast feedback loop

### CI Pipeline
Runs: `cargo nextest run --workspace`
- Unit tests
- Integration tests
- Excludes performance tests

### Performance Testing
Run explicitly with: `cargo test-perf`
- Requires release mode for accurate results: `cargo test-perf --release`
- Should be run on dedicated hardware or controlled environment
- Not suitable for CI due to:
  - Variable performance in virtualized environments
  - Resource contention in shared CI runners
  - Inconsistent timing measurements

### All Tests Including Performance
Run with: `cargo test-all`

## Test Organization

- Unit tests: In `src/` files next to implementation
- Integration tests: In `tests/` directory
- Performance tests: In `tests/` directory, marked with `#[ignore]`
- Examples tests: In `eventcore-examples/tests/`

## Performance Testing Guidelines

1. Always run performance tests in release mode
2. Run on consistent hardware
3. Ensure system is not under other load
4. Multiple runs recommended for consistency
5. Compare against baseline measurements

## Known Issues

See `PERFORMANCE_ISSUES.md` for current performance limitations and adjusted targets.