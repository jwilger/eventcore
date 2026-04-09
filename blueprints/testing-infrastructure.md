---
name: testing-infrastructure
summary: Contract tests, chaos harness, and deterministic testing tools for verifying EventStore backends.
---

# Testing Infrastructure

The `eventcore-testing` crate provides four testing tools: contract test suites that verify backend behavioral contracts, a chaos harness for fault injection, deterministic conflict simulation, and an event collector for integration testing.

## Overview

Every EventStore backend must pass the same contract test suite. The chaos and deterministic tools stress-test retry logic and error handling paths. The EventCollector enables black-box integration testing via the projection system.

## Contract Tests

### Suites

| Suite                | Tests | Verifies                                                                     |
| -------------------- | ----- | ---------------------------------------------------------------------------- |
| EventStore           | 5     | Read/write, version conflicts, stream isolation, atomicity                   |
| EventReader          | 5     | Global ordering, position resumption, prefix filtering, batch limits         |
| CheckpointStore      | 4     | Save/load, update overwrite, missing returns None, subscription independence |
| ProjectorCoordinator | 4     | Leadership acquisition, blocking, independence, guard-drop release           |

### Usage

The `backend_contract_tests!` macro generates all 18 tests for a backend:

```rust
backend_contract_tests!(postgres, || async { make_postgres_store().await });
```

Each test uses UUID-based stream names for isolation — no test cleanup needed.

## Chaos Testing

`ChaosEventStore<S>` wraps any EventStore and probabilistically injects failures:

- `failure_probability` — Injects `StoreFailure` on read or append
- `version_conflict_probability` — Injects `VersionConflict` on append (checked first)
- `deterministic_seed` — Optional seed for reproducible failure sequences

Use case: stress-testing retry policies and error handling under realistic failure conditions.

## Deterministic Conflict Testing

`DeterministicConflictStore<S>` wraps any EventStore and injects exactly N version conflicts:

```rust
let store = DeterministicConflictStore::new(inner_store, 3);
// First 3 appends fail with VersionConflict, then delegates to inner store
```

Use case: testing that retry loops handle a known conflict count correctly.

## Event Collector

`EventCollector<E>` implements `Projector` to accumulate events for assertion:

```rust
let collector = EventCollector::<MyEvent>::new();
run_projection(collector.clone(), &backend).await?;
assert_eq!(collector.events(), expected);
```

Infallible (Error = `Infallible`), suitable for any integration test.

## Files

| File                                       | Description                                                  |
| ------------------------------------------ | ------------------------------------------------------------ |
| `eventcore-testing/src/contract.rs`        | 18 contract test functions + `backend_contract_tests!` macro |
| `eventcore-testing/src/chaos.rs`           | ChaosEventStore, ChaosConfig, probability types              |
| `eventcore-testing/src/deterministic.rs`   | DeterministicConflictStore                                   |
| `eventcore-testing/src/event_collector.rs` | EventCollector projector                                     |

## Related Systems

- [store-backends](store-backends.md) — Backends validated by these contract tests
- [command-execution](command-execution.md) — Retry logic stress-tested by chaos/deterministic tools
- [projection-system](projection-system.md) — EventCollector uses the Projector trait
- ADR-013: EventStore contract testing
- ADR-015: Testing crate scope
- ADR-031: Black-box integration testing via projections
- ADR-032: Integration test crate
