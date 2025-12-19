# ADR-021: Poll-Based Projector Trait Architecture

## Status

accepted

Supersedes: ADR-016, ADR-018, ADR-019, ADR-020

## Context

EventCore's subscription architecture (ADRs 016-020) evolved through multiple iterations attempting to provide push-based event delivery via `futures::Stream`. Implementation experience revealed fundamental tensions between push-based streaming and the practical needs of projection systems.

**The Core Problem:**

Building read model projections requires three coordinated concerns:

1. **Event Retrieval**: Getting events from the store in order
2. **Projection Application**: Updating the read model (database, API, etc.)
3. **Checkpoint Management**: Tracking progress to enable resumption after restart

Push-based streaming (ADR-016) optimizes for event retrieval but creates friction with the other two concerns:

- **Transaction Boundaries**: Projections typically need transactional consistency between applying an event and updating the checkpoint. Push-based streams make it awkward to control when events arrive relative to transaction boundaries.
- **Backpressure Complexity**: When projections need to pause (maintenance, backpressure, graceful shutdown), push-based streams require complex cancellation handling.
- **Error Recovery**: ADR-018/019 documented extensive error handling strategies, but push-based delivery complicates "retry from checkpoint" patterns because the stream state and checkpoint state are separate.

**Lessons from Commanded (Elixir):**

The Commanded ecosystem's Ecto Projections library has proven successful in production through a fundamentally different approach:

- Projectors are GenServer processes that **poll** for new events
- Each projection pass: poll events, apply within transaction, update checkpoint atomically
- Built-in idempotency via `projection_versions` table ensures events cannot be projected twice
- Error handling returns explicit strategies: `{:retry, context}`, `{:retry, delay, context}`, `:skip`, `{:stop, reason}`
- `after_update/3` callback provides lifecycle hooks for notifications

This pattern succeeds because it **aligns the control flow with the transactional requirements**. The projector controls when to fetch events, applies them transactionally, and checkpoints atomically.

**Why Push-Based Streaming Fell Short:**

1. **Separation of Concerns Became Separation of State**: EventSubscription delivered events, but checkpoint management lived elsewhere. Coordinating these required complex synchronization.

2. **Broadcast Channel Complexity**: PostgresEventStore used `tokio::sync::broadcast` for live event delivery. ADR-021 (the previous, never-implemented one) identified that broadcast lag caused silent event loss, violating at-least-once guarantees.

3. **Error Handling Proliferation**: ADR-018 defined middleware adapters, ADR-019 pivoted to trait callbacks - both attempting to solve the fundamental problem that push-based delivery doesn't naturally integrate with transactional error recovery.

4. **Testing Complexity**: Push-based streams required `idle_timeout` hacks and careful timing management for test termination.

**Key Forces:**

1. **Transactional Integrity**: Projection updates and checkpoint updates must be atomic to prevent drift
2. **Simplicity Over Cleverness**: Poll-based is simpler to understand, implement, and debug
3. **Proven Pattern**: Commanded's approach is battle-tested in production Elixir systems
4. **Contract Testing**: Per ADR-013, EventStore implementations use contract tests; projector-EventStore interaction must also be verifiable across implementations
5. **Library Philosophy**: EventCore should provide opinionated, correct defaults rather than maximum flexibility

**Why This Decision Now:**

After implementing ADRs 016-020, the subscription architecture complexity exceeded its value. Multiple interlocking ADRs, broadcast channel edge cases, and error handling middleware created a system harder to use correctly than to use incorrectly. A simpler foundation is needed before proceeding.

## Decision

EventCore will replace the push-based subscription architecture with a poll-based Projector trait inspired by Commanded's Ecto Projections:

**1. Poll-Based Event Retrieval**

Projectors poll for events rather than receiving pushed streams. The core loop:

```
loop {
    events = poll_events_after(checkpoint)
    for event in events {
        apply_event_in_transaction(event)
        update_checkpoint_atomically(event.position)
    }
    sleep_if_no_events()
}
```

**2. Projector Trait Definition**

```rust
pub trait Projector: Send + 'static {
    /// The event type this projector handles
    type Event: Subscribable;

    /// Error type for projection failures
    type Error: std::error::Error + Send;

    /// Context type for projection operations (e.g., database transaction handle)
    type Context;

    /// Apply a single event to the projection within the given context.
    /// The implementation should update the read model AND the checkpoint atomically.
    fn apply(&mut self, event: Self::Event, position: StreamPosition, ctx: &mut Self::Context) -> Result<(), Self::Error>;

    /// Handle projection failures (default: Fatal - stop immediately)
    fn on_error(&mut self, failure: FailureContext<Self::Event, Self::Error>) -> FailureStrategy {
        FailureStrategy::Fatal
    }

    /// Called after successful event application (default: no-op)
    /// Use for side effects like notifications, metrics, pub/sub
    fn after_apply(&mut self, event: &Self::Event, ctx: &Self::Context) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Returns the projector's unique name for checkpoint identification
    fn name(&self) -> &str;
}
```

**3. Integrated Checkpoint Management**

