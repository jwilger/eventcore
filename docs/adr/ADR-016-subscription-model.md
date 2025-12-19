# ADR-016: Event Subscription Model

## Status

superseded by ADR-021

## Implementation Status

**Implemented:**
- `EventSubscription` trait with `subscribe()` method returning `Stream<Item = Result<E, SubscriptionError>>`
- `Subscribable` trait for subscription type filtering (ADR-020)
- `SubscriptionQuery` with `filter_stream_prefix()` and `filter_event_type_name()` filters
- `InMemoryEventStore` and `PostgresEventStore` implementations
- Push-based `futures::Stream` delivery

**Not Yet Implemented:**
- `SubscriptionCoordinator` trait (see eventcore-018)
- `ActiveSubscription` with checkpoint management
- Consumer group coordination and rebalancing
- Glob pattern matching for stream prefixes (see eventcore-ihm)

## Context

EventCore deferred subscription design in ADR-002, establishing only the EventStore trait for atomic read/append operations. Now, as library users begin building read models and projections, the absence of subscription capabilities blocks the read side of CQRS.

**The Core Problem:**

Event-sourced systems require two fundamentally different stream access patterns:

1. **Write Pattern (Commands)**: Append events to specific aggregate streams, read entire stream for state reconstruction
2. **Read Pattern (Projections)**: Subscribe to cross-cutting event queries (all events, all events of type X, all events in category Y)

These patterns differ in lifecycle, semantics, and backend requirements—conflating them in EventStore would violate single responsibility.

**Key Forces:**

1. **Lifecycle Mismatch**: Commands are short request-response operations; subscriptions are long-lived stateful connections that must survive network failures and backend restarts
2. **Type Safety Requirements**: Domain model uses `StreamId` for aggregate identity (per ADR-012), but projection queries span multiple streams—mixing these concepts would confuse domain vs infrastructure concerns
3. **Production Deployment Reality**: Horizontally scaled applications are standard, not exceptional—multiple processes must coordinate subscription ownership without duplicate processing or orphaned subscriptions
4. **Backend Capability Variation**: In-memory stores cannot support durable subscriptions; file-based stores cannot efficiently deliver push-based streams; PostgreSQL can support full coordination via advisory locks
5. **Consumer Ergonomics**: Rust async ecosystem expects `futures::Stream`, not poll loops or callbacks—subscriptions must integrate naturally with `StreamExt`, `select!`, and async combinators
6. **Checkpoint Management Trade-offs**: Applications have diverse persistence needs (in-memory read models, per-event vs batched checkpoints, Redis vs PostgreSQL storage)—EventCore cannot impose one approach

**Why This Decision Now:**

ADR-002 deferred subscription design with the assumption that future requirements would clarify the design. Three drivers force this decision now:

1. **User Demand**: Library consumers building projections are blocked without subscription support
2. **Pattern Clarity**: Implementation experience with EventStore revealed that writable streams (aggregates) and read-only queries (projections) have fundamentally different semantics
3. **Production Readiness Gap**: EventCore must support distributed deployments, not just single-process development—coordination is not optional for real applications

## Decision

EventCore will provide event subscriptions through:

1. **Separate EventSubscription Trait**: Distinct from EventStore, acknowledging different lifecycle and semantics
2. **Composable SubscriptionQuery Type**: Struct-based filter chain (not magic strings or enum variants)
3. **Push-Based futures::Stream Delivery**: Native Rust async pattern, not poll loops
4. **Built-In SubscriptionCoordinator Trait**: First-class distributed coordination support for production deployments
5. **Associated Coordinator Type**: Type-safe backend capability expression
6. **Coordinator-Owned Checkpointing**: Checkpoint method on `ActiveSubscription` validates ownership and persists; consumer controls timing
7. **At-Least-Once Delivery**: Consumers must be idempotent; brief overlap during rebalancing is possible

**Core Design Principles:**

- **Separation of Concerns**: EventStore (commands) and EventSubscription (projections) are separate traits
- **Domain-First Types**: StreamId remains aggregate identity; projection queries use SubscriptionQuery
- **Type-Safe Composition**: Filter chains composable via methods, not string concatenation
- **Backend Flexibility**: Backends opt into capabilities via trait implementation
- **Production-Ready by Default**: Coordination is core, not an afterthought or plugin
- **Ownership Through Checkpointing**: Checkpoint validates subscription ownership; revocation discovered via checkpoint failure
- **At-Least-Once with Idempotency**: Delivery guarantees at-least-once; consumers must handle duplicates

