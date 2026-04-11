# ADR-0035: Event Schema Evolution via Enum Variants

## Status

Accepted

## Context

When event schemas evolve over time (new fields, changed types, removed
fields), previously persisted events may fail to deserialize against
updated Rust types. Applications need a strategy for evolving event
schemas without breaking replay of historical events.

eventcore stores events as JSON blobs via serde. The JSON uses serde's
default externally-tagged enum format, where the variant name is the
JSON key:

```json
{ "MoneyDeposited": { "account_id": "acct-123", "amount": 100 } }
```

Deserialization is driven entirely by the JSON structure — the
`event_type` column stored alongside event data is informational
metadata, not used for routing.

### Additive Changes

For backwards-compatible changes (new optional fields), applications
use `#[serde(default)]` at the field level. Old events deserialize
with the default value. No eventcore changes needed.

### Non-Backwards-Compatible Changes

For incompatible changes (field removal, type changes, semantic
changes), the question is: transform old events into new shapes
(upcasting), or handle multiple event shapes explicitly?

## Decision

Event schema evolution uses **new enum variants** rather than upcasting.

When an event's schema changes incompatibly, the application adds a new
variant to the event enum:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
enum AccountEvent {
    // Original variant — still deserializes from historical events
    MoneyDeposited {
        account_id: StreamId,
        amount: MoneyAmount,
    },
    // New variant with incompatible schema
    MoneyDepositedV2 {
        account_id: StreamId,
        amount: MoneyAmount,
        source: DepositSource,
        reference_id: ReferenceId,
    },
}
```

### Rules

1. **Old variants are never removed** — they represent historical facts
2. **`apply()` handles all variants** — pattern matching covers every
   version of the event
3. **`handle()` emits only the latest variant** — new events use the
   current schema
4. **Projectors handle all variants** — read models must process the
   full history

### Example: apply() with multiple versions

```rust
fn apply(&self, state: Self::State, event: &AccountEvent) -> Self::State {
    match event {
        AccountEvent::MoneyDeposited { amount, .. } => {
            state.deposit(*amount)
        }
        AccountEvent::MoneyDepositedV2 { amount, source, .. } => {
            state.deposit_with_source(*amount, source.clone())
        }
    }
}
```

## Consequences

### Positive

- No new infrastructure — works with existing serde and eventcore
- Events remain immutable facts — no transformation layer mutating
  historical data
- Explicit in code — `match` arms show all versions a handler supports
- Compiler enforces exhaustive matching — adding a variant forces all
  handlers to be updated
- No version metadata or upcast registry to maintain
- No migration step — old and new events coexist naturally

### Negative

- Event enums grow over time with historical variants
- Handlers must match all variants, even obsolete ones
- No way to "clean up" old variants without a full event store migration

### Why Not Upcasting

An upcasting system (registry of `fn(Value) -> Value` transformations)
was considered and rejected:

- Adds a new subsystem (version storage, upcast registry, chain
  application) for a problem serde already handles
- Transforms historical events, violating immutability
- Version metadata would need to be stored alongside events, changing
  the storage schema across all backends
- The `event_type` column currently uses `std::any::type_name` which
  is not stable — upcasting would depend on stable type identification
- Enum variants achieve the same result with zero infrastructure