Checkpoints are managed **within** the projection transaction, not as a separate concern:

- Projector implementations control checkpoint persistence
- Checkpoint updates happen atomically with read model updates
- No separate `SubscriptionCoordinator` or `ActiveSubscription` types needed

**4. Event Retrieval via Query**

Replace `EventSubscription` trait with simpler query-based polling:

```rust
pub trait EventReader {
    /// Poll for events matching the query after the given position
    async fn read_events_after<E: Subscribable>(
        &self,
        query: SubscriptionQuery,
        after: Option<StreamPosition>,
        limit: usize,
    ) -> Result<Vec<(E, StreamPosition)>, EventStoreError>;
}
```

EventStore implementations that want to support projections implement this trait.

**5. Contract Tests for Projector Compatibility**

Extend `eventcore-testing` with projector contract tests per ADR-013's pattern:

- Tests verify that `read_events_after` returns events in correct order
- Tests verify position values enable correct checkpoint resumption
- Tests verify filtering by event type works correctly
- All EventStore implementations supporting projections must pass these tests

**6. Error Handling (Preserved from ADR-019)**

The three-strategy model remains valid:

- **Fatal** (default): Stop immediately, prevent silent drift
- **Skip**: Log and continue (explicit opt-in for non-critical projections)
- **Retry**: Retry with optional delay (for transient failures)

**7. Removal of Push-Based Infrastructure**

The following are removed or deprecated:

- `EventSubscription` trait (push-based streaming)
- `SubscriptionCoordinator` trait (never implemented)
- `ActiveSubscription` type (never implemented)
- Broadcast channel infrastructure in EventStore implementations
- Complex idle_timeout handling

## Rationale

**Why Poll-Based Over Push-Based:**

The fundamental insight is that **projections need control over their processing cadence**, not maximum throughput. Push-based streaming optimizes for throughput at the cost of control. Poll-based retrieval optimizes for control, which is what projections actually need.

| Concern | Push-Based | Poll-Based |
|---------|------------|------------|
| Transaction boundaries | Stream delivers events outside transaction; must buffer/coordinate | Projector fetches within transaction |
| Checkpoint atomicity | Separate update after stream consumption | Same transaction as projection |
| Error recovery | Complex - stream state vs checkpoint state | Simple - restart from checkpoint |
| Backpressure | Requires buffering or complex cancellation | Just don't poll |
| Testing | Requires timing hacks (idle_timeout) | Deterministic |
| Implementation | Broadcast channels, background tasks | Simple database query |

**Why Mirror Commanded's API:**

Commanded's Ecto Projections is the most mature, production-tested projection library in the event sourcing ecosystem. Its design reflects years of real-world usage:

1. **`project/2` macro** (our `apply` method): Takes event + context, returns transaction operations
2. **`error/3` callback** (our `on_error`): Receives failure context, returns strategy
3. **`after_update/3` callback** (our `after_apply`): Lifecycle hook for side effects
4. **`projection_versions` table** (our checkpoint in Context): Idempotency tracking

The pattern is proven. Adapting it to Rust idioms preserves the benefits while fitting naturally into the language.

**Why Integrated Checkpointing:**

ADR-016 separated checkpointing as a "coordinator concern" with `ActiveSubscription.checkpoint()`. This created coordination problems:

- When to call checkpoint relative to projection?
- What if checkpoint fails after projection succeeds?
- How to make them atomic?

Commanded solves this by making checkpoint updates part of the same database transaction as projection updates. The `Ecto.Multi` struct bundles both operations. Our `Context` type parameter enables the same pattern - a database transaction handle that can update both projection tables and checkpoint tables atomically.

**Why Contract Tests for EventStore-Projector Interaction:**

ADR-013 established contract tests for EventStore implementations. The same principle applies to projector support:

- Different EventStore implementations will have different `read_events_after` implementations
- All must provide consistent ordering, filtering, and position semantics
- Contract tests guarantee any conforming implementation works with projectors
- External implementors can verify compatibility before deployment

**Trade-offs Accepted:**

- **Lower Maximum Throughput**: Poll-based can't match push-based for raw event delivery speed. Acceptable because projections are typically I/O bound on read model updates, not event retrieval.

- **Polling Latency**: New events aren't delivered instantly; there's a poll interval. Acceptable because most projections tolerate seconds of latency, and poll interval is configurable.

- **No Built-in Parallelism**: Single projector processes events sequentially. Acceptable because parallel projections can run separate projector instances, and ordering guarantees require sequential processing anyway.

- **More Responsibility on Projector Implementations**: Projectors must manage checkpoint persistence within their Context. Acceptable because this provides flexibility for different storage backends (same DB as projection, separate checkpoint store, Redis, etc.).

## Consequences

### Positive

- **Dramatically Simpler Architecture**: One trait (`Projector`) replaces four ADRs worth of types and traits
- **Transactional Correctness by Design**: Checkpoint atomicity is structural, not behavioral
- **Proven Pattern**: Following Commanded's battle-tested design
- **Easier Testing**: No timing-dependent idle_timeout hacks; poll-based is deterministic
- **Clearer Mental Model**: "Fetch, apply, checkpoint" loop is easy to understand and debug
- **Contract Tests Enable Ecosystem**: External EventStore implementations can verify projector compatibility

