---
name: projection-system
summary: Poll-based projection runner with checkpoint resumption, leader election, and configurable retry.
---

# Projection System

Builds read models by consuming the global event stream. Projections are poll-based, checkpoint-resumable, and coordinated via single-leader election to prevent duplicate processing.

## Overview

The projection system implements the "Q" side of CQRS. Projectors consume events from the global stream, apply them to read models, and checkpoint their progress. A coordination layer ensures only one instance of each projector runs at a time in distributed deployments.

## Architecture

### Projection Flow

```
run_projection(projector, backend)
  │
  ├── try_acquire(projector.name()) → leadership guard
  ├── load checkpoint → Option<StreamPosition>
  │
  └── Poll loop:
      ├── read_events(filter, page_after_checkpoint)
      ├── For each (event, position):
      │   ├── projector.apply(event, position, context)
      │   ├── On success: save checkpoint
      │   ├── On error: projector.on_error(FailureContext)
      │   │   ├── Fatal → stop, return error
      │   │   ├── Skip → log, save checkpoint, continue
      │   │   └── Retry → backoff, retry event
      │   └── Continue
      ├── If Batch mode: exit after one poll
      └── If Continuous mode: sleep (poll_interval or empty_poll_backoff)
```

### Key Traits

**Projector** — Event consumer:

- `apply(event, position, context)` — Process single event
- `name()` — Unique identifier for checkpointing and coordination
- `on_error(FailureContext)` — Decide failure strategy (Fatal/Skip/Retry)

**CheckpointStore** — Progress tracking:

- `load(name)` → `Option<StreamPosition>`
- `save(name, position)` — Persist after successful processing

**ProjectorCoordinator** — Leader election:

- `try_acquire(subscription_name)` → Guard or error
- Non-blocking (ADR-028): returns immediately, caller decides what to do on failure
- Guard released on drop

### Configuration

**ProjectionConfig** (public builder):

- `continuous()` — Switch to continuous polling mode (default: batch)
- `poll_interval(Duration)` — Delay between polls when events were found
- `empty_poll_backoff(Duration)` — Delay when no events found (longer to reduce load)
- `poll_failure_backoff(Duration)` — Delay after poll failure
- `max_consecutive_poll_failures(MaxConsecutiveFailures)` — Infrastructure failure threshold
- `event_retry_max_attempts(MaxRetryAttempts)` — Per-event retry limit
- `event_retry_delay(Duration)` — Initial backoff delay
- `event_retry_backoff_multiplier(BackoffMultiplier)` — Exponential growth factor (≥ 1.0)
- `event_retry_max_delay(Duration)` — Backoff cap

Internal types (`PollConfig`, `EventRetryConfig`, `PollMode`) are not exposed;
`ProjectionConfig` translates builder settings into these internal types.

### Failure Strategy

| Strategy | Behavior                                                     |
| -------- | ------------------------------------------------------------ |
| `Fatal`  | Stop projection, return error                                |
| `Skip`   | Log poisoned event, checkpoint past it, continue             |
| `Retry`  | Exponential backoff, retry up to max attempts, then escalate |

## Files

| File                                | Description                                                                       |
| ----------------------------------- | --------------------------------------------------------------------------------- |
| `eventcore/src/projection.rs`       | ProjectionConfig, run_projection(), run_projection_with_config(), internal runner |
| `eventcore-types/src/projection.rs` | Projector, EventReader, CheckpointStore, ProjectorCoordinator traits              |

## Public API (ADR-0037)

Two paths for running projections:

**Batch mode** — `run_projection()` convenience function:

```rust
run_projection(my_projector, &backend).await?;
```

Handles leadership acquisition, processes events once, exits. No
configuration needed.

**Configurable mode** — `run_projection_with_config()` with `ProjectionConfig`:

```rust
use std::time::Duration;
use eventcore::{ProjectionConfig, run_projection_with_config};

let config = ProjectionConfig::default()
    .continuous()
    .poll_interval(Duration::from_millis(200))
    .event_retry_max_attempts(MaxRetryAttempts::new(5));

run_projection_with_config(my_projector, &backend, config).await?;
```

`ProjectionConfig` exposes all poll and retry knobs via builder methods.
Handles leadership acquisition automatically. Both batch and continuous
modes are supported.

**Internal implementation**: `ProjectionRunner` is a crate-internal struct
used by `run_projection_with_config()`. It is not part of the public API.
`PollConfig`, `PollMode`, `EventRetryConfig`, and `NoCheckpointStore` are
also internal types.

## Related Systems

- [event-sourcing](event-sourcing.md) — Event stream being consumed
- [store-backends](store-backends.md) — Backend implementations of EventReader, CheckpointStore, ProjectorCoordinator
- [testing-infrastructure](testing-infrastructure.md) — EventCollector projector for testing
- ADR-019: Projector trait
- ADR-021: Poll-based projector design
- ADR-024: Projector configuration
- ADR-028: Non-blocking advisory lock acquisition
- ADR-029: Projection runner API simplification
- ADR-030: Layered API surface for application vs. backend developers
- ADR-036: Continuous polling via ProjectionRunner
- ADR-037: ProjectionConfig via free function
