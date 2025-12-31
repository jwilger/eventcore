# ADR-029: Projection Runner API Simplification

## Status

accepted

## Date

2025-12-30

## Deciders

Project maintainers

## Context

ADR-026 established session-scoped advisory locks on dedicated connections for projector coordination. ADR-028 specified non-blocking lock acquisition with `pg_try_advisory_lock()`. The projection runner needs to orchestrate three concerns:

1. **Event reading**: Polling for new events via `EventReader`
2. **Checkpoint management**: Tracking projection progress via `CheckpointStore`
3. **Coordination**: Ensuring single-leader operation via `ProjectorCoordinator`

The initial API proposal was:

```rust
ProjectionRunner::new(projector, &event_reader, &coordinator)
    .with_checkpoint_store(&checkpoint_store)
    .run()
```

**Problems with This Design:**

1. **Inconsistent Pattern**: Mixes constructor arguments with builder pattern. Why is `coordinator` in `new()` but `checkpoint_store` in `with_checkpoint_store()`? The distinction is arbitrary and confusing.

2. **Multiple Backend References**: Requires passing three separate references (`&event_reader`, `&coordinator`, `&checkpoint_store`) even when they all point to the same underlying infrastructure. In typical PostgreSQL deployments, one `PgPool` provides all three capabilities.

3. **Doesn't Match Command API**: ADR-010 established `execute(command, &store)` as the command execution pattern. The projection runner should follow the same ergonomic style: a free function with explicit dependencies.

4. **Unnecessary Struct**: `ProjectionRunner` exists only to call `.run()`. There's no benefit to the intermediate struct - it adds API surface without providing value.

**The Core Insight:**

For 99.9% of deployments, all three traits are implemented by the same backend. PostgreSQL stores provide event reading, checkpoint storage, and advisory lock coordination from a single connection pool. Requiring users to pass the same reference three times is ceremony without purpose.

## Decision

Use a free function `run_projection(projector, &backend)` where the backend must implement all three required traits via bounds.

**API Shape:**

```rust
/// Runs a projector against a backend that provides events, checkpoints, and coordination.
///
/// # Arguments
/// * `projector` - The projector implementation to run
/// * `backend` - A reference to a backend implementing EventReader, CheckpointStore, and ProjectorCoordinator
///
/// # Returns
/// Returns when the projector completes, is cancelled, or encounters a fatal error.
pub async fn run_projection<P, B>(
    projector: P,
    backend: &B,
) -> Result<(), ProjectionError>
where
    P: Projector,
    B: EventReader + CheckpointStore + ProjectorCoordinator,
{
    // Implementation coordinates all three concerns
}
```

**Simple Case (99.9% of deployments):**

```rust
// PostgreSQL provides all three traits
run_projection(my_projector, &postgres_store).await?;
```

**Mixed Backend Case:**

For deployments where events come from one source (e.g., EventStoreDB) but coordination belongs in another (e.g., PostgreSQL where read models live), users create a wrapper struct:

```rust
struct MyBackend<'a> {
    events: &'a EventStoreDbClient,
    postgres: &'a PgPool,
}

impl EventReader for MyBackend<'_> {
    // Delegate to self.events
}

impl CheckpointStore for MyBackend<'_> {
    // Delegate to self.postgres
}

impl ProjectorCoordinator for MyBackend<'_> {
    // Delegate to self.postgres
}

// Usage
let backend = MyBackend { events: &esdb, postgres: &pool };
run_projection(my_projector, &backend).await?;
```

**Why Keep Traits Separate (Not Consolidated):**

The three traits remain separate despite the unified API because:

1. **Single Responsibility**: Each trait has one job. `EventReader` reads events. `CheckpointStore` manages checkpoints. `ProjectorCoordinator` handles leader election. Clear boundaries make implementations easier to understand and test.

2. **Independent Unit Testing**: Each trait implementation can be tested in isolation. `CheckpointStore` tests don't need event fixtures. `ProjectorCoordinator` tests don't need checkpoint tables. This separation enables focused, fast tests.

3. **Reusability**: Individual traits may be used elsewhere. `EventReader` could power a CLI tool that dumps events. `CheckpointStore` could track progress for non-projection workflows. Consolidation would prevent these uses.

4. **Different Backends Are Valid**: Events from EventStoreDB, checkpoints and coordination from PostgreSQL is a legitimate architecture. Read models live in PostgreSQL, so coordination belongs there too. The wrapper pattern makes this explicit without framework complexity.

## Rationale

**Why Free Function Over Struct:**

ADR-010 established free functions as EventCore's API style. Benefits:

- Explicit dependencies visible in signature
- Composable and testable
- Matches `execute(command, &store)` pattern
- No unnecessary intermediate struct
- Rust community conventions (tokio::spawn, serde_json::to_string)

**Why Single Backend Parameter:**

The common case should be trivial. Most deployments use PostgreSQL for everything. Requiring:

```rust
run_projection(projector, &store, &store, &store).await?;
```

...is obviously wrong. And the builder pattern:

```rust
ProjectionRunner::new(projector, &store)
    .with_checkpoint_store(&store)
    .with_coordinator(&store)
    .run()
```

...is verbose for no benefit when all three are the same.

The trait bounds approach optimizes for the common case while still supporting the uncommon case via explicit wrapper structs.

**Why Wrapper Structs for Mixed Backends:**

Alternative approaches considered:

