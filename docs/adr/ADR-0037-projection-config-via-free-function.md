# ADR-0037: Projection Configuration via Free Function API

## Status

Accepted (supersedes ADR-0036)

## Date

2026-04-12

## Context

ADR-030 established that `run_projection()` is the primary API for
running projections and that `ProjectionRunner`, `PollMode`,
`PollConfig`, and `EventRetryConfig` are internal implementation
details that should not be part of the public API.

ADR-036 contradicted this by establishing `ProjectionRunner` as the
public API for continuous polling, arguing that adding configuration
to `run_projection()` would "erode that simplicity."

These two decisions conflict: ADR-030 hides the only mechanism
(ProjectionRunner) that ADR-036 designates for continuous mode. This
was discovered during an API cleanup (issue #281) where removing
ProjectionRunner from the public surface deleted continuous polling
functionality entirely.

### The Precedent

`execute()` already follows a pattern that resolves this tension:

```rust
execute(store, command, RetryPolicy::default()).await?;
execute(store, command, RetryPolicy::new(max_retries, backoff)).await?;
```

`RetryPolicy` is a public configuration struct with builder methods
and sensible defaults, passed to a free function. The simple case
remains simple; the configured case is one parameter away. This is
the established pattern in EventCore for free-function configuration.

## Decision

Expose projection configuration through a `ProjectionConfig` struct
passed directly to `run_projection()` as a required parameter. Since
EventCore is pre-1.0, breaking changes are acceptable, and the cleanest
API should win over backward compatibility.

### API Design

```rust
// Batch mode — default config
run_projection(projector, &backend, ProjectionConfig::default()).await?;

// Configured — custom config
let config = ProjectionConfig::default()
    .continuous()
    .poll_interval(Duration::from_millis(200))
    .empty_poll_backoff(Duration::from_millis(50));

run_projection(projector, &backend, config).await?;
```

### `ProjectionConfig` Fields

- **Mode**: Batch (default) or Continuous — set via `.continuous()`
- **Poll interval**: Duration between polls when events are found
- **Empty poll backoff**: Duration to wait when no events are found
- **Poll failure backoff**: Duration to wait after a poll failure
- **Max consecutive poll failures**: Threshold before stopping
- **Event retry max attempts**: Maximum retries for failed events
- **Event retry delay**: Initial delay between retries
- **Event retry backoff multiplier**: Multiplier for exponential backoff
- **Event retry max delay**: Cap on retry delay

All fields have sensible defaults matching current behavior.

### What Stays Internal

- `ProjectionRunner` — internal orchestrator, `pub(crate)`
- `PollMode` — internal enum used by the pipeline
- `PollConfig` / `EventRetryConfig` — internal structs that
  `ProjectionConfig` maps onto

## Rationale

**Why not keep ADR-036 (ProjectionRunner as public API)?**

ADR-036's approach creates the problems ADR-030 identified:

- Two ways to run projections (confusion)
- Internal builder pattern becomes public API (evolution constrained)
- Users must manage leadership acquisition themselves (error-prone)

**Why add the parameter directly to `run_projection()` instead of a
separate function?**

EventCore is pre-1.0 software. Breaking changes are acceptable, and
the cleanest API should take precedence over backward compatibility.
A single `run_projection()` with `ProjectionConfig` is simpler than
maintaining two functions (`run_projection` and
`run_projection_with_config`). `ProjectionConfig::default()` keeps the
simple case ergonomic.

**Why not configuration through the Projector trait?**

ADR-030 suggested retry timing could flow through
`FailureStrategy::Retry { delay }`. While `on_error()` controls
per-event failure strategy, it cannot control:

- Batch vs continuous mode selection
- Poll interval timing
- Poll failure recovery thresholds

These are operational concerns orthogonal to the projector's event
processing logic. A deployment might run the same projector in batch
mode during migrations and continuous mode in production.

## Consequences

### Positive

- Single public API for all projection modes (no confusion)
- `ProjectionRunner` internals can evolve freely
- Leadership acquisition handled automatically (less error-prone)
- Follows the `execute(store, command, policy)` precedent
- Single function instead of two (`run_projection` + `run_projection_with_config`)

### Negative

- One new public type (`ProjectionConfig`) added to the API surface
- Breaking change: callers of the old `run_projection(projector, &backend)` must add `ProjectionConfig::default()` parameter

### Migration from ADR-036

```rust
// Old (ADR-036): ProjectionRunner directly
let _guard = backend.try_acquire(projector.name()).await?;
ProjectionRunner::new(my_projector, &backend)
    .with_poll_mode(PollMode::Continuous)
    .with_poll_config(PollConfig::default())
    .with_checkpoint_store(&backend)
    .run()
    .await?;

// New (ADR-0037): free function with config
let config = ProjectionConfig::default().continuous();
run_projection(my_projector, &backend, config).await?;
```

## Related Decisions

- [ADR-010: Free Function API Design](ADR-010-free-function-api-design.md)
- [ADR-029: Projection Runner API Simplification](ADR-029-projection-runner-api-simplification.md)
- [ADR-030: Layered Crate Public API Design](ADR-030-layered-crate-public-api.md)
- [ADR-036: Continuous Polling via ProjectionRunner](ADR-0036-continuous-polling-via-projection-runner.md) — superseded by this ADR