## Rationale

**Why Separate EventSubscription Trait:**

The fundamental question was whether to extend EventStore or create a new trait. Choosing separation came down to recognizing that subscriptions and commands solve different problems:

- **Commands modify state atomically**—they must enforce aggregate boundaries, validate business rules, and guarantee consistency
- **Subscriptions observe state changes over time**—they must survive failures, resume from checkpoints, and handle rebalancing across processes

Conflating these in one trait would force every backend to support both patterns, even when that doesn't make sense (e.g., file-based event stores cannot efficiently push events). More critically, it would blur the conceptual boundary between *modifying* the event stream and *observing* it—a distinction central to CQRS.

ADR-002 anticipated this separation. Implementation experience confirmed it: subscription coordination requires heartbeat tables, rebalancing logic, and checkpoint management—none of which belong in the write path.

**Why Composable Filter Chain Over Magic Strings or Enum:**

Three API shapes were considered for subscription queries:

1. **Magic strings**: `subscribe("$et-MoneyDeposited")` - simple but error-prone, no IDE support
2. **Enum variants**: `SubscriptionQuery::EventType("MoneyDeposited")` - type-safe but not composable
3. **Filter chain**: `SubscriptionQuery::all().filter_event_type::<MoneyDeposited>()` - composable and discoverable

The filter chain won because projection queries are naturally multi-dimensional. A projection might want "all events from `account-*` streams of type `MoneyDeposited` where metadata `tenant=acme`". Enums force you to proliferate variants for every combination. Strings push all validation to runtime.

The filter chain aligns with ADR-003's type-driven development: invalid query combinations are caught at compile time, and IDE autocomplete guides developers to valid filter methods.

**Why Push-Based futures::Stream Over Poll Loops:**

Subscriptions could be poll-based (consumer calls `poll_next()` in a loop) or push-based (backend yields items via `futures::Stream`). Push-based won for ecosystem alignment:

- Rust async code expects `Stream`, not custom poll APIs
- `StreamExt` combinators (`.map()`, `.filter()`, `.take()`) are essential for projection logic
- Testing is cleaner: `stream.take(10).collect::<Vec<_>>().await` vs managing poll loops
- Integration with `select!` and `join!` requires `Stream`

The trade-off is backend complexity—push requires spawning tasks to deliver events. But this complexity is backend-internal and delivers better ergonomics for every library consumer.

**Why Built-In Coordination Over External Solutions:**

The critical insight: **coordination is not optional for production deployments**. Without it, multiple processes will duplicate work, orphan subscriptions, and create race conditions.

External coordination (etcd, Consul, Zookeeper) was rejected because:

1. **Operational burden**: Requires deploying separate infrastructure
2. **Integration complexity**: Application code must coordinate lock lifecycle with subscription lifecycle
3. **Semantic mismatch**: Generic locks don't understand subscription-specific concepts like rebalancing or checkpoint resumption

Kafka's consumer group model proves this works: coordination is *part of* the streaming system, not bolted on afterward. EventCore follows this pattern, using PostgreSQL advisory locks for coordination when PostgreSQL is the event backend. This means production-ready coordination with zero additional infrastructure.

**Why Separate SubscriptionCoordinator Trait:**

Coordination could be methods on EventSubscription, but separating them clarifies that these are orthogonal concerns:

- `EventSubscription` delivers events (data plane)
- `SubscriptionCoordinator` assigns subscriptions to processes (control plane)

This separation allows:
- Single-process applications to ignore coordination entirely
- Using different backends for events vs coordination (PostgreSQL for events, etcd for coordination)
- Testing event delivery without coordination complexity

**Why Associated Type for Coordinator:**

The coordinator is exposed via an associated type (`type Coordinator: SubscriptionCoordinator`) rather than a concrete type or generic parameter. This provides compile-time capability checking:

- Code requiring coordination can bound `S::Coordinator: SubscriptionCoordinator`
- Code that doesn't care can use `S: EventSubscription` without coordinator constraints
- InMemoryEventStore can use `type Coordinator = ()` to signal no coordination support

