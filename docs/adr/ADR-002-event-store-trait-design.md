# ADR-002: Event Store Trait Design

## Status

accepted

## Context

EventCore requires a pluggable storage abstraction that supports different backend implementations (PostgreSQL for production, in-memory for testing, potential future backends). This abstraction must enable ADR-001's multi-stream atomicity guarantees while remaining flexible enough to accommodate different storage technologies.

**Key Forces:**

1. **Multi-Stream Atomicity**: ADR-001 requires atomic append operations across multiple streams
2. **Backend Diversity**: PostgreSQL uses ACID transactions; in-memory uses locks; future backends may vary
3. **Implementation Encapsulation**: Atomicity mechanisms should be internal backend concerns, not exposed in consumer API
4. **Type Safety**: Rust's type system should prevent invalid usage patterns
5. **Event Ordering**: Both global ordering (UUIDv7) and stream-specific ordering (versions) are essential
6. **Subscription Support**: Projections need real-time event notifications
7. **Metadata Preservation**: Audit trails require storing causation, correlation, and custom metadata
8. **API Simplicity**: Library consumers should have straightforward read and append operations

**Why This Decision Now:**

The EventStore trait is the foundation contract between EventCore's command execution logic and storage backends. This decision shapes the API surface that library consumers interact with and defines the responsibilities that backend implementations must fulfill.

## Decision

EventCore will define two separate traits for storage abstraction:

**EventStore Trait - Core Operations:**
1. **Read Streams**: Query operation to retrieve events from one or more streams
2. **Atomic Append**: Single operation that appends events to multiple streams atomically with version checking
3. **Version-Based Concurrency**: Expected versions provided in append operation for optimistic concurrency control
4. **Metadata Preservation**: All events carry structured metadata for auditing and distributed tracing
5. **Atomicity as Implementation Detail**: HOW backends achieve atomicity (transactions, locks, etc.) is hidden from consumers

**EventSubscription Trait - Projection Support:**
1. **Separate Trait**: Subscriptions provided via companion trait, not mixed into EventStore
2. **Long-Lived Streams**: Delivers events in real-time for projection building
3. **Checkpointing**: Supports resuming from last processed position
4. **Optional Implementation**: Not all backends need to support subscriptions

## Rationale

**Why Simple Read + Append Operations:**

Event sourcing has a natural atomicity boundary: the single append operation that writes one or more events to the event log. Reading events is a query operation with no transactional semantics needed. This leads to a straightforward API with just two core operations.

Complexity of transaction management (how to achieve atomicity) is pushed into backend implementations where it belongs, not exposed in the consumer-facing API.

**Why Atomicity as Implementation Detail:**

Different backends achieve atomicity through different mechanisms:
- PostgreSQL uses ACID database transactions
- In-memory stores use mutex/lock-based synchronization
- Future backends may use other techniques

Library consumers should not need to understand or manage these mechanisms. The EventStore trait contract simply guarantees that append operations are atomic - HOW is the backend's responsibility.

**Why Separate Subscription Trait:**

Subscriptions have fundamentally different lifecycle and semantics from read/append operations:
- Subscriptions are long-lived event streams; read/append are short request-response operations
- Subscriptions deliver events asynchronously and continuously; read/append are one-shot operations
- Not all backends may support subscriptions (e.g., simple file-based stores)
- Different error modes and recovery strategies
- Separating concerns keeps EventStore trait focused and enables backends to implement only what they support

**Why Explicit Version Checking:**

Making expected versions explicit in append operations:
- Aligns with optimistic concurrency control pattern (read, compute, write with version check)
- Makes conflict detection part of the API contract
- Enables backends to implement version checking efficiently (e.g., PostgreSQL CHECK constraints)
- Prevents silent version conflicts and lost updates
- Supports automatic retry logic in command executor

**Trade-offs Accepted:**

- **Backend Requirements**: Backends must provide atomic multi-stream append operations
- **Version Conflict Handling**: Consumers must handle version conflict errors (but automatic retry mitigates this)
- **No Streaming Reads**: Read operation loads all events into memory (acceptable for event sourcing where streams are bounded)

These trade-offs are acceptable because:
- Atomicity is fundamental requirement per ADR-001
- Version conflicts are inherent to optimistic concurrency and handled by automatic retry
- Event sourced streams are typically bounded in size (snapshots handle long streams)
- API simplicity reduces cognitive load on library consumers

