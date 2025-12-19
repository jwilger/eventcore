# ADR-019: Projector Trait for Read Model Construction

## Status

superseded by ADR-021

## Implementation Status

**Not Yet Implemented** – The `Projector` trait described in this ADR is designed but not yet implemented. See eventcore-4tk for tracking.

**Currently Available:**
- Raw `Stream<Item = Result<E, SubscriptionError>>` API for building projections
- Manual error handling via `StreamExt` combinators (`.filter_map()`, `.take_while()`)
- `Subscribable` trait for view enum support

The trait-based `Projector` pattern will be implemented in a future release as described below.

## Context

ADR-018 established a middleware-based approach to subscription error handling: applications would wrap `Stream<Item = Result<E, SubscriptionError>>` with adapters like `SkipErrors` and `RetryWithBackoff`. While this approach offers maximum flexibility and composability, practical analysis reveals significant ergonomic and safety concerns.

**The Core Problem:**

Building read model projections from event streams is a common, critical operation. Developers need:

1. A clear place to put projection logic
2. A clear place to handle errors
3. Lifecycle hooks for side effects (notifications, metrics)
4. Safe defaults that prevent silent data loss
5. Discoverable API—what can I customize?

The middleware approach scatters these concerns across adapter chains, making error handling configuration implicit rather than explicit.

**Lessons from Commanded (Elixir):**

The Commanded CQRS framework has battle-tested a trait-based approach (`Commanded.Event.Handler` behaviour) where:

- Projectors implement a behaviour with well-defined callbacks
- `handle/2` processes events
- `error/3` receives rich failure context and returns a strategy (skip, retry, stop)
- `after_update/3` provides lifecycle hooks
- Default behavior is safe (stop on error, requiring explicit opt-in to skip/retry)

This pattern has proven successful in production systems because it gives error handling a clear "home" rather than distributing it across middleware.

**Why Middleware Falls Short:**

1. **Scattered Logic**: Error handling lives in adapter configuration, separate from projection logic
2. **Implicit Configuration**: Must remember to add adapters in correct order
3. **No Lifecycle Hooks**: No clear place for `after_update` side effects
4. **Manual Context Threading**: Retry state must be threaded through adapter chain
5. **Less Discoverable**: Must know which adapters exist and how to compose them
6. **No Default Safety**: Easy to forget error handling entirely

**Why This Decision Now:**

As we implement subscription error handling for eventcore-017/eventcore-4tk, we must choose between middleware composition and trait-based projectors. This decision affects the entire projection API surface and developer experience. Choosing now ensures consistent patterns across the subscription ecosystem.

## Decision

EventCore will provide a `Projector` trait that encapsulates projection logic, error handling, and lifecycle hooks in a single cohesive abstraction:

**1. Projector Trait Definition**

```rust
pub trait Projector: Send + 'static {
    /// The event type this projector handles
    type Event: Subscribable;

    /// Error type for projection failures
    type Error: std::error::Error + Send;

    /// Process a single event, updating read model state
    fn project(&mut self, event: Self::Event) -> Result<(), Self::Error>;

    /// Handle projection failures (default: Fatal)
    fn on_error(&mut self, ctx: FailureContext<Self::Event, Self::Error>) -> FailureStrategy {
        FailureStrategy::Fatal
    }

    /// Called after successful projection (default: no-op)
    fn after_update(&mut self, event: &Self::Event) -> Result<(), Self::Error> {
        Ok(())
    }
}
```

**2. Failure Context**

Rich context passed to error callback:

```rust
pub struct FailureContext<E, Err> {
    /// The event that failed to project
    pub event: E,
    /// The error returned by project()
    pub error: Err,
    /// Number of attempts (1 = first attempt)
    pub attempt: u32,
    /// Stream position for checkpoint tracking
    pub position: StreamPosition,
}
```

**3. Failure Strategies**

Three strategies matching ADR-018's error handling model:

```rust
pub enum FailureStrategy {
    /// Stop projection immediately (default)
    Fatal,
    /// Skip this event, continue to next
    Skip,
    /// Retry with optional delay
    Retry { delay: Option<Duration> },
}
```

