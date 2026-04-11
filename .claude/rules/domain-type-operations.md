# Domain Types Encapsulate Their Inner Values

Nutype wrapper types (and all domain value types) should expose domain
operations rather than requiring callers to extract the inner value.

## The Rule

When code needs to operate on a domain type's value, prefer adding a
domain function or trait impl to the type over extracting the inner
value with `into_inner()` or `into()`.

## Patterns

### Arithmetic

```rust
// Wrong: extract inner value, do math, re-wrap
let raw: u16 = amount.into_inner();
self.cents = self.cents.saturating_add(raw);

// Right: domain method encapsulates the operation
self.deposit(amount)

// Right: From impl for type conversions needed in folds
impl From<MoneyAmount> for i32 { ... }
let balance = events.fold(0i32, |acc, e| acc + i32::from(*amount));
```

### Collections and Comparisons

```rust
// Wrong: extract to raw primitives for comparison
let amounts: Vec<u16> = events.filter_map(|e| Some(amount.into_inner()));
assert!(amounts.contains(&50));

// Right: keep domain types throughout
let amounts: Vec<MoneyAmount> = events.filter_map(|e| Some(*amount));
assert!(amounts.contains(&test_amount(50)));
```

### Sorting

```rust
// Wrong: sort by extracting inner value
amounts.sort_by_key(|a| a.into_inner());

// Right: derive Ord on the domain type
#[nutype(derive(PartialOrd, Ord, ...))]
struct MoneyAmount(u16);
amounts.sort();
```

## When Extracting the Inner Value Is Acceptable

- **IO boundaries**: SQL parameter binding, JSON serialization (prefer
  serde where possible), logging format arguments
- **FFI or external API calls** that require raw primitives
- **Display/Debug formatting** (though prefer deriving these)

These are the edges of the system where the domain type necessarily
converts to a representation the outside world understands.

## Derives to Consider

When defining a nutype domain type, consider what operations callers
will need and derive accordingly:

- `Into` — for IO boundary extraction (always include)
- `PartialOrd, Ord` — if the type will be sorted or compared
- `Serialize, Deserialize` — for persistence (usually via serde)
- `From<T> for OtherType` — manual impl when cross-type arithmetic
  is needed (e.g., `From<MoneyAmount> for i32`)

## Applies To

- All nutype wrapper types in `eventcore-types`
- Domain types in application code using eventcore
- Test-specific domain types (same encapsulation discipline applies)

## Why

Extracting inner values defeats the purpose of the wrapper. If every
caller does `amount.into_inner()` to do math, the type is just
ceremony — it doesn't actually protect the domain boundary. Domain
operations keep the invariants enforced and make the code read in
domain language rather than primitive operations.
