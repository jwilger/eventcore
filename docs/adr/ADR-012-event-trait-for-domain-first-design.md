# ADR-012: Event Trait for Domain-First Design

## Status

accepted

## Context

EventCore's initial design represented events as a generic wrapper struct that contained domain event payloads. This created a separation between the infrastructure concept of an "Event" and the domain types representing what actually happened in the system.

**Original Design:**

```rust
struct Event<T> {
    payload: T,
    stream_id: StreamId,
    // ... other metadata
}
```

Domain types were wrapped inside the Event infrastructure:

```rust
let event = Event {
    payload: MoneyDeposited { amount: 100 },
    stream_id: account_id,
};
```

**Key Forces:**

1. **Domain-Driven Design Principles**: Domain types should be first-class citizens in the ubiquitous language, not wrapped in infrastructure concerns
2. **Type-Driven Development (ADR-003)**: Types should make domain concepts explicit and infrastructure implicit
3. **Developer Ergonomics**: Library consumers should work primarily with their domain types, not infrastructure wrappers
4. **Stream Identity**: StreamId represents the aggregate identity in DDD - it's a domain concept, not purely infrastructure
5. **API Clarity**: Simpler API surface when domain types are in the foreground
6. **Type Safety**: Need to maintain compile-time guarantees and type erasure for storage
7. **Async Compatibility**: Events must work with async trait methods and storage backends

**Why This Decision Now:**

During I-001 implementation (single-stream command end-to-end), the wrapper design created friction between domain modeling and infrastructure. The separation between `Event<T>` wrapper and domain types `T` forced developers to think in infrastructure terms when writing domain logic. This contradicts EventCore's philosophy of keeping domain code clean and infrastructure concerns in the background.

## Decision

Refactor Event from a generic wrapper struct to a trait that domain types implement directly.

**New Design:**

```rust
trait Event: Clone + Send + 'static {
    fn stream_id(&self) -> &StreamId;
}
```

Domain types ARE events by implementing the trait:

```rust
struct MoneyDeposited {
    account_id: StreamId,  // Aggregate identity
    amount: u64,
}

impl Event for MoneyDeposited {
    fn stream_id(&self) -> &StreamId {
        &self.account_id
    }
}
```

**API Changes:**

- `StreamWrites::append<E: Event>(event: E)` - accepts events directly, extracts stream_id automatically
- `CommandLogic` trait bound: `CommandLogic<E: Event>` instead of `CommandLogic<EventPayload>`
- `CommandLogic::apply(&self, state: Self::State, event: &E)` - domain types visible in signature
- `EventStore::read_stream<E: Event>()` - returns domain types, not wrappers
- Event metadata handled separately from domain types (still tracked by infrastructure)

**Trait Bounds Rationale:**

- **Clone**: Required for state reconstruction (apply method consumes events multiple times)
- **Send**: Required for async storage backends and cross-thread event handling
- **'static**: Required for type erasure in storage and async trait boundaries

## Rationale

**Why Event Trait Over Wrapper Struct:**

The wrapper design placed infrastructure (`Event<T>`) in the foreground and domain (`T`) in the background. This violates Domain-Driven Design principles where domain concepts should be explicit and infrastructure should fade into the background.

With the trait design:

- Domain types (MoneyDeposited, AccountCredited) are primary - developers work with these directly
- Infrastructure (Event trait) is a constraint/capability, not a wrapper
- StreamId lives naturally in the domain type where it belongs (aggregate identity)
- API calls use domain types: `append(money_deposited)` not `append(Event { payload: money_deposited })`

**Why StreamId in Domain Type:**

StreamId represents the aggregate identity in Domain-Driven Design - it's fundamentally a domain concept:

- A bank account (aggregate) has an account ID (StreamId)
- A shopping cart (aggregate) has a cart ID (StreamId)
- An order (aggregate) has an order ID (StreamId)

This identity is part of the domain model, not infrastructure configuration. Domain types naturally know which aggregate instance they belong to - making them declare it as part of their structure is appropriate.

**Why 'static Bound:**

The 'static lifetime bound is necessary for:

1. **Type Erasure in Storage**: EventStore must store heterogeneous event types. Storage backends use trait objects (`Box<dyn Any>`) to store events of different types together. Trait objects require 'static to ensure no dangling references.

2. **Async Trait Methods**: Event storage operations are async. Async methods in traits require 'static bounds because the future may outlive the function call scope.

3. **Cross-Thread Safety**: Events stored in multi-threaded storage backends must not contain references that could be invalidated when sent across threads.

**Implication**: Domain events must own their data - they cannot contain references to external data. This is acceptable because:

- Events are immutable facts meant to be persisted - they should be self-contained
- Event sourcing best practice: events should be complete records, not references
- Domain modeling typically uses owned data in event structures anyway

**Better Developer Ergonomics:**

Before (wrapper):

```rust
emit!(ctx, Event {
    stream_id: self.from_account.clone(),
    payload: MoneyWithdrawn { amount: self.amount },
});
```

After (trait):

```rust
emit!(ctx, MoneyWithdrawn {
    account_id: self.from_account,
    amount: self.amount,
});
```

The domain type is front-and-center. Infrastructure extracts what it needs (stream_id) via the trait method.

**Alignment with Type-Driven Development (ADR-003):**

ADR-003 establishes "no primitive obsession" and "domain clarity" as core principles. Having domain types wrapped in infrastructure violates this - the Event wrapper becomes a form of infrastructure obsession. The trait design keeps domain types pure and lets infrastructure concerns be expressed through trait bounds.

**Trade-offs Accepted:**