This design prevents runtime surprises: code written for distributed deployments won't accidentally compile against backends that don't support it.

**Why Checkpointing is a Coordinator Concern:**

Checkpointing serves two purposes in EventCore:

1. **Progress tracking**: Remember which events have been processed for resumption after restart
2. **Ownership assertion**: Prove that this consumer still owns the subscription

These purposes are coupled. When a subscription is reassigned during rebalancing, the new owner takes over from the last checkpoint. The old owner discovers revocation when their checkpoint attempt fails—they no longer own the subscription. This is how at-least-once delivery works: brief overlap between old and new owners is possible, but the checkpoint mechanism eventually forces convergence.

Because of this coupling, the checkpoint method belongs on `ActiveSubscription` (returned by the coordinator), not on the raw `EventStream`:

```rust
// Checkpoint is on ActiveSubscription, implemented by coordinator
let active: ActiveSubscription<E> = membership.assignments().next().await?;
while let Some(event) = active.events.next().await {
    process(event);
    active.checkpoint(event.position).await?;  // Validates ownership, persists
    // Returns Err(Revoked) if this subscription was reassigned
}
```

This design makes ownership validation automatic—consumers cannot checkpoint without going through the coordinator. The coordinator validates ownership and handles persistence. *Where* checkpoints are stored is a coordinator configuration choice (same database as events, separate storage, etc.), and *when* to checkpoint remains consumer choice (every event, batched, on timeout).

**Why StreamVersion for Checkpoint Type:**

Checkpoints could be opaque strings (backend-specific format) or a typed value. `StreamVersion` was chosen because:

1. **Semantic alignment**: Checkpoints *are* stream positions—the same concept as EventStore's `StreamVersion`
2. **Type reuse**: Consistency across the API reduces cognitive load
3. **No parsing overhead**: Backends don't serialize/deserialize; they use the value directly

For global subscriptions (`SubscriptionQuery::all()`), `StreamVersion` represents global position (based on monotonic sequence number ordering assigned at append time). This maintains type uniformity while supporting different query semantics.

**Trade-offs Accepted:**

- **Backend implementation complexity**: Push-based streams require background tasks; coordination requires heartbeat tables and rebalancing logic—accepted for production-readiness and consumer ergonomics
- **No automatic checkpoint persistence**: Consumers must handle checkpointing—accepted for architectural flexibility
- **Coordination requires persistent storage**: Cannot use purely in-memory backends for distributed coordination—accepted because distributed systems inherently require shared state

## Consequences

### Positive

- **Production-ready from day one**: Horizontal scaling works out of the box; no external coordination infrastructure required
- **Type-safe projection queries**: Compile-time prevention of invalid query construction; IDE autocomplete reveals available filters
- **Natural Rust async integration**: `futures::Stream` enables `StreamExt` combinators, `select!`, and standard async patterns
- **Clear conceptual boundaries**: Commands modify (EventStore), subscriptions observe (EventSubscription)—no confusion about responsibilities
- **Flexible checkpoint strategies**: Applications choose persistence approach (in-memory, batched, per-event) without library constraints
- **Backend capability transparency**: Associated types reveal at compile time whether a backend supports coordination

### Negative

- **Backend implementation burden**: Implementers must build push-based delivery, heartbeat management, and rebalancing logic—significantly more complex than simple read operations
- **Coordination latency**: Rebalancing takes time; subscriptions pause during membership changes (typically seconds, not milliseconds)

- **At-least-once delivery semantics**: Subscriptions guarantee events will be delivered at least once, but may be delivered multiple times during rebalancing, failures, or restarts. Consumers are responsible for idempotent event handling—processing the same event twice must produce the same result.
- **Consumer revocation via checkpoint failure**: During rebalancing, subscriptions may be reassigned to other processes. The previous owner discovers revocation when their checkpoint attempt fails (they no longer own the subscription). This allows the coordinator to reassign ownership while the old consumer continues processing buffered events—the old consumer will stop when checkpoint fails, while the new consumer starts from the last successful checkpoint. Brief overlap is possible, which is why idempotency is required.
- **Pattern matching complexity**: Glob pattern semantics must be precisely defined across all backends; edge cases (escaping, Unicode) require careful specification
- **Persistent storage requirement**: Distributed coordination requires durable state; purely in-memory backends cannot participate in coordination (acceptable limitation, but worth noting)