- **Tuple parameter**: `run_projection(projector, (&events, &checkpoints, &coordinator))` - Unclear which is which, easy to get wrong
- **Named parameters struct**: More ceremony than a purpose-built wrapper
- **Generic over each backend**: `run_projection<P, E, C, X>(projector, &events, &checkpoints, &coordinator)` - Verbose, doesn't solve the "same reference three times" problem

The wrapper struct approach:
- Makes the mixed-backend architecture explicit in code
- Documents which backend provides which capability
- Enables custom behavior (e.g., connection pooling strategies) if needed
- Is ~15 lines of boilerplate for an uncommon case

**Why This Matches EventCore's Philosophy:**

EventCore's API principles from ADR-010:

1. Free functions over methods on structs
2. Explicit dependencies in function signatures
3. Traits for polymorphism
4. Optimize for common case, support uncommon case

`run_projection(projector, &backend)` follows all four.

## Consequences

### Positive

- **Ergonomic**: Simple case is one line: `run_projection(my_projector, &store).await?`
- **Consistent**: Matches `execute(command, &store)` pattern from command execution
- **Explicit**: Dependencies visible in function signature
- **Flexible**: Mixed backends supported via wrapper structs
- **Testable**: Mock backend implementing all three traits for unit tests
- **Discoverable**: Free function at crate root, easy to find in docs

### Negative

- **Wrapper Boilerplate**: Mixed backend deployments require ~15 lines of wrapper code
- **Trait Bound Complexity**: Error messages for missing trait implementations may be verbose
- **All-or-Nothing**: Backend must implement all three traits; can't opt out of coordination

### Neutral

- **Separate Traits Retained**: Three traits instead of one consolidated trait. This is intentional for the reasons stated, but adds conceptual surface area.

### Enabled Future Decisions

- **Configuration**: Could add `run_projection_with_config(projector, &backend, config)` for poll intervals, retry policies
- **Cancellation**: Could accept a `CancellationToken` parameter for graceful shutdown
- **Metrics**: Could add `run_projection_with_metrics(projector, &backend, &metrics)` for observability
- **Testing Helpers**: Could provide `MockBackend` implementing all three traits

### Constrained Future Decisions

- **Cannot Split Function**: Adding separate parameters for each backend would break the simple case API
- **Trait Bounds Fixed**: Adding a fourth required trait would be a breaking change
- **No Optional Coordination**: Can't run without coordination; wrapper must implement `ProjectorCoordinator` even if it's a no-op

## Alternatives Considered

### Alternative 1: Consolidated `ProjectionBackend` Trait

**Description**: Create a single trait that combines all three capabilities:

```rust
pub trait ProjectionBackend: EventReader + CheckpointStore + ProjectorCoordinator {}

// Blanket implementation
impl<T> ProjectionBackend for T
where
    T: EventReader + CheckpointStore + ProjectorCoordinator
{}
```

**Pros**:
- Simpler function signature: `run_projection<P, B: ProjectionBackend>(...)`
- Single concept to understand

**Cons**:
- Hides what capabilities are actually needed
- Prevents using traits individually elsewhere
- Doesn't improve user experience (blanket impl means no explicit implementation needed anyway)

**Why Rejected**: The trait bounds in the function signature already express the requirement. A consolidated trait adds a name without adding clarity. Individual traits remain useful for other purposes.

### Alternative 2: Keep Builder Pattern

**Description**: Retain the `ProjectionRunner::new(...).with_...().run()` pattern but improve it:

```rust
ProjectionRunner::new(projector)
    .with_backend(&store)  // Sets all three if backend implements all traits
    .run()
```

**Pros**:
- Familiar builder pattern
- Could support optional configuration

**Cons**:
- Intermediate struct serves no purpose
- More verbose than free function
- Inconsistent with `execute()` pattern

**Why Rejected**: The builder pattern adds ceremony without benefit. Configuration can be handled via a separate `run_projection_with_config()` function if needed.

### Alternative 3: Separate Parameters with Defaults

**Description**: Accept each backend separately but allow omission when same:

```rust
run_projection(
    projector,
    events: &store,
    checkpoints: &store,  // Could default to `events` if not specified
    coordinator: &store,  // Could default to `events` if not specified
)
```

**Pros**:
- Explicit about each capability
- Supports mixed backends directly

**Cons**:
- Rust doesn't have default parameters
- Would need builder or Options for "same as events"
- Common case becomes verbose

**Why Rejected**: Rust's type system doesn't support default parameters. Any approximation (Options, builders) makes the common case worse to support the uncommon case.

### Alternative 4: Generic Over Each Backend Type

**Description**: Three type parameters, each with its own bound:

```rust
pub async fn run_projection<P, E, C, X>(
    projector: P,
    event_reader: &E,
    checkpoint_store: &C,
    coordinator: &X,
) -> Result<(), ProjectionError>
where
    P: Projector,
    E: EventReader,
    C: CheckpointStore,
    X: ProjectorCoordinator,
```

**Pros**:
- Maximum flexibility
- Each backend can be different type

**Cons**:
- Common case requires: `run_projection(p, &store, &store, &store)`
- Verbose and repetitive
- Doesn't express "these are usually the same thing"

**Why Rejected**: Optimizes for the uncommon case at the expense of the common case. The wrapper struct pattern handles mixed backends with less ongoing verbosity.

## References

- ADR-010: Free Function API Design Philosophy
- ADR-026: Subscription Table + Advisory Lock Coordination
- ADR-028: Advisory Lock Acquisition Behavior
- ADR-023: Projector Coordination for Distributed Deployments (superseded by ADR-026)