**4. Projector Runner**

Helper function to run a projector against a subscription:

```rust
pub async fn run_projector<P, S>(
    projector: P,
    subscription: S,
    config: ProjectorConfig,
) -> Result<(), ProjectorError>
where
    P: Projector,
    S: Stream<Item = Result<P::Event, SubscriptionError>>,
```

**5. Configuration**

```rust
pub struct ProjectorConfig {
    /// Maximum retry attempts before escalating to Fatal
    pub max_retries: u32,
    /// Base delay for exponential backoff
    pub retry_base_delay: Duration,
    /// Backoff multiplier
    pub retry_multiplier: f64,
    /// Optional jitter for retry delays
    pub retry_jitter: bool,
}
```

**6. Raw Stream Access Preserved**

The low-level `Stream<Item = Result<E, SubscriptionError>>` remains available for advanced use cases requiring custom stream processing beyond what `Projector` provides.

**7. Relationship to ADR-018**

This decision **supersedes** ADR-018's middleware-based error handling approach. The core principles from ADR-018 remain valid:

- Fatal is default (fail-fast philosophy)
- Skip and Retry are explicit opt-in
- Ordering is always preserved
- Error handling is application-specific

The change is **where** error handling is configured: trait callback vs stream middleware.

## Rationale

**Why Trait-Based Over Middleware:**

1. **Error Handling Has a Home**: The `on_error` callback is THE place to decide strategy—no ambiguity about where this logic belongs

2. **Rich Context by Default**: `FailureContext` provides event, error, attempt count, and position without manual threading

3. **Discoverable API**: IDE autocomplete shows `on_error`, `after_update`—developers know what they can customize

4. **Safe Defaults**: Not implementing `on_error` gives you Fatal behavior—safety requires zero effort

5. **Co-located Logic**: Projection logic, error handling, and lifecycle hooks live in one struct—easier to understand and maintain

6. **Matches Mental Model**: A projector IS a thing with behavior, not just a stream consumer with adapters bolted on

7. **Proven Pattern**: Commanded's `Event.Handler` behaviour has been battle-tested in production Elixir systems

**Why Keep Raw Stream Access:**

Some use cases don't fit the Projector model:

- Complex stream transformations (joining multiple subscriptions)
- Custom batching strategies
- Integration with external streaming systems
- One-off scripts and migrations

Providing both levels (trait-based and raw stream) serves different needs without forcing one pattern.

**Why `on_error` Returns Strategy Instead of Result:**

Returning `FailureStrategy` enum makes the decision explicit:

- `Fatal` clearly means "stop now"
- `Skip` clearly means "acknowledge and continue"
- `Retry` clearly means "try again with optional delay"

This is more intentional than returning `Result<(), E>` where the meaning of `Err` is ambiguous.

**Trade-offs Accepted:**

- **More API surface**: `Projector` trait, `FailureContext`, `FailureStrategy`, `run_projector`—more types to learn
- **Struct required**: Simple projections need a struct + impl, can't just use a closure
- **One strategy per projector**: Can't have different error strategies for different event types within one projector (would need multiple projectors or manual dispatch in `on_error`)

These trade-offs are acceptable because:

- API surface is cohesive and discoverable
- Struct + impl is idiomatic Rust for stateful behavior
- Per-projector strategy matches the common case; complex cases use raw streams

## Consequences

### Positive

- **Clear error handling contract**: Developers know exactly where to put error handling logic
- **Safe by default**: Fatal behavior without any configuration prevents silent projection drift
- **Rich failure context**: Event, error, attempt count, position available in callback
- **Lifecycle hooks**: `after_update` provides clean extension point for notifications, metrics
- **Discoverable**: IDE shows available callbacks, reducing learning curve
- **Proven pattern**: Mirrors Commanded's successful production-tested design
- **Flexibility preserved**: Raw stream access remains for advanced use cases

### Negative

- **Additional abstraction layer**: Developers must learn `Projector` trait in addition to raw streams
- **Struct boilerplate**: Simple projections require struct definition and trait implementation
- **Single error strategy per projector**: Complex per-event-type strategies require manual dispatch or multiple projectors
- **Two ways to do things**: Both `Projector` trait and raw streams available, potentially confusing

