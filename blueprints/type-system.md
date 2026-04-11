---
name: type-system
summary: Semantic domain types with nutype validation enforcing parse-don't-validate at construction boundaries.
---

# Type System

Domain types that make illegal states unrepresentable. Every concept in the event sourcing model has a semantic named type with compile-time or construction-time validation via nutype.

## Overview

The type system lives in `eventcore-types` and is shared across all crates. It enforces invariants through the type system rather than runtime checks: once a `StreamId` is constructed, it is guaranteed valid. No downstream code re-validates.

## Domain Types

### Stream Identity

| Type             | Validation                                             | Purpose                                 |
| ---------------- | ------------------------------------------------------ | --------------------------------------- |
| `StreamId`       | Non-empty, trimmed, ≤255 chars, no glob chars (`*?[]`) | Identifies an event stream              |
| `StreamPrefix`   | Non-empty, trimmed, ≤255 chars                         | Filters streams by starts-with matching |
| `StreamVersion`  | `usize`, monotonic via `increment()`                   | Per-stream event count for concurrency  |
| `StreamPosition` | `Uuid` (UUIDv7, timestamp-ordered)                     | Global event ordering for projections   |

### Command Types

| Type                 | Purpose                                     |
| -------------------- | ------------------------------------------- |
| `StreamDeclarations` | Validated collection of ≥1 unique StreamIds |
| `NewEvents<E>`       | Wrapped `Vec<E>` output from `handle()`     |

### Configuration Types

| Type                         | Validation         | Purpose                              |
| ---------------------------- | ------------------ | ------------------------------------ |
| `BatchSize`                  | `usize`            | Pagination limit for event reads     |
| `MaxRetries`                 | `u32`              | Retry limit for command execution    |
| `MaxRetryAttempts`           | `u32`              | Per-event retry limit in projections |
| `DelayMilliseconds`          | `u64`              | Backoff delay values                 |
| `AttemptNumber`              | `NonZeroU32`       | 1-based attempt counter              |
| `RetryCount`                 | `u32`              | 0-based retry counter                |
| `BackoffMultiplier`          | `f64 ≥ 1.0`        | Exponential backoff growth factor    |
| `MaxConsecutiveFailures`     | `NonZeroU32`       | Poll failure threshold               |
| `FailureProbability`         | `f32 ∈ [0.0, 1.0]` | Chaos testing injection rate         |
| `VersionConflictProbability` | `f32 ∈ [0.0, 1.0]` | Chaos testing conflict rate          |

## Design Principles

### Parse-Don't-Validate

Types are validated at construction boundaries only. A `StreamId` value is proof of validity — no code downstream of construction checks `is_valid()` or re-validates.

### nutype Convention

All domain newtypes use the `nutype` crate:

```rust
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255, predicate = no_glob_metacharacters),
    derive(Clone, PartialEq, Eq, Hash, Display, Serialize, Deserialize, AsRef, Deref)
)]
pub struct StreamId(String);
```

Benefits: private inner field, fallible constructor, standard trait derives, no manual boilerplate.

### Domain Operations Over Inner Value Extraction

Domain types should expose operations (arithmetic, comparison, formatting)
rather than requiring callers to extract the inner value. `into_inner()`
and `Into` conversions should appear only at IO boundaries (SQL parameter
binding, logging format arguments) — never in domain logic or test
assertions.

When a domain type needs arithmetic, implement `From<DomainType> for TargetType`
or domain methods (e.g., `AccountBalance::deposit(amount)`). When sorting
is needed, derive `PartialOrd` and `Ord`. Collections should hold domain
types, not extracted primitives.

### Validation Functions

Custom validation predicates (e.g., `no_glob_metacharacters`) must have property tests via proptest.

## Files

| File                                | Description                                                           |
| ----------------------------------- | --------------------------------------------------------------------- |
| `eventcore-types/src/store.rs`      | StreamId, StreamVersion, StreamWrites, EventStore errors              |
| `eventcore-types/src/command.rs`    | Event trait, CommandLogic, StreamDeclarations, NewEvents              |
| `eventcore-types/src/projection.rs` | StreamPosition, EventPage, EventFilter, BatchSize, retry config types |
| `eventcore-types/src/validation.rs` | `no_glob_metacharacters` predicate                                    |
| `eventcore-types/src/errors.rs`     | CommandError enum                                                     |

## Related Systems

- [event-sourcing](event-sourcing.md) — Types used throughout the event sourcing model
- [macro-codegen](macro-codegen.md) — Macros that generate code referencing these types
- ADR-003: Type system patterns
- ADR-017: StreamId reserved characters
