# EventCore Architecture

**Document Version:** 1.7
**Date:** 2025-12-14
**Phase:** 4 - Architecture Synthesis

## Overview

EventCore is a type-driven event sourcing library for Rust that delivers atomic multi-stream commands, optimistic concurrency, and first-class developer ergonomics. The architecture below is a faithful projection of every accepted architectural decision record (ADR). Each section captures the final, current design after applying the ADRs in chronological order—no cross references to the ADRs themselves are required to understand the system.

## Architectural Principles

1. **Type-Driven Development** – All externally visible APIs express domain constraints in their signatures. Domain concepts use validated newtypes (e.g., `StreamId`, `EventId`, `CorrelationId`) constructed via smart constructors, ensuring "parse, don't validate" semantics. Phantom types and typestate patterns make illegal states unrepresentable. Total functions and structured errors replace panics.
2. **Correctness over Throughput** – Multi-stream atomicity, optimistic concurrency detection, and immutability are non-negotiable. Performance optimizations must preserve these guarantees and therefore happen _within_ atomic transaction boundaries provided by the backing store.
3. **Infrastructure Neutrality** – The library owns infrastructure concerns (stream management, retries, metadata, storage abstraction) and never assumes a particular business domain. Applications own their domain events, metadata schemas, and business rules.
4. **Free-Function APIs** – Public entry points are free functions with explicit dependencies (`execute(command, store)`), keeping the API surface minimal and composable. Structs exist only when grouping configuration or results adds clarity.
5. **Developer Ergonomics** – The `#[derive(Command)]` macro generates all infrastructure boilerplate. Developers write only domain code (state reconstruction + business logic). Automatic retries, contract-test tooling, and in-memory storage are included in the main crate to support a "working command in 30 minutes" onboarding goal.

## System Blueprint

```mermaid
graph TB
    App[Application Code]
    ExecFn[execute() function]
    Cmd[Command System]
    Events[Event System]
    Store[Event Store Abstraction]
    Backend[Storage Backend]
    SubTrait[Event Subscription Abstraction]
    Projections[Projection/Read Model Builders]

    App -->|execute(command, store)| ExecFn
    ExecFn -->|resolve & queue streams| Cmd
    ExecFn -->|fold domain events| Events
    Events -->|trait bounds| Store
    Store -->|atomic append| Backend
    Cmd -->|apply/handle| App

    App -->|subscribe(query, coordinator)| SubTrait
    SubTrait -->|futures::Stream delivery| Projections
    SubTrait -->|coordinate assignments| Backend
    Projections -->|checkpoint progress| SubTrait

    subgraph Type System
        Types[Validated Domain Types]
        Errors[Error Hierarchy]
        Meta[Event Metadata]
    end

    ExecFn -.->|uses| Types
    ExecFn -.->|emits| Errors
    Store -.->|preserves| Meta
    SubTrait -.->|preserves| Meta
    Events -.->|domain types implement| Types

    style ExecFn fill:#e1f5ff
    style Store fill:#e1ffe1
    style Cmd fill:#ffe1e1
    style Events fill:#fff3cd
    style SubTrait fill:#d4edda
    style Projections fill:#cce5ff
```

## Event Store Abstraction

### Responsibilities

The `EventStore` trait exposes two core operations for the **write side** (commands):

1. `read_stream` / `read_streams` – fetch all events for one or more streams, returning both events and the current stream versions.
2. `append_events` – atomically register new streams (if needed) and append events to one or more streams while verifying expected versions.

The separate `EventSubscription` trait provides the **read side** (projections and read models) through long-lived event feeds. This separation reflects the fundamental distinction between modifying state (commands via EventStore) and observing state changes over time (subscriptions via EventSubscription). Not every backend must implement subscriptions; backends opt into this capability.

### Atomicity and Transactions

- Multi-stream atomicity is achieved by delegating to the backend's native transaction mechanism (PostgreSQL ACID transactions, in-memory mutexes, etc.).
- Append operations are "all-or-nothing" across every stream referenced in a command.
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

These helpers are provided by the dedicated `eventcore-testing` crate. Consumers should add `eventcore-testing` under the `[dev-dependencies]` section in their Cargo.toml so the testing utilities do not inflate production release binaries. The crate will only be included in downstream release artifacts if a project explicitly elects to list it under `[dependencies]`; keeping it as a dev-dependency preserves lean release builds while still enabling rich testing and contract verification during development and CI.

## Event Subscriptions & Projections

### Separation of Concerns

EventCore separates the write path (commands modifying state) from the read path (projections observing state changes):

- **EventStore** handles commands: atomic multi-stream appends with optimistic concurrency for aggregate state modification
- **EventSubscription** handles projections: long-lived event streams for building read models and triggering side effects

This separation embodies CQRS (Command-Query Responsibility Segregation) at the architectural level. Commands and subscriptions differ fundamentally in lifecycle, semantics, and backend requirements, so conflating them would violate single responsibility.

### Subscription Queries

Projections query events using `SubscriptionQuery`, a composable filter chain:

```rust
// Type-safe filter composition (not magic strings)
let query = SubscriptionQuery::all()
    .filter_stream_prefix("account-")  // Literal prefix filtering
    .filter_event_type::<MoneyDeposited>();
```

The composable API provides:

- **Type Safety**: Invalid query combinations caught at compile time
- **Discoverability**: IDE autocomplete guides developers to valid filter methods
- **Flexibility**: Multi-dimensional queries (stream patterns + event types + metadata filters)

**Literal Prefixes vs Future Pattern Matching:**

`StreamPrefix` currently performs literal prefix matching. Because both `StreamId` and `StreamPrefix` prohibit glob metacharacters (`*`, `?`, `[`, `]`), future pattern matching can be added without ambiguity:

- Literal filtering: `filter_stream_prefix(StreamPrefix::try_new("account-")?)` matches streams starting with "account-"
- Future pattern matching: `filter_stream_pattern(StreamPattern::new("account-*"))` will use explicit glob syntax for wildcard matching

This separation prevents confusion between literal identifiers (domain concepts) and query patterns (infrastructure features). The type system makes intent explicit—developers cannot accidentally confuse literal filtering with pattern matching.

Queries remain infrastructure types distinct from `StreamId` (which represents aggregate identity). This maintains domain-first design—`StreamId` stays focused on business concepts while `SubscriptionQuery` expresses cross-cutting infrastructure queries.

### Push-Based Delivery

Subscriptions deliver events as `futures::Stream`, integrating naturally with Rust's async ecosystem:

```rust
let subscription: impl EventSubscription = /* ... */;
let events: impl Stream<Item = Event> = subscription.subscribe(query).await?;

// Use StreamExt combinators
events
    .map(|evt| build_projection(evt))
    .take(100)
    .collect::<Vec<_>>()
    .await;
```

This push-based model enables:

- Standard async patterns: `select!`, `join!`, `StreamExt` combinators
- Clean testing: collect streams into vectors, assert on counts/contents
- Natural composition: chain filters, maps, and take operations

Backends implement push delivery through spawned tasks and internal buffers, keeping this complexity internal while providing ergonomic consumer APIs.

### Distributed Coordination

The `SubscriptionCoordinator` trait provides production-ready horizontal scaling:

**Control Plane (Coordination):**
- Assigns subscriptions to consumer processes
- Detects failures via heartbeats and timeouts
- Rebalances subscriptions when processes join/leave
- Manages checkpoint persistence and resumption

**Data Plane (Delivery):**
- `EventSubscription` delivers event streams to assigned consumers
- Consumers process events and checkpoint progress
- Checkpoint validates ownership—failure indicates revocation during rebalancing

Backends expose coordination capabilities via an associated type:

```rust
trait EventSubscription {
    type Coordinator: SubscriptionCoordinator;
    fn coordinator(&self) -> Option<&Self::Coordinator>;
}
```

This design:

- Makes backend capabilities explicit at compile time
- Allows single-process applications to ignore coordination entirely
- Enables using different backends for events vs coordination if needed

### At-Least-Once Delivery

Subscriptions guarantee **at-least-once delivery**:

- Events may be delivered multiple times during failures, restarts, or rebalancing
- Brief overlap between old and new owners is possible during reassignment
- **Consumers must be idempotent**: processing the same event twice must produce the same result

This delivery semantic reflects production reality—exactly-once would require distributed transactions that conflict with event sourcing's append-only model. Applications design projections to handle duplicates gracefully.

### Subscription Error Handling

When projection handlers fail to process events, applications need flexible control over recovery strategies. Unlike command execution where the library automatically retries transient failures (version conflicts), subscription errors require application-specific handling because the same error may demand different responses depending on projection semantics.

**Error Handling Strategies:**

Applications configure error handling through failure callbacks that receive rich context (event, error, attempt count, stream position) and return one of three strategies:

- **Fatal (Default)**: Stop processing and crash the subscription—ensures operators discover problems immediately, prevents silent projection drift
- **Skip**: Log the error and continue to next event—tolerates gaps, suitable for caches, dashboards, or non-critical projections
- **Retry**: Retry the same event with exponential backoff—handles transient failures (external API timeouts) without losing events

Fatal is the default because silent data loss is more dangerous than a crashed projection. Applications that want resilience must consciously opt into Skip or Retry, documenting their tolerance for gaps or delayed processing.

**Ordering Preservation:**

All error handling strategies preserve temporal ordering—events are never processed out of order:

- **Fatal**: Stops the stream, no further events delivered
- **Skip**: Skips the current event, continues to next in order (creates gap, not reorder)
- **Retry**: Blocks subsequent events until current event succeeds or exhausts retries

This is a hard constraint. Projections depend on temporal ordering for correctness (financial ledgers, state machines, aggregate queries). At-least-once delivery allows duplicate events (handled via idempotency), but reordering would corrupt projections in ways idempotency cannot fix.

**Retry Configuration:**

When using Retry strategy, applications configure:

- **Max Attempts**: Hard limit before escalating to Fatal (prevents infinite loops on permanent failures)
- **Backoff Policy**: Exponential backoff with configurable base delay and multiplier
- **Jitter**: Optional randomization to prevent thundering herd during recovery
- **Timeout**: Per-attempt timeout for slow operations

