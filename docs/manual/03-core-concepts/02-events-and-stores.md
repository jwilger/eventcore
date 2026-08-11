# Chapter 3.2: Events and Event Stores

Events are the heart of EventCore - immutable records of things that happened in your system. This chapter explores event design, storage, and the guarantees EventCore provides.

## What Makes a Good Event?

Events should be:

1. **Past Tense** - They record what happened, not what should happen
2. **Immutable** - Once written, events never change
3. **Self-Contained** - Include all necessary data
4. **Business-Focused** - Represent domain concepts, not technical details

### Event Design Principles

```rust
// ❌ Bad: Technical focus, present tense, missing context
#[derive(Serialize, Deserialize)]
struct UpdateUser {
    id: String,
    data: HashMap<String, Value>,
}

// ✅ Good: Business focus, past tense, complete information
#[derive(Serialize, Deserialize)]
struct CustomerEmailChanged {
    customer_id: CustomerId,
    old_email: Email,
    new_email: Email,
    changed_by: UserId,
    changed_at: DateTime<Utc>,
    reason: EmailChangeReason,
}
```

## Event Structure in EventCore

### Domain Events Implement the `Event` Trait

In EventCore your events are plain domain types. There is no framework
wrapper struct around them — a type becomes an event by implementing the
`Event` trait, which ties each event to the stream (aggregate) it belongs
to:

```rust,ignore
use eventcore::{Event, StreamId};
use serde::{Deserialize, Serialize};

/// Your domain event. Each variant carries the data of one business fact
/// and knows which stream it belongs to.
#[derive(Debug, Clone, Serialize, Deserialize)]
enum OrderEvent {
    OrderShipped {
        order_id: StreamId,
        tracking_number: TrackingNumber,
        carrier: Carrier,
        shipped_at: DateTime<Utc>,
    },
}

impl Event for OrderEvent {
    fn stream_id(&self) -> &StreamId {
        match self {
            OrderEvent::OrderShipped { order_id, .. } => order_id,
        }
    }

    fn event_type_name() -> &'static str {
        "OrderEvent"
    }
}
```

The `Event` trait requires two things:

- `stream_id()` — returns the stream this event belongs to. In DDD terms,
  this is the aggregate identity. Events carry their own stream, so there is
  no separate "event to write" envelope.
- `event_type_name()` — a stable name written to the `event_type` storage
  column for auditing and debugging. It is **not** used for deserialization,
  so it is safe to keep it stable even if the Rust type moves between
  modules.

Events are stored and returned as your concrete event type. EventCore does
**not** wrap events in a `StoredEvent` envelope.

### Event Ordering

`EventReader` returns each event with an opaque, ordered `StreamPosition`.
Projectors checkpoint this position to resume in the same delivery sequence.