### Enabled Future Decisions

- **Composable filter expansion**: Add `.filter_event_type::<E>()`, `.filter_metadata(key, value)` without breaking existing queries
- **Checkpoint persistence helpers**: Provide optional checkpoint managers for common patterns (PostgreSQL, Redis) while preserving consumer control
- **Subscription processing middleware**: Add batching, dead letter queues, circuit breakers as combinators on `EventStream` (processing behavior), distinct from query-side filtering on `SubscriptionQuery` (event selection)
- **Custom assignment strategies**: Extend SubscriptionCoordinator to support sticky assignment, weighted distribution, partition-aware routing
- **Projection utilities**: Build fold/reduce helpers, aggregate builders, and projection templates on top of subscription primitive
- **Enhanced pattern matching**: Support more sophisticated glob syntax (character classes, brace expansion) if use cases emerge
- **Cross-datacenter coordination**: Extend SubscriptionCoordinator for multi-region deployments with eventual consistency

### Constrained Future Decisions

- **At-least-once delivery is fundamental**: Subscriptions provide at-least-once semantics—consumers must be idempotent. Exactly-once would require two-phase commit or similar complexity that conflicts with the event sourcing model.
- **Checkpointing on ActiveSubscription**: Checkpoint calls go through `ActiveSubscription.checkpoint()` which validates ownership via the coordinator—cannot be purely external or on raw EventStream. This is how consumers discover revocation.
- **Trait separation is permanent**: EventSubscription and EventStore cannot merge—commands and subscriptions remain distinct concepts
- **Coordinator separation is permanent**: SubscriptionCoordinator remains separate from EventSubscription—data plane and control plane stay decoupled
- **StreamId domain focus**: StreamId cannot represent projection queries—it remains aggregate identity per ADR-012
- **Type-safe queries only**: Subscription queries must use SubscriptionQuery builder pattern—cannot accept magic strings or opaque query objects
- **Checkpoint type is fixed**: `StreamVersion` is the checkpoint type—cannot use backend-specific string formats
- **Push-based delivery is fixed**: Subscriptions return `futures::Stream`—cannot revert to poll-based or callback models
- **Coordination is opt-in by backend**: Backends explicitly declare coordination support via associated type—cannot assume all backends coordinate

## Alternatives Considered

### Alternative 1: Merge Subscriptions into EventStore Trait

**Description**: Add subscription methods directly to EventStore rather than creating a separate trait.

**Why Rejected**:

ADR-002 already established the separation principle, and implementation experience validated it. Commands (EventStore) and subscriptions solve fundamentally different problems with different failure modes, lifecycle requirements, and backend capabilities. Merging them would:

- Force simple backends (file-based, basic in-memory) to implement complex subscription logic they cannot support
- Conflate write-path concerns (atomicity, consistency, aggregate boundaries) with read-path concerns (checkpoint resumption, rebalancing, push delivery)
- Prevent independent evolution—adding subscription middleware would require EventStore trait changes
- Violate single responsibility—one trait handling both state modification and state observation

The separation is not incidental convenience; it reflects the fundamental CQRS boundary.

### Alternative 2: Magic String Query Syntax (e.g., "$et-MoneyDeposited")

**Description**: Use convention-based string patterns for subscription queries, similar to EventStoreDB.

**Why Rejected**:

String-based APIs trade type safety for simplicity, but Rust's strength is compile-time guarantees. Magic strings would:

- Push all query validation to runtime—typos discovered during execution, not compilation
- Eliminate IDE autocomplete—developers must memorize or reference documentation
- Couple consumer code to backend-specific string conventions
- Make invalid query construction impossible to prevent at compile time (ADR-003 type-safety principle)

The composable `SubscriptionQuery` API provides the same flexibility with compile-time safety and IDE discoverability.

### Alternative 3: Poll-Based Subscriptions Instead of Push-Based Streams

**Description**: Consumer calls `poll_next()` in a loop rather than receiving a `futures::Stream`.

**Why Rejected**:

The Rust async ecosystem standardized on `Stream` for async sequences. Poll-based APIs would:

- Require consumers to write poll loops, manage backoff, and handle timing manually
- Prevent use of `StreamExt` combinators (`.map()`, `.filter()`, `.take()`)—essential for projection logic
- Complicate integration with `select!`, `join!`, and other async control flow
- Make testing more complex—must simulate poll timing rather than just collecting results

The backend complexity of push-based delivery (spawning tasks, managing buffers) is justified by aligning with Rust async idioms that every consumer expects.

### Alternative 4: External Coordination Systems (etcd, Consul, Zookeeper)

**Description**: Delegate subscription coordination to external distributed consensus systems.

**Why Rejected**:

External coordination shifts operational burden to library consumers and creates semantic mismatches:

- **Operational cost**: Requires deploying, monitoring, and maintaining additional infrastructure
- **Integration complexity**: Application code must coordinate lock acquisition with subscription lifecycle, error handling, and graceful shutdown
- **No rebalancing semantics**: Generic distributed locks don't understand subscription-specific concepts like checkpoint resumption or graceful revocation
- **Proliferation of failure modes**: Network partitions now affect both EventCore backend AND coordination system

For deployments already using PostgreSQL (or similar) for event storage, using the same backend for coordination via advisory locks requires zero additional infrastructure and provides tighter semantic integration.

### Alternative 5: Simple Locking Without Rebalancing

**Description**: Provide only exclusive locks on subscriptions—first process wins, others fail until lock is released.

**Why Rejected**:

Locking without rebalancing creates operational problems that every production deployment will encounter:

- **Load imbalance**: First process to start grabs all subscriptions; processes started later remain idle
- **Manual recovery**: When a process dies, an operator must manually release locks and restart subscriptions
- **No elasticity**: Adding capacity (new processes) doesn't redistribute work

The incremental complexity of rebalancing (heartbeat, timeout detection, reassignment) is small compared to the value it provides. Production systems need this; making it optional would just defer the inevitable.

### Alternative 6: StreamId Type Distinction for Writable vs Query Streams

**Description**: Create separate types `WritableStreamId` (for commands) and `QueryStream` (for subscriptions).

**Why Rejected**:

This conflates domain concepts (aggregate identity) with infrastructure access control:

- StreamId represents aggregate identity in the domain model (per ADR-012)—it's a business concept, not an access permission
- Virtual streams (`$all`, event-type projections) aren't "streams" in the aggregate sense—they're cross-cutting queries
- Type proliferation would leak into command signatures unnecessarily—commands don't care about read vs write distinction
- Runtime enforcement is still required (backend must validate append targets)—type distinction doesn't eliminate checks, just adds ceremony

Keeping `StreamId` focused on aggregate identity maintains domain-first design. Infrastructure concerns (subscription queries) belong in infrastructure types (`SubscriptionQuery`).

### Alternative 7: Associated Type vs Separate Marker Trait for Coordination

**Description**: Use `trait SupportsCoordination: EventSubscription {}` instead of `type Coordinator` associated type.

**Why Rejected**:

Marker traits prove capability but don't provide access to the capability:

- Marker trait answers "does it coordinate?" but not "what coordinator does it provide?"
- Generic code would need both `S: EventSubscription + SupportsCoordination` AND a way to get the coordinator
- Associated types are more expressive—`S::Coordinator` gives the actual type to work with
- Allows graceful degradation—`coordinator()` returns `Option<&Self::Coordinator>`, clearly signaling when unavailable

Associated types better express "backends that support coordination provide this specific coordinator type."

## References

- **ADR-002**: Event Store Trait Design (established EventSubscription as separate trait)
- **ADR-003**: Type System Patterns for Domain Safety (type-safe APIs over strings)
- **ADR-012**: Event Trait for Domain-First Design (StreamId as aggregate identity)
- **ARCHITECTURE.md**: For detailed trait signatures, struct definitions, and implementation examples (this ADR documents WHY, ARCHITECTURE.md documents HOW)
- **Martin Dilger "Understanding Event Sourcing"** Ch2-4: Projections and read models
- **EventStoreDB subscription patterns**: Inspiration for query types and coordination model
- **Kafka consumer groups**: Model for distributed subscription coordination
