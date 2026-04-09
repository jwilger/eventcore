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

**PollConfig:**

- `poll_interval` — Delay between polls when events were found
- `empty_poll_backoff` — Delay when no events found (longer to reduce load)
- `max_consecutive_poll_failures` — Infrastructure failure threshold before giving up

**EventRetryConfig:**

- `max_retry_attempts` — Per-event retry limit
- `base_retry_delay` — Initial backoff delay
- `backoff_multiplier` — Exponential growth factor (≥ 1.0)
- `max_retry_delay` — Backoff cap

**PollMode:**

- `Batch` — Process once and exit (useful for testing and one-shot projections)
- `Continuous` — Poll forever with configurable sleep intervals

### Failure Strategy

| Strategy | Behavior                                                     |
| -------- | ------------------------------------------------------------ |
| `Fatal`  | Stop projection, return error                                |
| `Skip`   | Log poisoned event, checkpoint past it, continue             |
| `Retry`  | Exponential backoff, retry up to max attempts, then escalate |

## Files

| File                                | Description                                                          |
| ----------------------------------- | -------------------------------------------------------------------- |
| `eventcore/src/projection.rs`       | ProjectionRunner, PollConfig, EventRetryConfig, run_projection()     |
| `eventcore-types/src/projection.rs` | Projector, EventReader, CheckpointStore, ProjectorCoordinator traits |

## Related Systems

- [event-sourcing](event-sourcing.md) — Event stream being consumed
- [store-backends](store-backends.md) — Backend implementations of EventReader, CheckpointStore, ProjectorCoordinator
- [testing-infrastructure](testing-infrastructure.md) — EventCollector projector for testing
- ADR-019: Projector trait
- ADR-021: Poll-based projector design
- ADR-024: Projector configuration
- ADR-028: Non-blocking advisory lock acquisition
- ADR-029: Projection runner API simplification
