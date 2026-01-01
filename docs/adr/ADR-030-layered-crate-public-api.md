# ADR-030: Layered Crate Public API Design

## Status

Accepted

## Date

2025-12-31

## Context

EventCore's crate structure has evolved organically, resulting in unclear boundaries between what application developers need versus what backend implementers need. The current state has several problems:

**Problem 1: API Surface Bloat**

The main `eventcore` crate could re-export almost everything from `eventcore-types`, exposing backend-implementer types to application developers who don't need them. Types like `EventStoreError`, `EventStreamReader`, `EventStreamSlice`, `StreamWrites`, and `StreamVersion` are implementation details for anyone using EventCore to build applications.

**Problem 2: Unclear Audience**

No clear distinction exists between:
- "Use this to build applications" (the primary use case)
- "Use this to build EventStore backends" (the secondary use case)

Application developers should see a minimal, focused API. Backend implementers need access to all the traits and types required to implement `EventStore`, `EventReader`, `CheckpointStore`, and `ProjectorCoordinator`.

**Problem 3: Harder to Evolve**

A large public API surface constrains future changes. Every public type creates a backwards compatibility obligation. Types that shouldn't be public create unnecessary constraints on internal refactoring.

**Current State Analysis:**

Looking at `eventcore/src/lib.rs`, the crate currently:
- Exports `run_projection` (good - primary API)
- Exports `execute` (good - primary API)
- Exports `Command` derive macro and `require!` macro (good - application infrastructure)
- Exports retry configuration types: `RetryPolicy`, `BackoffStrategy`, `MetricsHook`, `RetryContext`, `ExecutionResponse` (good - command execution concerns)
- Re-exports `eventcore_postgres` as `postgres` feature-gated (good - backend access)
- Uses types from `eventcore-types` internally without re-exporting them all (currently correct)

The key insight: **the current `lib.rs` is already close to the right design**. The ADR codifies this approach and establishes principles to maintain it.

**ADR-010 and ADR-029 Alignment:**

ADR-010 established that free functions are the primary API style. ADR-029 established `run_projection(projector, &backend)` as the preferred entry point for projections. This ADR extends those decisions to define what should and shouldn't be exported.

## Decision

Establish a layered crate architecture with strict separation of concerns:

### Layer 1: `eventcore` Crate (Application Developers)

The main crate exports only what application developers need to write commands and projectors.

**Primary Entry Points:**
- `execute()` - Run commands against an event store
- `run_projection()` - Run projectors against a backend

**Command Infrastructure:**
- `RetryPolicy` - Configure retry behavior
- `BackoffStrategy` - Configure backoff timing
- `ExecutionResponse` - Command execution result
- `MetricsHook` - Integration with metrics systems
- `RetryContext` - Context passed to metrics hooks

**Macros:**
- `Command` derive macro (via `eventcore-macros`)
- `require!` macro for business rule validation

**Re-exports for Command/Projector Implementation:**
- From `eventcore-types`: `CommandLogic`, `CommandStreams`, `CommandError`, `Event`, `StreamId`, `StreamDeclarations`, `NewEvents`, `StreamResolver`
- From `eventcore-types`: `Projector`, `FailureContext`, `FailureStrategy`, `StreamPosition`
- `DelayMilliseconds`, `AttemptNumber` (needed for `RetryPolicy` and `RetryContext`)

**Feature-Gated Backends:**
- `postgres` feature enables `eventcore::postgres` module

### Layer 2: `eventcore-types` Crate (Backend Implementers)

This crate exports everything needed to implement EventStore backends and other infrastructure.

**Traits for Implementation:**
- `EventStore` - Core event storage abstraction
- `EventReader` - Read events across streams
- `CheckpointStore` - Track projection progress
- `ProjectorCoordinator` - Leader election for projectors

**Types for Trait Implementations:**
- `EventStoreError`, `Operation` - Error handling
- `EventStreamReader`, `EventStreamSlice` - Event reading results
- `StreamWrites`, `StreamWriteEntry`, `StreamVersion` - Event writing
- `EventFilter`, `EventPage` - Event querying
- `StreamPrefix` - Stream pattern matching

**Configuration Types (Internal Use):**
- `MaxRetries`, `BatchSize`, `MaxConsecutiveFailures`, `MaxRetryAttempts`
- `BackoffMultiplier`, `RetryCount`

### NOT Exported from `eventcore` (Internal Implementation Details)

The following are internal machinery that should not be part of the public API:

- `ProjectionRunner` - Internal orchestration; use `run_projection()` instead
- `PollMode`, `PollConfig` - Operational tuning, not application code
- `EventRetryConfig` - Retry timing controlled by `Projector::on_error()` return values
- `NoCheckpointStore` - Internal null object pattern

### Key Principles

1. **Conservative Exports**: It's easier to add public types than to remove them. Start minimal.

2. **Audience Separation**: Clear distinction between:
   - Application developer API (`eventcore`)
   - Backend implementer API (`eventcore-types`)