## Consequences

**Positive:**

- **API Simplicity**: Library consumers interact with just two core operations (read, append)
- **Clear Separation**: Backend implementation complexity hidden behind clean trait boundary
- **Backend Flexibility**: Each backend chooses appropriate atomicity mechanism for its technology
- **Testability**: In-memory backend can easily implement with simple locks
- **Audit Trail Support**: Metadata preservation built into trait contract
- **Focused Traits**: Separation of EventStore and EventSubscription keeps concerns isolated
- **Type Safety**: Rust trait system enforces backend capabilities at compile time

**Negative:**

- **Backend Complexity**: Implementing atomic multi-stream append is non-trivial for some storage technologies
- **Version Conflict Errors**: Consumers receive errors on conflicts (mitigated by automatic retry in executor)
- **Memory Loading**: Read operations load entire stream history into memory (mitigated by snapshotting patterns)
- **Backend Limitations**: Not all storage technologies can provide required atomicity guarantees

**Enabled Future Decisions:**

- Command executor can implement automatic retry by re-issuing append after re-reading streams
- Backends can optimize append operations internally (batching, connection pooling, etc.)
- Chaos testing can inject failures at append boundaries
- Monitoring can track append success rates, latency, version conflicts
- Read-through caching strategies can be implemented transparently

**Constrained Future Decisions:**

- All EventStore implementations must provide atomic multi-stream append
- Append operations must include version expectations for optimistic concurrency
- Subscription functionality must be in separate trait
- Reads must return complete stream histories (no streaming/pagination in trait)

## Alternatives Considered

### Alternative 1: Include Subscriptions in EventStore Trait

Combine read, append, and subscription operations in a single unified trait.

**Rejected Because:**
- Violates single responsibility principle - mixing short-lived operations with long-lived streams
- Forces backends to implement both even if they only support core operations
- Makes testing more complex (must mock multiple unrelated concerns)
- Different lifecycle semantics and error modes mixed together
- Some backends (file-based, simple in-memory) may support read/append but not subscriptions
- Harder to evolve traits independently

### Alternative 2: Expose Transaction/Session in API

Provide explicit transaction or session handles that consumers must manage:

**Rejected Because:**
- Event sourcing's atomicity boundary is naturally the single append operation
- Exposes implementation details (how backends achieve atomicity) in consumer API
- Adds API complexity and learning curve without corresponding benefit
- Consumers don't need transaction control - they just need atomic appends
- Different backends use different atomicity mechanisms (transactions, locks, etc.)
- Violates encapsulation - HOW atomicity is achieved should be hidden

### Alternative 3: Separate Traits per Stream Count

Provide different traits for single-stream vs multi-stream operations:

**Rejected Because:**
- Creates artificial distinction in API when implementation is the same
- Forces type system complexity to distinguish single vs multi-stream commands
- Multi-stream append can trivially handle single-stream case
- Adds cognitive load for library consumers
- Benefits are minimal (slight performance optimization for single-stream case)

### Alternative 4: Streaming Read Operations

Provide async stream/iterator interface for reading events instead of loading all into memory:

**Rejected Because:**
- Event sourced streams are typically bounded in size (snapshots handle long streams)
- Adds API complexity (async iterators, backpressure, error handling mid-stream)
- Most command logic needs full history to compute state
- Performance benefits minimal for typical stream sizes
- Can be added later if needed without breaking existing API
- Simplicity preferred for initial design

### Alternative 5: Version-Free Append with Last-Write-Wins

Allow appends without version checking, using timestamp-based ordering:

**Rejected Because:**
- Defeats optimistic concurrency control
- Cannot detect concurrent modifications and conflicts
- Lost updates become silent failures
- Violates fundamental event sourcing correctness guarantees
- Makes automatic retry impossible (no conflict detection)
- Not suitable for business-critical operations requiring consistency

## References

- ADR-001: Multi-Stream Atomicity Implementation Strategy
- REQUIREMENTS_ANALYSIS.md: FR-4 Event Store Abstraction
- REQUIREMENTS_ANALYSIS.md: FR-4.1 Pluggable Storage
- REQUIREMENTS_ANALYSIS.md: FR-4.2 Event Ordering
- REQUIREMENTS_ANALYSIS.md: FR-4.3 Event Metadata
- REQUIREMENTS_ANALYSIS.md: FR-6.1 Event Subscription