### Negative

- **Breaking Change**: Existing code using `EventSubscription` must migrate
- **Polling Latency**: Not suitable for real-time streaming use cases (sub-second latency requirements)
- **Manual Checkpoint Implementation**: Projectors must implement checkpoint persistence (no built-in storage)
- **Supersedes Recent Work**: ADRs 016-020 represent significant design effort now discarded

### Supersedes

This ADR supersedes:

- **ADR-016** (Event Subscription Model): Push-based EventSubscription trait replaced by poll-based EventReader
- **ADR-018** (Subscription Error Handling): Error handling strategy preserved but context simplified
- **ADR-019** (Projector Trait): Projector trait concept preserved but redesigned for poll-based model
- **ADR-020** (Subscribable Trait): Subscribable trait retained for type filtering; subscription delivery mechanism changes

These ADRs remain as historical records of design evolution.

### Enabled Future Decisions

- **Derive Macro**: `#[derive(Projector)]` could reduce boilerplate for common patterns
- **Checkpoint Storage Helpers**: Provide optional checkpoint persistence utilities for PostgreSQL, Redis, etc.
- **Parallel Projection**: Multiple projector instances could partition event space for throughput
- **Projection Rebuilding**: Simple checkpoint reset enables projection reconstruction from scratch
- **Batch Processing**: `apply_batch()` method for throughput optimization

### Constrained Future Decisions

- **Poll-Based is Fundamental**: Cannot revert to push-based without another architectural revision
- **Sequential Processing**: Ordering guarantees require sequential event processing within a projector
- **Checkpoint Responsibility**: Projectors own checkpoint management; library provides utilities, not enforcement

## Alternatives Considered

### Alternative 1: Fix Push-Based Implementation

**Description**: Continue with ADR-016 architecture, fix broadcast lag and coordination issues.

**Why Rejected**:

The issues with push-based are architectural, not implementation bugs:

- Broadcast lag is inherent in bounded channels under load
- Transaction boundaries remain awkward regardless of delivery reliability
- Checkpoint coordination complexity doesn't decrease with better implementation

Fixing symptoms doesn't address the fundamental mismatch between push-based delivery and transactional projection requirements.

### Alternative 2: Hybrid Push/Poll

**Description**: Push for live events, poll for catch-up, automatically switch between modes.

**Why Rejected**:

Adds complexity without proportional benefit:

- Must handle mode transitions correctly
- Two code paths to test and maintain
- Edge cases at transition boundaries
- Push-based issues still present during live processing

If poll-based is acceptable for catch-up, it's acceptable for live events too. The latency difference (milliseconds vs tens of milliseconds) rarely matters for projections.

### Alternative 3: Keep EventSubscription, Add Projector Layer

**Description**: Preserve push-based EventSubscription as low-level primitive, build poll-based Projector on top.

**Why Rejected**:

- Two abstraction levels to understand and maintain
- EventSubscription complexity remains in codebase
- Users may use wrong abstraction level
- No clear use case for push-based that poll-based doesn't serve

Removing push-based entirely simplifies the library and user mental model.

### Alternative 4: Callback-Based Instead of Trait-Based

**Description**: Configure projector with closures rather than implementing a trait.

**Why Rejected**:

Traits provide:
- Type safety for Event and Error types
- IDE discoverability of required methods
- Clear documentation of contract
- Standard Rust pattern for pluggable behavior

Closures would require runtime configuration and lose compile-time guarantees.

### Alternative 5: No Projector Abstraction (Just EventReader)

**Description**: Provide only `EventReader::read_events_after()`, let users build their own projection loop.

**Why Rejected**:

- Every user reinvents the same loop structure
- Error handling patterns diverge
- Checkpoint management ad-hoc
- No contract tests for user implementations

The Projector trait captures the common pattern and enables contract testing.

## References

- **Commanded Ecto Projections**: [HexDocs](https://hexdocs.pm/commanded_ecto_projections/) - Primary inspiration
- **Commanded.Event.Handler**: [Documentation](https://hexdocs.pm/commanded/Commanded.Event.Handler.html) - Behaviour pattern
- **ADR-013**: EventStore Contract Testing Approach - Contract test pattern to extend
- **ADR-016**: Event Subscription Model (superseded)
- **ADR-018**: Subscription Error Handling Strategy (superseded)
- **ADR-019**: Projector Trait for Read Model Construction (superseded)
- **ADR-020**: Subscribable Trait for Subscription Participation (superseded)
- **Martin Dilger "Understanding Event Sourcing"**: Tracking Event Processor pattern

Sources:
- [Commanded.Projections.Ecto Documentation](https://hexdocs.pm/commanded_ecto_projections/Commanded.Projections.Ecto.html)
- [Commanded Ecto Projections Usage Guide](https://hexdocs.pm/commanded_ecto_projections/usage.html)
- [GitHub - commanded/commanded-ecto-projections](https://github.com/commanded/commanded-ecto-projections)