3. **Free Functions Over Structs**: `execute()` and `run_projection()` are the API, not builder or runner structs.

4. **Projector Owns Its Config**: Retry timing and failure strategies are projector concerns, expressed through `Projector::on_error()` returning `FailureStrategy`.

5. **Operational Concerns Are External**: Poll intervals, batch sizes, and similar tuning parameters are deployment concerns, not application code. They belong in environment configuration or backend construction, not in the main API.

## Rationale

**Why Remove `ProjectionRunner` from Public API:**

`run_projection(projector, &backend)` provides everything users need. There is no "advanced scenario" that requires direct runner access that cannot be achieved through the free function API. Exposing `ProjectionRunner`:
- Invites misuse (constructing runners incorrectly)
- Constrains future changes (runner internals become public API)
- Creates confusion (two ways to do the same thing)

ADR-029 already established `run_projection` as the preferred API.

**Why Remove `PollMode`, `PollConfig`, `EventRetryConfig`:**

These are operational tuning concerns, not application logic:
- Poll timing should be environment-dependent (different in dev vs prod)
- Retry timing for event processing is controlled by `FailureStrategy::Retry` from `Projector::on_error()`
- If more granular retry timing is needed, it can be added to `FailureStrategy::Retry { delay }` without exposing internal config structs

Application code should not encode operational parameters.

**Why Strict Separation Matters:**

1. **Integration tests are clearer**: When tests need backend types, they import from `eventcore-types` with an explicit comment like "test infrastructure". Real app code only imports from `eventcore`.

2. **Backend implementations are stable**: Backend implementers have a clear, stable target (`eventcore-types` traits). Changes to `eventcore` internals don't affect backends.

3. **Evolution is easier**: Internal refactoring (e.g., changing how `ProjectionRunner` works) doesn't break public API.

**Why This Aligns with ADR-010:**

ADR-010 established:
- Free functions as primary API style
- Types made public only when compiler or testing requires it
- Explicit dependencies in function signatures

This ADR applies those principles to crate-level organization.

## Consequences

### Positive

- **Minimal API surface** for application developers
- **Clear guidance** on which crate to use for which purpose
- **Backend implementations decoupled** from main crate internals
- **Easier internal evolution** without breaking changes
- **Better documentation** - smaller API is easier to document
- **Faster compilation** - fewer public types means less code generation

### Negative

- **Dual imports in tests** - tests need explicit separation of concerns

Note: While this represents a breaking change to the public API, EventCore is pre-1.0.0 and follows semantic versioning. Breaking changes are expected and inconsequential at this stage - they require only a minor version bump (0.x → 0.y).

### Neutral

- Three crates to understand instead of one (but with clear purposes)
- More explicit imports (but clearer intent)

### Migration Path

For users affected by removed exports (note: minimal impact expected given pre-1.0 status):

1. **Types now in `eventcore-types` only**: Add `use eventcore_types::{...}` to imports

2. **Types removed entirely** (e.g., `ProjectionRunner`, `PollConfig`):
   - Use `run_projection()` instead of constructing runners directly
   - Move operational config to environment/backend construction

## Alternatives Considered

### Alternative 1: Single Crate with `#[doc(hidden)]`

**Description**: Keep everything in one crate but hide internal types from docs.

**Pros**:
- Simpler crate structure
- No import changes needed

**Cons**:
- Hidden types are still public API (breaking changes still break)
- Doesn't solve the audience confusion problem
- Rust community considers `#[doc(hidden)]` a weak API boundary

**Why Rejected**: `#[doc(hidden)]` doesn't provide real API boundaries.

### Alternative 2: Feature Flags for Internal Types

**Description**: Gate internal types behind a `backend-impl` feature flag.

**Pros**:
- Single crate
- Explicit opt-in for backend types

**Cons**:
- Feature flags affect dependency resolution globally
- Doesn't match Rust ecosystem norms (traits in separate crates)
- Complex feature interactions

**Why Rejected**: Separate crates are the idiomatic Rust approach.

### Alternative 3: Re-export Everything

**Description**: Accept the current state and document which types are for which audience.

**Pros**:
- No breaking changes
- Simple mental model (one crate has everything)

**Cons**:
- API bloat persists
- Evolution remains constrained
- Documentation burden

**Why Rejected**: Violates ADR-010's principle of minimal public API surface.

## Related Decisions

- [ADR-010: Free Function API Design Philosophy](ADR-010-free-function-api-design.md) - Establishes free functions as primary API style
- [ADR-029: Projection Runner API Simplification](ADR-029-projection-runner-api-simplification.md) - Establishes `run_projection` as preferred entry point
- [ADR-022: Crate Reorganization for Feature Flags](ADR-022-crate-reorganization-for-feature-flags.md) - Original crate split motivation

## References

- Rust API Guidelines: https://rust-lang.github.io/api-guidelines/
- Semver compatibility in Rust: https://doc.rust-lang.org/cargo/reference/semver.html
