# ADR-001: Multi-Stream Atomicity Implementation Strategy

## Status

accepted

## Context

EventCore's core value proposition is enabling atomic operations across multiple event streams, unlike traditional event sourcing frameworks that force rigid aggregate boundaries. This capability eliminates the need for complex saga patterns or eventual consistency when business operations span multiple entities.

**Key Forces:**

1. **Business Requirements**: Real-world operations often span multiple entities (e.g., money transfer between accounts requires atomic debit/credit)
2. **Consistency Guarantees**: Partial updates are unacceptable for many business operations
3. **Complexity Trade-offs**: Sagas and process managers add significant complexity when immediate consistency is needed
4. **Storage Capabilities**: Different storage backends have varying atomicity mechanisms
5. **Performance Impact**: Multi-stream operations must be efficient enough for production use

**Why This Decision Now:**

Multi-stream atomicity is the foundational architectural decision that differentiates EventCore from existing event sourcing libraries. This decision shapes the entire library design including the event store trait, command execution model, and versioning strategy.

## Decision

EventCore will implement multi-stream atomicity by leveraging the underlying storage backend's native transaction mechanisms. Commands will execute within a single transaction that spans all affected streams, with the following guarantees:

1. **All-or-Nothing Writes**: All events across all streams are written atomically, or none are written
2. **Storage-Native Transactions**: Use each storage backend's transaction primitives (e.g., PostgreSQL ACID transactions, in-memory locks)
3. **Consistent Version Checking**: Version conflicts are detected before any writes occur
4. **Transaction Isolation**: Appropriate isolation levels to prevent dirty reads and phantom events

## Rationale

**Why Storage-Native Transactions:**

The storage backend already provides battle-tested atomicity mechanisms. Reimplementing distributed transactions at the library level would be complex, error-prone, and unnecessary when PostgreSQL already provides ACID guarantees.

**Why Single-Transaction Model:**

Alternative approaches like two-phase commit or saga patterns introduce significant complexity and potential for partial failures. By requiring storage backends to support atomic multi-stream writes, we push complexity down to the proven storage layer where it belongs.

**Why Version Checking Within Transaction:**

Checking versions and writing events within the same transaction prevents time-of-check-to-time-of-use race conditions. This ensures optimistic concurrency control remains reliable under concurrent load.

**Trade-offs Accepted:**

- **Storage Dependency**: Not all storage backends can provide multi-stream atomicity (e.g., some NoSQL databases)
- **Performance**: Multi-stream transactions may have higher latency than single-stream operations due to lock acquisition
- **Contention**: Multiple streams mean more potential for lock contention under high concurrency

These trade-offs are acceptable because:

- PostgreSQL and similar RDBMS systems are widely available and production-ready
- Correctness is more important than raw throughput for most business operations
- Performance is still adequate (current benchmarks: 25-50 ops/sec multi-stream)

## Consequences

**Positive:**

- **Simplicity**: Library code is straightforward - start transaction, validate versions, write events, commit
- **Correctness**: Atomic guarantees are rock-solid, backed by proven database technology
- **Debuggability**: Transaction logs in the database provide clear audit trail
- **Testability**: In-memory adapter can simulate atomicity with simple locking

**Negative:**

- **Storage Requirements**: EventStore trait implementations must support transactions
- **Performance Ceiling**: Multi-stream throughput limited by database transaction capacity
- **Lock Contention**: Poor stream design (e.g., hot spots) can cause contention
- **Backend Limitations**: Some storage backends may not be suitable for EventCore

**Enabled Future Decisions:**

- Event store trait must include transaction/session concept
- Command executor needs transaction lifecycle management
- Retry logic can safely re-execute entire command on conflict
- Chaos testing can inject transaction failures

**Constrained Future Decisions:**

- Cannot support storage backends without transaction support
- Performance optimizations must work within transaction boundaries
- Stream design patterns must consider lock contention

## Alternatives Considered

### Alternative 1: Saga Pattern with Compensation

Implement multi-stream operations as a series of single-stream operations with compensation logic for rollback.

**Rejected Because:**

- Introduces eventual consistency where immediate consistency is required
- Compensation logic is complex and error-prone
- Defeats the core value proposition of EventCore
- External observers can see intermediate states

### Alternative 2: Two-Phase Commit (2PC)

Implement distributed 2PC across multiple storage instances.

**Rejected Because:**

- Significantly more complex than single-transaction approach
- 2PC coordination adds failure modes (coordinator failure, participant timeout)
- Unnecessary when single storage instance can handle all streams
- Performance overhead of coordination protocol
- Not needed for typical EventCore use cases

### Alternative 3: Append-Only Log with External Coordination

Write all events to a single append-only log, then distribute to stream-specific stores.

**Rejected Because:**

- Adds complexity of log coordination and distribution
- Introduces eventual consistency to per-stream views
- Requires additional infrastructure for log management
- Overkill for the typical scale of EventCore applications

### Alternative 4: Application-Level Locking

Implement custom locking mechanism in EventCore library code.

**Rejected Because:**

- Reinvents the wheel - databases already do this well
- Difficult to implement correctly across distributed systems
- Does not provide durability guarantees on process crash
- Complicates the library significantly

## References

- REQUIREMENTS_ANALYSIS.md: FR-1.4 Atomic Multi-Stream Writes
- REQUIREMENTS_ANALYSIS.md: NFR-1.3 Performance Optimization Strategy
- PostgreSQL ACID guarantees: https://www.postgresql.org/docs/current/tutorial-transactions.html