Within a stream, order is stronger: every adapter returns events in contiguous
`StreamVersion` order. That order is stable, including across filesystem-store
clones and Git merges (see [Stream Versioning](#stream-versioning) below).

Across streams, `StreamPosition` is a projection cursor, not a distributed
causality or commit-order guarantee. PostgreSQL, SQLite, and the in-memory
store use UUIDv7 positions. The filesystem store uses a different rule after a
Git merge; its cross-stream cursor is replica-local, described below.

### Filesystem Store Ordering After Git Merges

The filesystem store has two deliberately different orderings:

- **Per-stream order** is canonical `StreamVersion` order and is the same in
  every clone.
- **Cross-stream projection order** is local-ingestion order: a clone assigns a
  fresh, increasing `StreamPosition` when it first sees an event. A
  merge-introduced event is therefore delivered after the events that clone had
  already processed, even if another clone saw the events in a different order.

This prevents a live projection from rewinding or silently skipping a
merge-introduced event. It also means two clones can process the same eventual
set of cross-stream events in different orders. Keep filesystem-store
projections order-independent and idempotent, or designate one clone as the
projection owner when ordering matters across streams.

### Event Metadata

EventCore does not impose a metadata schema. Per the library's infrastructure
neutrality principle, concerns like "who triggered this" (actor), correlation
IDs, and causation IDs are **owned by your application**, not by EventCore.

Model whatever audit context your domain needs as ordinary fields on your
event variants — they are part of the business fact and serialized with the
rest of the payload:

```rust,ignore
#[derive(Debug, Clone, Serialize, Deserialize)]
enum CustomerEvent {
    EmailChanged {
        customer_id: StreamId,
        old_email: Email,
        new_email: Email,
        // Application-owned audit context, carried in the event itself.
        changed_by: UserId,
        changed_at: DateTime<Utc>,
        correlation_id: CorrelationId,
    },
}
```

This keeps the audit trail immutable and self-contained: the metadata lives
inside the event, replays deterministically, and never depends on a separate
framework table whose schema you do not control.

## Event Store Abstraction

EventCore defines a trait that storage adapters implement:

```rust,ignore
pub trait EventStore {
    /// Read events from a single stream as a lazy async `Stream`.
    ///
    /// The returned `EventStream<E>` yields events on demand rather than
    /// collecting the whole history up front. When you need the events as a
    /// `Vec`, use the `collect_events` helper.
    fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> impl Future<Output = Result<EventStream<E>, EventStoreError>> + Send;

    /// Atomically append events to one or more streams with optimistic
    /// concurrency control via expected stream versions.
    fn append_events(
        &self,
        writes: StreamWrites,
    ) -> impl Future<Output = Result<EventStreamSlice, EventStoreError>> + Send;
}
```

`read_stream` returns a lazy [`EventStream`] — an async `Stream` of events.
To materialize the whole history into a `Vec` (the common case when
reconstructing state), pass it to the `collect_events` free function:

```rust,ignore
use eventcore::{collect_events, EventStream};

let stream: EventStream<MyEvent> = store.read_stream(stream_id).await?;
let events: Vec<MyEvent> = collect_events(stream).await?;
```

> **Note:** The earlier `EventStreamReader` type (which eagerly collected
> events and exposed `len()`/`first()`/`into_iter()`) was removed in
> [ADR-0049](../../adr/ADR-0049-streaming-reads.md). Use `EventStream` plus
> `collect_events` instead.

## Stream Versioning

Each stream has a `StreamVersion` — an event count that starts at `0` for an
empty stream and increments by one with every appended event. EventCore uses
this version for optimistic concurrency control: a write declares the version
it expects each stream to be at, and the append is rejected if reality has
moved on.

```rust,ignore
// StreamVersion and StreamWrites live in eventcore_types (not re-exported
// from the eventcore facade), since application code rarely touches them.
use eventcore_types::StreamVersion;

// Versions start at 0 (empty stream) and increment as events are appended.
let empty = StreamVersion::new(0);
let after_one_event = empty.increment(); // StreamVersion::new(1)
```

You almost never construct `StreamVersion` or assemble writes by hand. The
`execute()` function reads the declared streams, records their current
versions, runs your command's `handle()`, and appends the resulting events
under those versions atomically. If another writer advanced a stream in the
meantime, the append fails with
`EventStoreError::VersionConflict { stream_id, expected, actual }`, and
`execute()` automatically reloads and retries according to the
[`RetryPolicy`](./05-error-handling.md).

### How the Write Path Actually Works

When you do need to assemble a write directly against the `EventStore` trait
(for example, when building or testing a backend), the unit of work is
`StreamWrites`. You register each stream with its expected version, then
append events whose `stream_id()` matches a registered stream:

```rust,ignore
use eventcore_types::{StreamVersion, StreamWrites};

// Build the atomic write batch the same way execute() does internally.
let writes = StreamWrites::new()
    // First write: the stream is expected to be empty (version 0).
    .register_stream(account_id.clone(), StreamVersion::new(0))
    .and_then(|w| w.append(BankEvent::AccountOpened { /* ... */ }))
    .and_then(|w| w.append(BankEvent::MoneyDeposited { /* ... */ }))
    .expect("builder should succeed");

// Append atomically. Fails with EventStoreError::VersionConflict if the
// stream's current version is not the expected version.
store.append_events(writes).await?;
```

`register_stream` enforces a single expected version per stream — registering
the same stream twice with different versions returns
`EventStoreError::ConflictingExpectedVersions`. Appending an event whose
stream was never registered returns `EventStoreError::UndeclaredStream`.

> **In application code, prefer `execute()`.** Constructing `StreamWrites`
> directly bypasses state reconstruction and business-rule validation.
> Commands produce events from `handle()`, and `execute()` is the canonical
> entry point that wires versioning, retries, and atomic append together. See
> [Commands and the Macro System](./01-commands-and-macros.md).

## Storage Adapters

### PostgreSQL Adapter

The production-ready adapter with ACID guarantees:

```rust,ignore
use eventcore_postgres::PostgresEventStore;

// Connect with default pool settings.
let event_store = PostgresEventStore::new("postgresql://localhost/eventcore").await?;

// Apply the bundled schema migrations (one time). `migrate()` panics on
// failure rather than returning an error.
event_store.migrate().await;
```

To customize the connection pool, build a `PostgresConfig` and use
`with_config`. The config exposes only pool-level knobs — `max_connections`,
`acquire_timeout`, and `idle_timeout` (defaults: 10 connections, 30s acquire,
600s idle):

```rust,ignore
use std::num::NonZeroU32;
use std::time::Duration;
use eventcore_postgres::{MaxConnections, PostgresConfig, PostgresEventStore};

let config = PostgresConfig {
    max_connections: MaxConnections::new(NonZeroU32::new(20).expect("non-zero")),
    acquire_timeout: Duration::from_secs(30),
    idle_timeout: Duration::from_secs(600),
};

let event_store =
    PostgresEventStore::with_config("postgresql://localhost/eventcore", config).await?;
event_store.migrate().await;
```

If you already manage your own `sqlx` pool, hand it over with
`PostgresEventStore::from_pool(pool)`.

PostgreSQL schema:

```sql
-- Events table with optimal indexing
CREATE TABLE events (
    id UUID PRIMARY KEY DEFAULT gen_uuidv7(),
    stream_id VARCHAR(255) NOT NULL,
    version BIGINT NOT NULL,
    event_type VARCHAR(255) NOT NULL,
    payload JSONB NOT NULL,
    metadata JSONB NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,

    -- Ensure stream version uniqueness
    UNIQUE(stream_id, version),

    -- Indexes for common queries
    INDEX idx_stream_id (stream_id),
    INDEX idx_occurred_at (occurred_at),
    INDEX idx_event_type (event_type)
);
```

### SQLite Adapter

Embedded event store for single-process applications, CLI tools, and desktop/mobile apps:

```rust,ignore
use eventcore_sqlite::{SqliteEventStore, SqliteConfig};
use std::path::PathBuf;

// File-backed store
let config = SqliteConfig {
    path: PathBuf::from("./my-app-events.db"),
    encryption_key: None,
};
let store = SqliteEventStore::new(config)?;
store.migrate().await?;

// In-memory store (for testing with SQLite persistence semantics)
let store = SqliteEventStore::in_memory()?;
store.migrate().await?;
```

> **At-rest encryption is opt-in at compile time.** The `encryption_key`
> field is only honored when the crate's non-default `encryption` Cargo
> feature is enabled (which builds against SQLCipher). On the default
> `bundled` build, SQLite is plain and `encryption_key` is **silently
> ignored** — the database is written unencrypted. Enable the `encryption`
> feature before relying on a key.

SQLite schema:

```sql
CREATE TABLE eventcore_events (
    event_id TEXT PRIMARY KEY,
    stream_id TEXT NOT NULL,
    stream_version INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    event_data TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE UNIQUE INDEX idx_eventcore_events_stream_version
    ON eventcore_events (stream_id, stream_version);
CREATE INDEX idx_eventcore_events_stream_id
    ON eventcore_events (stream_id);
```

### In-Memory Adapter

Perfect for testing and development:

```rust,ignore
use eventcore_memory::InMemoryEventStore;

// Not generic — one store handles whatever event types your commands produce.
let event_store = InMemoryEventStore::new();
```

For fault-injection testing, the `eventcore-testing` crate provides a chaos
wrapper via the `ChaosEventStoreExt` extension trait. Configure failure
injection with the `ChaosConfig` builder:

```rust,ignore
use eventcore_memory::InMemoryEventStore;
use eventcore_testing::chaos::{ChaosConfig, ChaosEventStoreExt};

// Deterministic chaos (seeded) with a 10% chance of injected failures.
let chaotic_store = InMemoryEventStore::new().with_chaos(
    ChaosConfig::deterministic()
        .with_failure_probability(0.1)
        .with_version_conflict_probability(0.05),
);
```

`ChaosConfig` injects store failures and synthetic version conflicts so you
can exercise your retry handling; use `ChaosConfig::default()` for
non-deterministic behavior or `ChaosConfig::deterministic()` for a seeded,
reproducible run.

## Event Design Patterns

### Event Granularity

Choose the right level of detail:

```rust
// ❌ Too coarse - loses important details
struct OrderUpdated {
    order_id: OrderId,
    new_state: OrderState,  // What actually changed?
}

// ❌ Too fine - creates event spam
struct OrderFieldUpdated {
    order_id: OrderId,
    field_name: String,
    old_value: Value,
    new_value: Value,
}

// ✅ Just right - meaningful business events
enum OrderEvent {
    OrderPlaced { customer: CustomerId, items: Vec<Item> },
    PaymentReceived { amount: Money, method: PaymentMethod },
    OrderShipped { tracking: TrackingNumber },
    OrderDelivered { signed_by: String },
}
```

### Event Evolution

EventCore has **no upcasting subsystem, schema registry, or event
serializer** — schema evolution is handled entirely with serde. Per
[ADR-0035](../../adr/ADR-0035-event-schema-evolution-via-enum-variants.md),
there are two sanctioned techniques:

1. **Additive changes** — add a field with `#[serde(default)]` so old events
   (which lack the field) still deserialize.
2. **Incompatible changes** — introduce a **new enum variant** rather than
   mutating an existing one. Old events keep matching their original variant;
   handlers and projectors match all variants.

Design events to evolve gracefully:

```rust
// Version 1
#[derive(Serialize, Deserialize)]
struct UserRegistered {
    user_id: UserId,
    email: Email,
}

// Version 2 - Added field with default
#[derive(Serialize, Deserialize)]
struct UserRegistered {
    user_id: UserId,
    email: Email,
    #[serde(default)]
    referral_code: Option<String>,  // New field
}

// Version 3 - Structural change
#[derive(Serialize, Deserialize)]
#[serde(tag = "version")]
enum UserRegisteredVersioned {
    #[serde(rename = "1")]
    V1 { user_id: UserId, email: Email },

    #[serde(rename = "2")]
    V2 {
        user_id: UserId,
        email: Email,
        referral_code: Option<String>,
    },

    #[serde(rename = "3")]
    V3 {
        user_id: UserId,
        email: Email,
        referral: Option<ReferralInfo>,  // Richer type
    },
}
```

### Event Enrichment

If your domain needs ambient context (session, request, environment), put it
in the event itself — there is no EventCore "metadata envelope" to attach it
to. The pattern below is **application-level, illustrative code**: EventCore
defines none of these types. Capture the context at the command boundary and
include it in the event's fields so it is serialized and replayed with the
rest of the payload:

```rust,ignore
// Application-level pattern — not an EventCore API.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OrderPlaced {
    order_id: StreamId,
    items: Vec<Item>,
    // Ambient context modeled as ordinary fields on the event.
    session_id: SessionId,
    request_id: RequestId,
    environment: Environment,
}
```

## Querying Events

### Reading a Single Stream

The `EventStore` trait exposes `read_stream`, which takes a `StreamId` and
returns a lazy `EventStream<E>` of that stream's events in version order
(oldest to newest). To materialize the history into a `Vec`, pass the stream
to the `collect_events` helper:

```rust,ignore
use eventcore::{collect_events, EventStream, StreamId};

let stream_id = StreamId::try_new("order-123").expect("valid stream id");
let stream: EventStream<OrderEvent> = store.read_stream(stream_id).await?;
let events: Vec<OrderEvent> = collect_events(stream).await?;
```

Because `read_stream` yields events lazily, the executor folds them into
command state one at a time — the memory win matters for large streams.

`read_stream_after(stream_id, version)` is the corresponding incremental API.
It excludes the first `version` events and is used by command-state snapshots
to catch a saved state up with only its tail. The default implementation is
correct for custom stores but skips an opened full stream; production adapters
override it to seek directly.

> **Most applications never call `read_stream` directly.** `execute()` reads
> the declared streams, folds them through `apply()`, and reconstructs state
> for you. Direct reads are for projections and ad-hoc inspection.

### Reading Multiple Streams

A command consistency boundary can span multiple streams, but the read API is
still per-stream — read each `StreamId` and fold its events into the same
state. Inside `execute()`, EventCore does this for every stream the command
declares (via `#[stream]` fields and `#[derive(Command)]`) and merges them
into a single reconstructed state before calling `handle()`.

For read models that need a cross-stream view, build a `Projector` and drive
it with `run_projection`. The projection runner delivers events together with
the store's `StreamPosition` cursor and checkpoints progress so it can resume.
For filesystem stores that participate in Git merges, this delivery order is
replica-local rather than shared across clones. See
[Projections](../02-getting-started/04-projections.md).

## Event Store Guarantees

### 1. Atomicity

All events in a single `append_events` call succeed or fail together — even
when they span multiple streams. A command that withdraws from account A and
deposits to account B emits both events from `handle()`, and `execute()`
appends them in one atomic batch:

```rust,ignore
// handle() returns both events; execute() appends them atomically.
fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
    Ok(vec![
        BankEvent::MoneyWithdrawn { account_id: self.from.clone(), /* ... */ },
        BankEvent::MoneyDeposited { account_id: self.to.clone(), /* ... */ },
    ]
    .into())
}
```

If any stream's version check fails, the entire batch is rolled back and no
events are written.

### 2. Consistency

Version checks prevent conflicting writes. Each append declares the
`StreamVersion` it expects per stream; the store rejects the append if the
current version differs:

```rust,ignore
// Two concurrent commands both read a stream at version 5.
// The first command commits, advancing the stream to version 6.
// The second command's append now fails:
//   EventStoreError::VersionConflict { stream_id, expected: 5, actual: 6 }
//
// execute() catches this automatically, reloads state, and retries
// according to the RetryPolicy.
```

### 3. Durability

Events are persisted before `append_events` returns success. Once the call
completes, the events survive a process crash — the PostgreSQL and SQLite
backends commit the transaction before returning.

### 4. Ordering

Events maintain both stream order and a projection delivery cursor:

- **Stream order** — within one stream, events are returned in
  `StreamVersion` order (oldest to newest). This is stable across filesystem
  store clones.
- **Cross-stream cursor** — `EventReader` returns `StreamPosition` values in a
  resumable sequence. PostgreSQL, SQLite, and memory use UUIDv7 positions; the
  filesystem store uses replica-local ingestion order after Git merges. It is
  not a universal distributed commit order.

## Performance Optimization

### Batch Writing

A single command can emit many events, and they are all appended in one
atomic `append_events` call — so the natural batching unit is the command. A
command's `handle()` returns a `NewEvents` collection, and `execute()` appends
the whole collection at once under the declared stream versions:

```rust,ignore
fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
    let events: Vec<MyEvent> = self
        .items
        .iter()
        .map(|item| MyEvent::ItemProcessed { /* ... */ })
        .collect();

    Ok(events.into())
}
```

When you ingest a large external dataset, drive it as a sequence of command
executions (one per logical unit of work) rather than hand-assembling a giant
write. Each `execute()` call gets its own atomic append and optimistic
concurrency check.

### Stream Partitioning

Distribute load across streams by choosing stream identities that spread
writes out. `StreamId` is a validated domain type, so construct it with
`try_new` (the only constructor — there is no `from_static`/`new`), handling
the validation result:

```rust,ignore
use eventcore::StreamId;

// Instead of one hot stream...
let stream_id = StreamId::try_new("orders")?;

// ...partition by hash to avoid a single contended stream.
let partition = order_id.hash() % 16; // 16 partitions
let stream_id = StreamId::try_new(format!("orders-{partition}"))?;
```

### Read-Side Caching

Hot-path reads belong in your read models, not in re-reading streams.
Projections (the "Q" in CQRS) maintain denormalized, query-optimized views
that you can cache freely without affecting write correctness — read models
and write models are deliberately separate code paths.

Build a `Projector`, drive it with `run_projection`, and store its output in
whatever cache or read database suits your queries. The projection runner
checkpoints its `StreamPosition`, so it resumes incrementally rather than
re-reading history. Do **not** add an ad-hoc cache inside the write path.
Command state is normally reconstructed fresh, and the supported exception is
an explicit durable command-state snapshot described in
[State Reconstruction](./03-state-reconstruction.md#durable-command-state-snapshots).
Snapshots retain stream versions and are caught up before command handling, so
optimistic concurrency remains correct. See
[Projections](../02-getting-started/04-projections.md).

## Testing with Events

### Event Fixtures

Because events are plain domain types, test fixtures are just constructors —
no framework wrappers required. These builders are **application-level test
helpers** that live in your own test module:

```rust,ignore
// Application-level helper: build a domain event for a test.
fn account_opened() -> BankEvent {
    BankEvent::AccountOpened {
        account_id: StreamId::try_new("account-123").expect("valid stream id"),
        owner: Owner::try_new("Alice").expect("valid owner"),
        initial_balance: Money::try_new(1000).expect("valid amount"),
    }
}
```

> **Note:** Events are plain enum variants — construct them directly without
> wrappers, and parse their fields into your domain types at the boundary.

### Behavioral Assertions

Prefer testing behavior through the public API over inspecting internal event
structure. The most reliable way to verify your command logic is to run it
through `execute()` against an `InMemoryEventStore` and assert on the
observable outcome:

```rust,ignore
// Application-level test: exercise the command through execute().
#[tokio::test]
async fn deposit_records_money_deposited() {
    let store = InMemoryEventStore::new();
    let account_id = StreamId::try_new("account-123").expect("valid stream id");

    let response = execute(
        &store,
        Deposit { account_id: account_id.clone(), amount: 100 },
        RetryPolicy::new(),
    )
    .await
    .expect("deposit should succeed");

    // ExecutionResponse exposes how many attempts were needed.
    assert_eq!(response.attempts(), 1);

    // Read the stream back and assert on the events that were appended.
    let events: Vec<BankEvent> =
        collect_events(store.read_stream(account_id).await.expect("read")).await.expect("collect");
    assert!(matches!(events.as_slice(), [BankEvent::MoneyDeposited { .. }]));
}
```

For richer scenarios, the `eventcore-testing` crate provides contract-test and
chaos tooling for verifying backend behavior.

## Summary

Events in EventCore are:

- ✅ **Immutable records** of business facts
- ✅ **Resumable across-stream projection reads** via `StreamPosition`; see
  [Event Ordering](#event-ordering) for the filesystem merge-mode caveat
- ✅ **Version-controlled** per stream via `StreamVersion` for optimistic
  concurrency
- ✅ **Atomically written** across streams by `execute()` / `append_events`
- ✅ **Self-contained** — audit context lives in the event's own fields, owned
  by your application

Best practices:

1. Design events around business concepts
2. Include all necessary data (including audit context) in the event itself
3. Plan for event evolution with serde defaults and new variants (ADR-0035)
4. Let `execute()` handle versioning and atomic appends
5. Distribute load with stream partitioning, and serve reads from projections

Next, let's explore [State Reconstruction](./03-state-reconstruction.md) →
