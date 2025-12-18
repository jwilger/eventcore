# ADR-020: Subscribable Trait for Subscription Participation

## Status

accepted

## Context

ADR-012 established the Event trait for domain-first design, defining how domain types participate in the event sourcing write path. ADR-016 established the EventSubscription trait for read-side subscriptions. As implementation progressed, a gap emerged between what the Event trait provides and what subscriptions need.

**The Core Problem:**

The Event trait answers write-side questions:
- **"What stream do I belong to?"** - `stream_id()` returns aggregate identity
- **"What am I called?"** - `event_type_name()` returns the type's name for storage
- **"What types exist in my enum?"** - `all_type_names()` returns variant names for enum events

Subscriptions need to answer different questions:
- **"What event types can I handle?"** - Which stored events should be delivered to this subscriber
- **"How do I deserialize stored events?"** - Converting raw bytes back to domain types

These are fundamentally different concerns. A view enum aggregating MoneyDeposited and MoneyWithdrawn events cannot meaningfully answer "what stream do I belong to?" because it spans multiple streams. Yet it absolutely can answer "what event types can I handle?" and "how do I deserialize them?"

**Key Forces:**

1. **View Enums Cannot Implement Event**: View enums like `AccountEventView { Deposited(MoneyDeposited), Withdrawn(MoneyWithdrawn) }` aggregate events from different streams. They have no single `stream_id` and are never stored directly - they exist only for subscription consumption.

2. **Type Name Filtering vs TypeId**: Rust's `TypeId::of::<E>()` treats all enum variants as the same type. Subscription filtering needs to distinguish "Deposited" from "Withdrawn" - the stored type name, not the Rust type.

3. **Zero-Cost Adoption for Existing Events**: The 99% case is subscribing to concrete Event types. Requiring manual Subscribable implementation for every Event would be burdensome.

4. **CQRS Alignment**: Event trait serves the write side (commands store events). Subscribable trait serves the read side (projections consume events). Separation reflects CQRS boundaries.

5. **Error Handling**: Subscriptions must handle deserialization failures gracefully (schema evolution, corrupt data). The deserializer needs to be part of the subscription contract.

**Why This Decision Now:**

During eventcore-017 implementation, tests demonstrated the need to subscribe to view enums that aggregate disjoint event types. The existing `EventSubscription::subscribe<E: Event>()` bound blocked this pattern - view enums cannot implement Event because they have no stream_id. The trait separation was necessary to unblock multi-type subscription use cases.

## Decision

EventCore introduces a separate `Subscribable` trait for subscription participation, distinct from the `Event` trait for storage participation.

**1. Subscribable Trait Definition**

```rust
pub trait Subscribable: Clone + Send + 'static {
    /// Returns the set of event type names this subscribable type can handle.
    fn subscribable_type_names() -> Vec<EventTypeName>;

    /// Attempts to deserialize stored event data into this type.
    fn try_from_stored(type_name: &EventTypeName, data: &[u8]) -> Result<Self, SubscriptionError>;
}
```

**2. Blanket Implementation for Event Types**

All types implementing `Event` automatically implement `Subscribable`:

```rust
impl<E: Event> Subscribable for E {
    fn subscribable_type_names() -> Vec<EventTypeName> {
        E::all_type_names()  // Delegate to Event trait
    }

    fn try_from_stored(_type_name: &EventTypeName, data: &[u8]) -> Result<Self, SubscriptionError> {
        serde_json::from_slice(data)  // Generic serde deserialization
    }
}
```

**3. EventSubscription Bound Change**

```rust
// Before: E must implement Event
fn subscribe<E: Event>(&self, query: SubscriptionQuery) -> ...

// After: E must implement Subscribable (which Event types do automatically)
fn subscribe<E: Subscribable>(&self, query: SubscriptionQuery) -> ...
```

**4. View Enum Pattern**

View enums implement Subscribable manually:

```rust
enum AccountEventView {
    Deposited(MoneyDeposited),
    Withdrawn(MoneyWithdrawn),
}

impl Subscribable for AccountEventView {
    fn subscribable_type_names() -> Vec<EventTypeName> {
        vec![
            EventTypeName::try_new("MoneyDeposited").unwrap(),
            EventTypeName::try_new("MoneyWithdrawn").unwrap(),
        ]
    }

    fn try_from_stored(type_name: &EventTypeName, data: &[u8]) -> Result<Self, SubscriptionError> {
        match type_name.as_ref() {
            "MoneyDeposited" => {
                let e: MoneyDeposited = serde_json::from_slice(data)?;
                Ok(AccountEventView::Deposited(e))
            }
            "MoneyWithdrawn" => {
                let e: MoneyWithdrawn = serde_json::from_slice(data)?;
                Ok(AccountEventView::Withdrawn(e))
            }
            _ => Err(SubscriptionError::UnknownEventType(type_name.clone())),
        }
    }
}
```

**5. Result Items for Error Propagation**

Subscription streams yield `Result<E, SubscriptionError>` items, allowing consumers to handle deserialization failures gracefully per ADR-018's at-least-once delivery semantics.

## Rationale

**Why Separate Trait Over Extending Event:**

The Event trait's `stream_id()` method is fundamentally incompatible with view enums. View enums aggregate events from multiple streams - they have no single aggregate identity. Forcing view enums to implement a meaningless `stream_id()` violates interface segregation and creates confusion.

Separation also aligns with Single Responsibility Principle:
- Event trait: "How am I stored?" (stream identity, type name, serialization)
- Subscribable trait: "How am I consumed?" (type matching, deserialization)

**Why Blanket Implementation:**

The blanket `impl<E: Event> Subscribable for E` provides zero-cost adoption. Developers already implementing Event get Subscribable for free. The 99% use case (subscribing to concrete event types) requires no additional code.

This mirrors Rust's standard library patterns (e.g., `impl<T: Iterator> IntoIterator for T`).

**Why Type Name-Based Filtering:**

Rust's `TypeId::of::<AccountEvent>()` returns the same value whether the actual variant is `AccountEvent::Deposited` or `AccountEvent::Withdrawn`. For subscription filtering, we need to distinguish these cases.

`EventTypeName` strings (e.g., "MoneyDeposited", "MoneyWithdrawn") provide variant-level discrimination. When a subscriber wants only deposit events, they can filter by type name even though deposits and withdrawals share an enum type.

This approach also enables cross-language interop - type names are strings that any system can understand, unlike Rust-specific TypeIds.

**Why `try_from_stored` Returns Result:**

Schema evolution happens. Events stored months ago may not match current struct definitions. Rather than panic or silently drop events, `try_from_stored` returns `Result<Self, SubscriptionError>`.

This enables consumer choice:
- Strict consumers can fail fast on any deserialization error
- Tolerant consumers can skip incompatible events and continue
- Logging consumers can record errors for investigation

Per ADR-016's at-least-once semantics, consumers must be idempotent anyway. Handling deserialization errors is part of robust consumption.

**Trade-offs Accepted:**

- **Manual impl for view enums**: Developers must write `subscribable_type_names()` and `try_from_stored()` for each view enum. This is acceptable because view enums are relatively rare and the implementation is mechanical.

- **String-based type matching**: Type safety is reduced compared to Rust's type system. Mitigated by EventTypeName validation (non-empty, max length) and consistent use of `event_type_name()` throughout storage.

- **No automatic schema evolution**: Unlike some frameworks, EventCore doesn't auto-upgrade events. This matches Rust's explicit philosophy - schema changes require explicit migration code.

## Consequences

### Positive

