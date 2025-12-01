# EventCore Architecture

**Document Version:** 1.4
**Date:** 2025-11-30
**Phase:** 4 - Architecture Synthesis

## Overview

EventCore is a type-driven event sourcing library for Rust that delivers atomic multi-stream commands, optimistic concurrency, and first-class developer ergonomics. The architecture below is a faithful projection of every accepted architectural decision record (ADR). Each section captures the final, current design after applying the ADRs in chronological order—no cross references to the ADRs themselves are required to understand the system.

## Architectural Principles

1. **Type-Driven Development** – All externally visible APIs express domain constraints in their signatures. Domain concepts use validated newtypes (e.g., `StreamId`, `EventId`, `CorrelationId`) constructed via smart constructors, ensuring “parse, don’t validate” semantics. Phantom types and typestate patterns make illegal states unrepresentable. Total functions and structured errors replace panics.
2. **Correctness over Throughput** – Multi-stream atomicity, optimistic concurrency detection, and immutability are non-negotiable. Performance optimizations must preserve these guarantees and therefore happen _within_ atomic transaction boundaries provided by the backing store.
3. **Infrastructure Neutrality** – The library owns infrastructure concerns (stream management, retries, metadata, storage abstraction) and never assumes a particular business domain. Applications own their domain events, metadata schemas, and business rules.
4. **Free-Function APIs** – Public entry points are free functions with explicit dependencies (`execute(command, store)`), keeping the API surface minimal and composable. Structs exist only when grouping configuration or results adds clarity.
5. **Developer Ergonomics** – The `#[derive(Command)]` macro generates all infrastructure boilerplate. Developers write only domain code (state reconstruction + business logic). Automatic retries, contract-test tooling, and in-memory storage are included in the main crate to support a “working command in 30 minutes” onboarding goal.

## System Blueprint

```mermaid
graph TB
    App[Application Code]
    ExecFn[execute() function]
    Cmd[Command System]
    Events[Event System]
    Store[Event Store Abstraction]
    Backend[Storage Backend]

    App -->|execute(command, store)| ExecFn
    ExecFn -->|resolve & queue streams| Cmd
    ExecFn -->|fold domain events| Events
    Events -->|trait bounds| Store
    Store -->|atomic append| Backend
    Cmd -->|apply/handle| App

    subgraph Type System
        Types[Validated Domain Types]
        Errors[Error Hierarchy]
        Meta[Event Metadata]
    end

    ExecFn -.->|uses| Types
    ExecFn -.->|emits| Errors
    Store -.->|preserves| Meta
    Events -.->|domain types implement| Types

    style ExecFn fill:#e1f5ff
    style Store fill:#e1ffe1
    style Cmd fill:#ffe1e1
    style Events fill:#fff3cd
```

## Event Store Abstraction

### Responsibilities

The `EventStore` trait exposes two core operations:

1. `read_stream` / `read_streams` – fetch all events for one or more streams, returning both events and the current stream versions.
2. `append_events` – atomically register new streams (if needed) and append events to one or more streams while verifying expected versions.

A separate `EventSubscription` trait provides long-lived projection feeds (poll or push). Subscriptions are optional; not every backend must implement them.

### Atomicity and Transactions

- Multi-stream atomicity is achieved by delegating to the backend’s native transaction mechanism (PostgreSQL ACID transactions, in-memory mutexes, etc.).
- Append operations are “all-or-nothing” across every stream referenced in a command.
- Backend implementations hide their transaction mechanics; the trait merely promises atomic semantics.

### Versioning & Optimistic Concurrency

- Each stream tracks a monotonically increasing `StreamVersion` starting at 0. Every appended event increments the version by 1.
- During Phase 2 of execution, the executor captures the version for each stream it reads.
- During Phase 5, `append_events` receives the entire map of expected versions and atomically verifies **all** of them. A mismatch on any stream yields `EventStoreError::VersionConflict` (classified as retriable) and no events are written.
- Version verification occurs inside the backend transaction to eliminate TOCTOU races.

### Metadata & Ordering

All persisted events carry immutable metadata:

| Field                        | Purpose                                                              |
| ---------------------------- | -------------------------------------------------------------------- |
| `EventId (UUIDv7)`           | Globally ordered identity for cross-stream projections and debugging |
| `StreamId` & `StreamVersion` | Aggregate identity and per-stream ordering                           |
| `Timestamp`                  | Commit time (not command start time)                                 |
| `CorrelationId`              | Logical operation identifier (stable across retries)                 |
| `CausationId`                | Immediate trigger of the event (usually the command)                 |
| `CustomMetadata<M>`          | Application-defined, strongly typed metadata payload                 |

Metadata is validated at construction time, persisted verbatim by every backend, and never mutated after commit.

### Storage Implementations

- **InMemoryEventStore** ships inside the main crate with zero third-party dependencies. It is the default for tests, tutorials, and quickstarts.
- **Production backends** (e.g., PostgreSQL) live in separate crates to avoid imposing heavy dependencies on every user. They implement `EventStore` and, when applicable, `EventSubscription`.
- All implementations must support chaos testing hooks (e.g., injected conflicts) and optional instrumentation for observability.

### Contract Testing

A reusable contract test suite (`eventcore_testing::event_store_contract_tests`) verifies that implementations:

- Detect version conflicts under concurrent writes (single and multi-stream scenarios).
- Enforce atomicity—either all streams are updated or none are.
- Preserve metadata and ordering guarantees.

Every backend (first-party or third-party) integrates these tests into its CI pipeline to guarantee semantic compliance.

All of these helpers live in the dedicated `eventcore-testing` crate, which depends on the main `eventcore` library but stays out of release builds unless consumers add it as a dev-dependency. This keeps the production crate lean while still providing rich testing utilities.

> **Status:** As of this PR the crate only ships chaos utilities; the contract suite is on the roadmap and will arrive in a future release.

## Event System & Metadata

### Domain-First Event Trait

Domain events implement the simple `Event` trait:

```rust
pub trait Event: Clone + Send + 'static {
    fn stream_id(&self) -> &StreamId;
}
```

- Developers model events as plain structs with owned data. No infrastructure wrapper is required.
- The `'static` bound ensures events are self-contained values suitable for storage, async boundaries, and cross-thread movement.
- `EventStore::read_stream` and command logic operate on these domain types directly.

### Metadata Pipeline

- Standard metadata fields (IDs, versions, timestamps, tracing IDs) are handled by infrastructure when events are persisted.
- Applications supply strongly typed custom metadata `M: Serialize + DeserializeOwned` to capture audit information (actors, IP addresses, etc.) without violating infrastructure neutrality.
- Metadata records are immutable facts; changes require emitting compensating events rather than editing existing ones.

## Command Model

### Macro-Generated Infrastructure

The `#[derive(Command)]` macro turns annotated struct fields into full command infrastructure:

```rust
#[derive(Command)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,
    #[stream]
    to_account: StreamId,
    amount: Money,
}
```

The macro produces:

- A phantom `StreamSet` type encoding the declared streams.
- An implementation of `CommandStreams` that surfaces stream declarations to the executor.
- Compile-time enforcement that only declared streams can be targeted via `StreamWrite<StreamSet, Event>` and the `emit!` macro.

### CommandLogic Trait

Developers implement `CommandLogic` for domain behavior:

```rust
impl CommandLogic for TransferMoney {
    type State = AccountPairState;
    type Event = AccountEvent;

    fn apply(&self, mut state: Self::State, event: &Self::Event) -> Self::State { /* ... */ }

    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        require!(state.can_transfer(self.amount), "Insufficient funds");
        emit!(state.ctx, AccountDebited { /* ... */ });
        emit!(state.ctx, AccountCredited { /* ... */ });
        Ok(state.ctx.into())
    }

    fn stream_resolver(&self) -> Option<&dyn StreamResolver<Self::State>> {
        None
    }
}
```

- `apply` reconstructs state by folding historical events.
- `handle` validates business rules and produces new domain events using the type-safe `emit!` helper.
- `stream_resolver` is optional; commands needing runtime discovery return `Some(self)` (or another resolver) to opt into dynamic loading.

### Dynamic Stream Discovery

When commands implement `StreamResolver<State>`, the executor:

1. Seeds a `VecDeque<StreamId>` with statically declared streams.
2. Maintains `scheduled` and `visited` hash sets to deduplicate work.
3. Pops a stream ID, reads it exactly once, folds events, and records the stream’s version.
4. Invokes `discover_related_streams(&state)` to enqueue additional stream IDs discovered from reconstructed state.
5. Continues until the queue is empty, ensuring both static and discovered streams participate in optimistic concurrency.