### Supersedes

This ADR **supersedes ADR-018** for subscription error handling implementation. ADR-018's principles (Fatal default, Skip/Retry opt-in, ordering preservation) remain valid and are implemented through the `Projector` trait rather than stream middleware.

ADR-018 remains a valid historical record of the middleware approach considered and why we initially chose it. This ADR documents the pivot to trait-based error handling after deeper analysis of the Commanded ecosystem.

### Enabled Future Decisions

- **Derive macro**: `#[derive(Projector)]` could reduce boilerplate for simple cases
- **Batched projections**: `project_batch(&mut self, events: Vec<E>)` callback for throughput optimization
- **Checkpointing integration**: `Projector` could integrate with `SubscriptionCoordinator` checkpointing
- **Idempotency helpers**: Built-in duplicate detection similar to Commanded's `projection_versions`
- **Process manager trait**: Similar pattern for sagas/process managers handling events and dispatching commands

### Constrained Future Decisions

- **Callback signature stability**: `on_error` and `after_update` signatures are public API
- **FailureStrategy variants**: Adding new strategies is breaking change
- **FailureContext fields**: Adding required fields is breaking change (could use builder pattern)

## Alternatives Considered

### Alternative 1: Pure Middleware (ADR-018 Original)

**Description**: Wrap streams with `SkipErrors`, `RetryWithBackoff` adapters.

**Why Reconsidered**:

After deeper analysis, middleware scatters error handling logic:

```rust
// Where does error handling live? In the adapter chain configuration.
subscription
    .retry_with_backoff(config)
    .skip_errors()
    .for_each(|e| project(e))
```

vs trait-based:

```rust
// Error handling lives in the projector, next to projection logic
impl Projector for MyProjector {
    fn project(&mut self, e: E) -> Result<(), Err> { ... }
    fn on_error(&mut self, ctx: FailureContext) -> FailureStrategy { ... }
}
```

The trait-based approach is more cohesive and discoverable.

### Alternative 2: Closure-Based Error Handler

**Description**: Pass error handling closure to `run_projector`:

```rust
run_projector(
    |event| project(event),
    |error, event, ctx| FailureStrategy::Retry { delay: None },
    subscription,
)
```

**Why Rejected**:

- No clear home for projection state
- No lifecycle hooks (`after_update`)
- Less discoverable than trait methods
- Harder to test in isolation

### Alternative 3: Builder Pattern for Projector Configuration

**Description**: Configure projector behavior via builder:

```rust
ProjectorBuilder::new()
    .on_event(|e| project(e))
    .on_error(|ctx| FailureStrategy::Skip)
    .after_update(|e| notify(e))
    .run(subscription)
```

**Why Rejected**:

- Runtime configuration vs compile-time trait implementation
- No type safety for event types
- Harder to share projector definitions
- Less idiomatic Rust

### Alternative 4: Attribute Macros (Like Commanded's `project/2`)

**Description**: Use procedural macros for declarative projection definition:

```rust
#[projector(application = MyApp)]
impl AccountProjector {
    #[project]
    fn handle_deposit(&mut self, e: MoneyDeposited) { ... }

    #[project]
    fn handle_withdrawal(&mut self, e: MoneyWithdrawn) { ... }
}
```

**Why Deferred**:

Attractive for ergonomics but adds complexity:

- Procedural macro development and maintenance
- Compile-time debugging difficulty
- Can be added later as sugar over `Projector` trait

The plain trait approach provides foundation; macros can be layered on top.

## References

- **ADR-018**: Subscription Error Handling Strategy (superseded by this ADR)
- **ADR-016**: Event Subscription Model (establishes EventSubscription trait)
- **Commanded.Event.Handler**: [Documentation](https://hexdocs.pm/commanded/Commanded.Event.Handler.html)
- **Commanded Ecto Projections**: [Usage Guide](https://hexdocs.pm/commanded_ecto_projections/usage.html)
- **Martin Dilger "Understanding Event Sourcing"**: Ch4 Projections