- **View enums work naturally**: Multi-type subscription use cases unblocked without forcing meaningless Event implementations
- **Zero-cost for existing code**: Blanket impl means Event types work unchanged
- **CQRS boundaries respected**: Write trait (Event) and read trait (Subscribable) are clearly separated
- **Type name filtering enables variant discrimination**: Can filter enum variants individually
- **Error handling built in**: `try_from_stored` returning Result enables graceful schema evolution handling
- **Cross-language compatible**: String-based type names work across language boundaries

### Negative

- **Two traits to understand**: Developers must learn both Event and Subscribable, though the blanket impl hides Subscribable in common cases
- **Manual view enum implementation**: View enums require boilerplate `match` on type names
- **String-based matching loses some type safety**: Typos in type name strings caught at runtime, not compile time
- **EventTypeName proliferation**: Type names appear in Event::event_type_name(), Subscribable::subscribable_type_names(), and try_from_stored() match arms

### Enabled Future Decisions

- **Derive macro**: `#[derive(Subscribable)]` could auto-generate view enum implementations from variant type annotations
- **Schema versioning**: Future `try_from_stored` implementations could accept version metadata for explicit schema migration
- **Projection registration**: Central registry of projections could validate that subscribable_type_names() matches stored events
- **Cross-aggregate projections**: View enums naturally express projections spanning multiple aggregate types

### Constrained Future Decisions

- **Subscribable bound is permanent**: `EventSubscription::subscribe` now bounds on Subscribable, not Event
- **Type name strings are storage format**: Changing from EventTypeName strings would require migration
- **try_from_stored signature is stable**: Adding parameters would break existing Subscribable implementations

## Alternatives Considered

### Alternative 1: Add Optional stream_id() to Event Trait

**Description**: Make `stream_id()` return `Option<&StreamId>`, allowing view enums to return `None`.

**Why Rejected**:

This conflates two different concepts. Events ALWAYS have a stream_id - it's their aggregate identity, a fundamental property. Making it optional muddies the semantics of what an Event is. View enums aren't events; they're aggregations of events. Forcing them into the Event trait creates a category error.

### Alternative 2: Generic Type Parameter on EventSubscription

**Description**: `EventSubscription<Filter>` where Filter specifies subscription behavior.

**Why Rejected**:

This pushes complexity to the trait level rather than the type level. Every EventSubscription implementation would need to handle arbitrary Filter types. The current design keeps EventSubscription simple (one subscribe method) and moves filtering to SubscriptionQuery + type-level Subscribable.

### Alternative 3: Marker Trait Instead of Subscribable

**Description**: Empty `trait Subscribable: Event {}` as a marker, with subscription logic elsewhere.

**Why Rejected**:

Marker traits can't carry behavior. We need `subscribable_type_names()` and `try_from_stored()` to be part of the contract. A marker trait would require additional infrastructure to provide this behavior, adding complexity without benefit.

### Alternative 4: Closure-Based Subscription Registration

**Description**: `subscribe(type_names: Vec<String>, deserializer: fn(&[u8]) -> E)`

**Why Rejected**:

Closures are harder to compose and test than trait implementations. The trait-based approach allows type inference, blanket implementations, and standard derive patterns. Closures would require manual registration for every subscription.

### Alternative 5: TypeId-Based Filtering

**Description**: Use `TypeId::of::<E>()` instead of string type names.

**Why Rejected**:

TypeId treats all enum variants as the same type. `TypeId::of::<AccountEvent>()` is identical for both `AccountEvent::Deposited` and `AccountEvent::Withdrawn`. This makes variant-level filtering impossible. TypeIds are also Rust-specific, preventing cross-language interop.

## References

- **ADR-012**: Event Trait for Domain-First Design (establishes Event trait contract)
- **ADR-016**: Event Subscription Model (establishes EventSubscription trait)
- **ADR-018**: Subscription Error Handling Strategy (Result items, at-least-once semantics)
- **eventcore-017**: Subscription Foundation (implementation that revealed the need for Subscribable)