This queue-based approach eliminates the multi-pass re-read loop while guaranteeing deterministic ordering and state completeness.

## Command Execution Pipeline

The primary API is the async free function `execute(command, store)`. Each attempt runs five deterministic phases:

1. **Stream Resolution** – Ask the command for its static stream declarations and seed the dynamic discovery queue.
2. **Read & Version Capture** – Drain the queue, reading each stream exactly once, folding events into state, and building the expected-version map.
3. **State Reconstruction** – After all required streams have been read, the accumulated state represents a consistent snapshot for the command.
4. **Business Logic** – Invoke `CommandLogic::handle`, producing `NewEvents` (potentially empty) or returning `CommandError` for validation/business rule failures.
5. **Atomic Append** – Write all emitted events using the captured expected versions. Any mismatch triggers `EventStoreError::VersionConflict`.

### Automatic Retry & Backoff

If Phase 5 returns a concurrency error:

- The executor consults the configurable `RetryPolicy` (max attempts, base delay, multiplier, optional jitter).
- After waiting for the computed backoff, execution restarts from Phase 2 with a fresh queue and state; correlation and causation IDs remain unchanged so tracing reflects a single logical operation.
- Permanent errors (validation failures, business rule violations, non-retriable storage errors) short-circuit and return immediately with enriched context.

### Metadata Continuity

- Correlation IDs are generated once per `execute` call and preserved across retries.
- Causation IDs typically use the command identifier and never change.
- Commit timestamps reflect when events are successfully persisted, not when execution began.

### Observability Hooks

- Each phase emits structured logs and metrics (e.g., read durations, queue depth, retry counts, version conflict rates).
- Backoff decisions expose telemetry for contention analysis.
- Correlation/causation IDs tie command execution to surrounding telemetry.

## Type System Patterns

- **Validated Newtypes** – `StreamId`, `EventId`, `CorrelationId`, `Money`, etc., enforce invariants at construction time via the `nutype` crate.
- **Phantom Types & Typestate** – `StreamWrite<StreamSet, Event>` enforces compile-time stream access control; `NewEvents` carries the same phantom to ensure only declared streams receive emissions.
- **Total Functions** – Public APIs return `Result` instead of panicking. Error enums derive `thiserror` and support pattern matching.
- **Trait Composition** – Narrow traits (`CommandStreams`, `CommandLogic`, `StreamResolver`) keep responsibilities focused and implementations testable.

## Error Handling

- **CommandError** – Categorizes domain failures (validation, business rule violations, infrastructure issues surfaced to commands). Business rule violations are permanent by design.
- **EventStoreError** – Represents storage-layer failures (version conflicts, connectivity, serialization). Version conflicts map to `ConcurrencyError` and are retriable.
- **Validation Errors** – Raised by newtype constructors and automatically bubbled up through `CommandError`.
- **Retry Classification** – Errors implement marker traits (or equivalent metadata) indicating whether they are retriable or permanent. The executor consults this classification before attempting any retry.
- **Context Enrichment** – Errors carry correlation ID, causation ID, stream identifiers, and diagnostic details to aid debugging and distributed tracing.

## Reference Implementations & Tooling

- **InMemoryEventStore** is included in `eventcore` and used across documentation, examples, and internal tests. It supports optional chaos hooks (e.g., `ConflictOnceStore`, `CountingEventStore`) for scenario-driven testing.
- **External Backends** (e.g., `eventcore-postgres`) implement the same traits, run the contract test suite, and may offer additional observability or operational features.
- **Testing Utilities** – The crate exposes helpers for property-based testing, contract verification, and integration scenarios so downstream users can exercise real command flows without managing infrastructure.

## Putting It All Together

By following the flow above, applications gain:

1. Type-safe domain modeling with zero boilerplate for infrastructure.
2. Deterministic, atomic execution of complex multi-stream business operations.
3. Automatic concurrency management and retry behavior that keeps business code simple.
4. Rich metadata and observability hooks for auditing, compliance, and debugging.
5. Pluggable storage backends validated by a shared contract-suite, ensuring every implementation honors the same semantic guarantees.

This document is the single source of truth for EventCore’s architecture; ADRs capture how we arrived here, while this blueprint describes the system as it stands today.
