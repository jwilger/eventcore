# ADR-0036: Continuous Polling via ProjectionRunner

## Status

Accepted

## Context

The `run_projection()` free function provides a simple API for running
projections in batch mode — process available events once and exit. For
long-running projection processes that need to poll indefinitely,
`PollMode::Continuous` exists on `ProjectionRunner` but is not
accessible through `run_projection()`.

The question: how should continuous polling be exposed through the
public API?

### Options Considered

1. **Add a `PollMode` parameter to `run_projection()`**
2. **Add a separate `run_projection_continuous()` function**
3. **Use `ProjectionRunner` directly for continuous mode** (status quo)

## Decision

Option 3: `ProjectionRunner` is the public API for continuous polling.
`run_projection()` remains the batch-only convenience function.

### Rationale

`run_projection()` exists to minimize ceremony for the common case:
process events, checkpoint, exit. Adding parameters (poll mode, poll
config, backoff settings) would erode that simplicity.

Continuous polling requires configuration that batch mode does not:

- Poll interval between checks for new events
- Backoff strategy when no events are found
- Failure thresholds before stopping
- Shutdown signaling (cancellation token or similar)

These are operational concerns that belong on the builder, not on a
convenience function. `ProjectionRunner` already supports all of them:

```rust
// Batch mode: use the convenience function
run_projection(my_projector, &backend).await?;

// Continuous mode: use ProjectionRunner directly
let _guard = backend.try_acquire(projector.name()).await?;
ProjectionRunner::new(my_projector, &backend)
    .with_poll_mode(PollMode::Continuous)
    .with_poll_config(PollConfig::default())
    .with_checkpoint_store(&backend)
    .run()
    .await?;
```

A separate `run_projection_continuous()` was rejected because it would
need the same configuration parameters as `ProjectionRunner`, making it
a thin wrapper with no added value.

## Consequences

### Positive

- `run_projection()` stays simple — one function, no parameters beyond
  projector and backend
- Continuous mode configuration is explicit and visible at the call site
- No new API surface to maintain
- Existing code and documentation unchanged

### Negative

- Continuous mode requires more ceremony than batch mode (builder pattern)
- Users must manage leadership acquisition (`try_acquire`) themselves
  when using `ProjectionRunner` directly
- Two ways to run projections could cause confusion

### Documentation

- ARCHITECTURE.md should document both paths clearly
- Examples should show the continuous mode pattern