- **'static Bound Constraint**: Events cannot contain references - they must own their data
  - _Acceptable because_: Events are permanent records meant for storage; they should be self-contained

- **Slightly More Complex Trait Implementation**: Developers must implement Event trait on each domain type
  - _Acceptable because_: Implementation is trivial (one-line method returning a field reference)

- **Type Erasure Complexity**: Storage backends must handle downcasting from trait objects
  - _Acceptable because_: Complexity is internal to storage implementations, not visible to library consumers

**Consistency with EventCore Philosophy:**

From CLAUDE.md: "Domain types should be first-class, not wrapped." This change directly implements that principle. Domain events are now domain-first, with infrastructure traits providing necessary capabilities without obscuring the domain model.

## Consequences

**Positive:**

- **Domain-Driven Design Alignment**: Domain types are first-class citizens, not wrapped in infrastructure
- **Clearer Ubiquitous Language**: Code reads as domain language (MoneyDeposited, AccountCredited) not infrastructure wrappers
- **Better API Ergonomics**: Simpler, more intuitive API where domain types are visible throughout
- **StreamId as Domain Identity**: Aggregate identity lives naturally in domain model where it belongs
- **Type Safety Maintained**: Trait bounds provide same compile-time guarantees as wrapper struct
- **Less Infrastructure Noise**: Domain code focuses on business concepts, infrastructure fades to background
- **Type Inference Benefits**: Generic bounds on traits often provide better type inference than wrapper structs

**Negative:**

- **'static Lifetime Requirement**: Domain events cannot contain references - must own all data
- **Trait Implementation Required**: Each domain event type must implement Event trait (though implementation is trivial)
- **Type Erasure Complexity**: Storage implementations must handle downcasting from Any trait objects
- **Migration Impact**: Existing code using wrapper design requires updates to trait-based design

**Enabled Future Decisions:**

- Event metadata can be managed separately from domain types (infrastructure concern)
- Custom derive macro `#[derive(Event)]` could automate trait implementation based on field attributes
- Event serialization strategies can operate on trait bounds without wrapping
- Projection builders work directly with domain types, not infrastructure wrappers
- Event upcasting/downcasting patterns can leverage trait objects

**Constrained Future Decisions:**

- All domain event types must implement Clone + Send + 'static bounds
- Events cannot contain borrowed data - must own all fields
- StreamId must be accessible from domain types (part of domain model)
- Storage backends must handle type erasure and downcasting correctly

## Alternatives Considered

### Alternative 1: Keep Generic Wrapper Struct

Continue with `Event<T>` wrapper containing domain payloads.

**Rejected Because:**

- Places infrastructure in foreground, domain in background (opposite of DDD)
- API surface cluttered with Event wrapper noise
- StreamId treated as infrastructure parameter when it's domain identity
- Developers forced to think in infrastructure terms when writing domain code
- Violates ADR-003 principle of domain clarity
- Does not align with EventCore's type-driven development philosophy
- Creates unnecessary abstraction layer between developer and domain model

### Alternative 2: Event Trait Without StreamId Method

Define Event trait without stream_id method; pass stream_id separately to append operations.

**Rejected Because:**

- StreamId is aggregate identity (domain concept) - should be part of domain type
- Requires passing stream_id redundantly: `append(stream_id, event)` when event already knows its stream
- Potential for mismatch errors (passing wrong stream_id for an event)
- Less type-safe than event declaring its own stream identity
- Adds parameter ceremony to every append operation
- Doesn't reflect DDD principle that aggregates know their own identity

### Alternative 3: No Lifetime Bounds (Remove 'static)

Define Event trait without 'static bound, allowing events with references.

**Rejected Because:**

- **Type Erasure Impossible**: Cannot store events in trait objects (`Box<dyn Any>`) without 'static
- **Async Incompatibility**: Async trait methods require 'static for futures outliving function scope
- **Storage Complexity**: Backends cannot store events across thread boundaries without 'static
- **Lifetime Management Burden**: Applications must manage event lifetimes carefully, creating complexity
- **Not Event Sourcing Best Practice**: Events should be self-contained permanent records, not references

Events as immutable facts meant for persistence should be self-contained. The 'static bound enforces this best practice.

### Alternative 4: Separate Metadata Trait

Define Event trait for domain types, separate Metadata trait for infrastructure concerns.

**Rejected Because:**

- Increases complexity - two traits instead of one
- Metadata is infrastructure concern, not domain concern - should be handled by storage layer
- StreamId is domain identity, belongs in Event trait (aggregate identity)
- Other metadata (timestamps, correlation IDs) already handled separately (ADR-005)
- Additional trait doesn't provide meaningful separation of concerns
- Makes API more complex without corresponding benefit

### Alternative 5: Macro-Generated Wrapper

Use procedural macro to auto-generate Event wrapper around domain types.

**Rejected Because:**

- Still creates wrapper separation between infrastructure and domain
- Macro complexity doesn't address fundamental design issue
- Developers still think in terms of wrapped vs unwrapped types
- Doesn't achieve goal of domain-first design
- Macro maintenance burden for limited benefit
- Wrapper pattern itself is the problem, not the manual vs generated nature

## References

- ADR-003: Type System Patterns for Domain Safety (domain clarity, no primitive obsession)
- ADR-005: Event Metadata Structure (will require update to clarify metadata handled separately from Event trait)
- CLAUDE.md: Type-Driven Development principles ("Domain types should be first-class, not wrapped")
- REQUIREMENTS_ANALYSIS.md: FR-5 Type-Driven Domain Modeling
- Domain-Driven Design (Eric Evans): Aggregates and Identity
- I-001 Implementation: Single-Stream Command End-to-End (where this design emerged)