This configuration mirrors command retry (automatic OCC handling) but applies to application-initiated retry of projection failures.

**Separation of Concerns:**

Error handling is configured when consuming the subscription stream, not at subscription creation:

- `SubscriptionQuery` describes event selection (stream filters, event types)
- Error handling middleware wraps the stream at consumption time
- Same subscription can be consumed by different handlers with different error tolerance

This maintains the clean separation established by `EventSubscription`—the trait delivers events, application code processes them with chosen error handling.

### Checkpoint Management

Consumers control **when** to checkpoint; coordinators control **where** and **how**:

```rust
let membership = coordinator.join("my-consumer-group").await?;
let active: ActiveSubscription<E> = membership.assignments().next().await?;

while let Some(event) = active.events.next().await {
    update_read_model(event);

    // Validates ownership, persists position
    active.checkpoint(event.position).await?;
    // Returns Err(Revoked) if subscription was reassigned
}
```

The `ActiveSubscription::checkpoint()` method:

1. **Validates ownership**: Ensures this consumer still owns the subscription
2. **Persists position**: Stores checkpoint for resumption after restart
3. **Discovers revocation**: Fails if subscription was reassigned during rebalancing

This design makes ownership validation automatic—consumers cannot checkpoint without going through the coordinator. Checkpoint storage location (same database as events, separate storage, etc.) is a coordinator configuration choice.

### Production Deployment

EventCore supports distributed deployments without external infrastructure:

- **PostgreSQL backend** uses advisory locks for coordination—zero additional services
- **Consumer groups** distribute subscription load across multiple processes
- **Automatic rebalancing** when processes start, stop, or crash
- **Checkpoint resumption** ensures no events are lost across restarts

This built-in coordination eliminates operational burden while providing production-grade reliability. Applications deploying to PostgreSQL gain horizontal scaling automatically.

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
3. Pops a stream ID, reads it exactly once, folds events, and records the stream's version.
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

- **Validated Newtypes** – `StreamId`, `EventId`, `CorrelationId`, `StreamPrefix`, and other domain types enforce invariants at construction time via the `nutype` crate. Character validation prevents invalid identifiers:
  - `StreamId` and `StreamPrefix` reject glob metacharacters (`*`, `?`, `[`, `]`) to enable future pattern matching without ambiguity
  - Both require non-empty strings (after trimming), maximum 255 characters, with leading/trailing whitespace sanitized
  - Future glob pattern support will use a distinct `StreamPattern` type that explicitly permits metacharacters
- **Phantom Types & Typestate** – `StreamWrite<StreamSet, Event>` enforces compile-time stream access control; `NewEvents` carries the same phantom to ensure only declared streams receive emissions.
- **Total Functions** – Public APIs return `Result` instead of panicking. Error enums derive `thiserror` and support pattern matching.
- **Trait Composition** – Narrow traits (`CommandStreams`, `CommandLogic`, `StreamResolver`, `EventSubscription`, `SubscriptionCoordinator`) keep responsibilities focused and implementations testable.

## Error Handling

- **CommandError** – Categorizes domain failures (validation, business rule violations, infrastructure issues surfaced to commands). Business rule violations are permanent by design.
- **EventStoreError** – Represents storage-layer failures (version conflicts, connectivity, serialization). Version conflicts map to `ConcurrencyError` and are retriable.
- **Validation Errors** – Raised by newtype constructors and automatically bubbled up through `CommandError`.
- **Retry Classification** – Errors implement marker traits (or equivalent metadata) indicating whether they are retriable or permanent. The executor consults this classification before attempting any retry.
- **Context Enrichment** – Errors carry correlation ID, causation ID, stream identifiers, and diagnostic details to aid debugging and distributed tracing.

## Reference Implementations & Tooling

- **InMemoryEventStore** is included in `eventcore` and used across documentation, examples, and internal tests. It supports optional chaos hooks (e.g., `ConflictOnceStore`, `CountingEventStore`) for scenario-driven testing. It does not implement `EventSubscription` (single-process memory cannot support durable subscriptions).
- **External Backends** (e.g., `eventcore-postgres`) implement the same traits, run the contract test suite, and may offer additional observability or operational features. Production backends typically implement both `EventStore` and `EventSubscription` with full coordination support.
- **Testing Utilities** – The `eventcore-testing` crate exposes helpers for property-based testing, contract verification, and integration scenarios so downstream users can exercise real command flows without managing infrastructure.

## Putting It All Together

By following the flow above, applications gain:

1. Type-safe domain modeling with zero boilerplate for infrastructure.
2. Deterministic, atomic execution of complex multi-stream business operations.
3. Automatic concurrency management and retry behavior that keeps business code simple.
4. Rich metadata and observability hooks for auditing, compliance, and debugging.
5. Pluggable storage backends validated by a shared contract-suite, ensuring every implementation honors the same semantic guarantees.
6. Production-ready event subscriptions with distributed coordination for building read models and projections at scale.

This document is the single source of truth for EventCore's architecture; ADRs capture how we arrived here, while this blueprint describes the system as it stands today.
