# ADR-007: Optimistic Concurrency Control Strategy

## Status

accepted

## Context

EventCore enables concurrent command execution against shared event streams without blocking. In multi-user systems, multiple commands may simultaneously read from and attempt to write to the same streams, potentially causing lost updates or inconsistent state if not properly coordinated. Traditional locking approaches would serialize access and eliminate the performance benefits of event sourcing.

**Key Forces:**

1. **Concurrent Execution**: Multiple commands executing simultaneously must not corrupt each other's changes
2. **Lost Update Prevention**: If Command A reads stream at version 5, and Command B commits version 6 before A completes, Command A must detect this conflict
3. **Multi-Stream Atomicity**: ADR-001 requires atomic writes across multiple streams - version checking must span all affected streams
4. **Performance Under Contention**: Pessimistic locking would serialize multi-stream commands, destroying throughput under concurrent load
5. **User Experience**: Developers need transparent conflict handling without manual retry logic
6. **Correctness Over Speed**: ADR-001 prioritizes correctness - version conflicts must never be missed
7. **Storage Backend Integration**: Version checking must work within ADR-002's EventStore trait abstraction
8. **Error Classification**: ADR-004 requires distinguishing retriable (version conflicts) from permanent (validation) failures

**Why This Decision Now:**

Optimistic concurrency control (OCC) is fundamental to EventCore's concurrent execution model. This decision affects the EventStore trait contract, event metadata structure, command executor retry logic, and developer experience under concurrent load. It must be defined before implementing the command executor to ensure consistent concurrency semantics throughout.

## Decision

EventCore will implement optimistic concurrency control using stream version numbers with the following strategy:

**1. Version-Based Conflict Detection**

Every stream maintains a monotonically increasing version number:

- Version starts at 0 (empty stream)
- Each event appended increments version by 1 (version 1, 2, 3, ...)
- Stream version stored in event metadata per ADR-005
- Version represents the position of the last event in the stream

**2. Read-Capture-Write-Verify Pattern**

Commands follow a three-phase execution:

**Phase 1 - Read with Version Capture:**

- Command reads events from declared streams
- For each stream, capture current version number
- Version represents state upon which command logic will operate
- Empty streams have version 0

**Phase 2 - Compute:**

- Reconstruct state by applying events (CommandLogic::apply)
- Execute business logic and generate events (CommandLogic::handle)
- Events prepared but not yet committed
- No locks held during this phase

**Phase 3 - Write with Version Verification:**

- Attempt atomic append with expected version numbers
- Backend verifies EVERY stream version matches expected version
- If any version mismatch: ConcurrencyError (retriable)
- If all versions match: commit all events atomically, increment versions
- Version verification happens within storage transaction per ADR-001

**3. Multi-Stream Version Checking**

For commands writing to multiple streams:

- Expected version captured for EACH stream during read phase
- Write operation includes expected versions for ALL streams
- Backend verifies ALL version expectations atomically
- Single version mismatch fails entire operation (no partial writes per ADR-001)
- Conflict detection across stream boundaries prevents subtle inconsistencies

**4. ConcurrencyError Classification**

Version conflicts produce ConcurrencyError per ADR-004:

- Marked as Retriable (automatic retry appropriate)
- Includes diagnostic context: which streams conflicted, expected vs actual versions
- Enables command executor to implement automatic retry with backoff
- Never silently ignored - always explicit error or success

**5. Atomic Version Checking**

Version verification happens within storage backend transaction:

- PostgreSQL: CHECK constraints and SELECT FOR UPDATE in transaction
- In-memory: Mutex-protected version comparison and increment
- Version check and event append form single atomic operation
- Prevents time-of-check-to-time-of-use race conditions
- Atomicity mechanism per ADR-002 (backend's responsibility)

**6. Monotonic Version Guarantees**

Stream versions provide ordering invariants:

- Versions always increase (never decrease or skip)
- Version N implies events 1 through N exist
- Empty stream is version 0
- First event creates version 1
- Version numbers never reused or reset
- Enables deterministic event ordering within streams

## Rationale

**Why Optimistic Over Pessimistic Locking:**

Pessimistic locking (acquiring locks before reading) would:

- Serialize all access to popular streams (e.g., account balances)
- Multi-stream commands would hold multiple locks simultaneously (deadlock risk)
- Long-running computations block other commands
- Contradicts event sourcing's read-scale advantages
- Poor user experience (waiting for locks instead of fast failure and retry)

Optimistic concurrency:

- Allows concurrent reads without blocking
- Only serializes at commit time (minimal contention window)
- Failed attempts retry with fresh state
- Better throughput when conflicts are rare (common in practice)
- Aligns with event sourcing patterns (append-only, immutable)

**Why Version Numbers Over Timestamps:**

Timestamps are unsuitable for concurrency control:

- Clock skew in distributed systems causes ambiguity
- Microsecond precision insufficient under high concurrency
- No deterministic ordering guarantee
- Cannot represent "next expected version" concept
- Comparison semantics unclear (equal timestamps?)

Version numbers provide:

- Deterministic, unambiguous ordering
- Explicit "expected next version" semantics
- No clock synchronization required
- Simple integer comparison (cheap and reliable)
- Standard practice in event sourcing systems

**Why Multi-Stream Version Checking:**

Single-stream version checking with multi-stream writes would allow:

- Command A reads streams X and Y at versions 5 and 10
- Command B writes to stream Y, advancing to version 11
- Command A writes to both, checking only X (version 5 matches)
- Command A's write to Y based on stale state (version 10) but not detected

Multi-stream version checking ensures:

- All streams validated against captured state
- Impossible to commit based on partially stale state
- Maintains ADR-001's atomicity guarantees
- Prevents subtle multi-stream inconsistencies

**Why Atomic Version Verification:**

Separating version check from event write creates race condition:

1. Command checks versions (all match)
2. Another command commits between check and write
3. Command writes based on stale state

Atomic verification within transaction:

- Version check and event append in single atomic operation
- No race window between check and commit
- Leverages database ACID guarantees per ADR-001
- Storage backend manages atomicity per ADR-002

**Why ConcurrencyError as Retriable:**

Version conflicts are transient failures:

- Conflict indicates another command committed first
- Re-reading streams and re-executing produces valid result
- Business logic unchanged (inputs still valid)
- Automatic retry transparent to developers

Marking as Retriable per ADR-004:

- Enables automatic retry logic in command executor
- Distinguishes from permanent failures (validation, business rules)
- Provides clear semantics for error handling
- Expected behavior under concurrent load

**Trade-offs Accepted:**

- **Retry Overhead**: Conflicting commands retry, increasing latency and resource usage
- **Hot Stream Contention**: Popular streams (e.g., global counters) may have high conflict rates
- **Wasted Computation**: Failed command computation discarded on conflict
- **Eventual Success Not Guaranteed**: Under extreme contention, commands may exceed retry limit

These trade-offs are acceptable because:

- Conflicts rare in well-designed systems (proper stream partitioning)
- Retry latency acceptable for business operations (vs. blocking on locks)
- Alternative (pessimistic locking) has worse throughput characteristics
- Automatic retry with backoff handles typical conflict rates
- Applications can implement custom retry strategies if needed
- Correctness never compromised (conflicts always detected)

## Consequences

**Positive:**

- **No Blocking**: Commands never wait for locks during read or compute phases
- **Conflict Detection**: Version mismatches always detected - no silent lost updates
- **Multi-Stream Safety**: Atomic version checking across all streams prevents partial staleness
- **Transparent Retry**: Developers don't implement retry logic - executor handles automatically
- **Performance Under Low Contention**: Excellent throughput when conflicts are rare
- **Simple Reasoning**: Version numbers provide clear, deterministic semantics
- **Storage Backend Flexibility**: Each backend implements atomicity using appropriate mechanism
- **Type-Safe Versioning**: Version type from ADR-003 prevents primitive obsession
- **Observability**: Version conflicts visible in metrics and logs for monitoring contention

**Negative:**

- **Retry Latency**: Commands under contention experience retry delays
- **Wasted Work**: Failed commands discard computation on conflict
- **Hot Stream Problem**: Single popular stream can become contention bottleneck
- **Unbounded Retries**: Extreme contention can exhaust retry limits
- **Developer Awareness**: Developers must design streams to minimize conflicts
- **Testing Complexity**: Reproducing high-contention scenarios requires careful test setup

**Enabled Future Decisions:**

- Command executor can implement automatic retry with exponential backoff (ADR-008)
- Retry policies can be configurable (max attempts, backoff strategy, timeouts)
- Monitoring dashboards can track version conflict rates per stream
- Stream partitioning strategies can reduce hot stream contention
- Chaos testing can inject conflicts to verify retry behavior
- Performance optimizations can identify high-contention streams
- Custom retry strategies can be provided for specific command types
- Circuit breakers can detect sustained high conflict rates

**Constrained Future Decisions:**

- EventStore trait must accept expected versions in append operations
- Event metadata must include stream version per ADR-005
- Version type must support monotonic increment operations
- Storage backends must implement atomic version verification
- Command executor must retry on ConcurrencyError
- Version numbers always start at 0 and increment by 1
- Multi-stream appends must verify ALL stream versions atomically
- Version conflicts must always produce ConcurrencyError (never succeed silently)

## Alternatives Considered

### Alternative 1: Pessimistic Locking with Lock Acquisition

Acquire locks on all streams before reading, hold until commit.

**Rejected Because:**

- Serializes all access to locked streams (poor concurrency)
- Multi-stream commands hold multiple locks (deadlock risk without careful ordering)
- Long computations block other commands entirely
- Lock management complexity (timeouts, deadlock detection)
- Contradicts event sourcing's read-scale benefits
- Poor user experience (waiting for locks vs. fast conflict detection)
- Requires backend support for distributed locking
- Violates EventCore's performance goals under concurrency

### Alternative 2: Last-Write-Wins (No Version Checking)

Accept all writes; use timestamps to determine ordering.

**Rejected Because:**

- Lost updates are silent and undetectable
- No conflict detection - incorrect results under concurrent load
- Timestamp-based ordering unreliable (clock skew)
- Violates event sourcing correctness guarantees
- Cannot distinguish concurrent from sequential operations
- Debugging impossible (which event "should" have won?)
- Unsuitable for business-critical operations requiring consistency
- Contradicts ADR-001's correctness-first approach

### Alternative 3: Version Checking Only on Single "Primary" Stream

For multi-stream commands, check version only on one designated stream.

**Rejected Because:**

- Allows committing based on stale state in non-primary streams
- Inconsistencies possible across streams
- Violates ADR-001's multi-stream atomicity guarantees
- Requires developers to reason about "primary" stream designation
- Subtle bugs when business logic depends on state from non-primary streams
- No correctness advantage over full multi-stream checking
- Version checking cost negligible compared to incorrect results

### Alternative 4: Global Sequence Number Across All Streams

Use single global sequence number instead of per-stream versions.

**Rejected Because:**

- Requires centralized coordination for sequence generation
- Single point of contention for all commands (serialization bottleneck)
- No benefit for conflict detection (still need per-stream versions)
- Complicates version tracking and reasoning
- Doesn't align with event sourcing's per-stream independence
- Performance worse than per-stream versions
- Unnecessary coupling between unrelated streams

### Alternative 5: Optimistic Locking with Separate Version Table

Store versions in separate table from events.

**Rejected Because:**

- Complicates transaction management (must update two locations atomically)
- Increases write latency (additional table update)
- Version-event consistency requires careful transaction coordination
- Storage overhead for separate version tracking
- Per-ADR-005, version is part of event metadata (belongs with event)
- No advantage over storing version in event metadata
- Violates event metadata immutability (version changes separately from events)

### Alternative 6: Compare-and-Swap (CAS) at Event Level

Use CAS operation for individual event writes instead of version checking.

**Rejected Because:**

- Doesn't support multi-stream atomicity (separate CAS per stream)
- Cannot express "write events A and B atomically" with CAS semantics
- No backend API for multi-stream CAS
- Requires multiple round-trips for multi-stream commands
- Violates ADR-001's single atomic operation requirement
- More complex than version-based approach
- CAS not universally available across storage backends

### Alternative 7: No Automatic Retry - Expose Conflicts to Developers

Report version conflicts, require developers to implement retry logic.

**Rejected Because:**

- Violates NFR-2.1 (minimal boilerplate) - retry logic per command
- Inconsistent retry behavior across applications
- Developers likely to implement incorrect retry strategies
- Lost opportunity to provide infrastructure value
- Conflicts are expected under concurrency - library should handle gracefully
- Increases cognitive load and error potential
- Standard event sourcing pattern is automatic retry on version conflict

### Alternative 8: Eventual Consistency with Conflict Resolution

Allow conflicting writes, resolve conflicts after the fact with merge logic.

**Rejected Because:**

- EventCore's value proposition is immediate consistency (ADR-001)
- Conflict resolution logic is domain-specific and complex
- Observers see inconsistent intermediate states
- Business operations require strong consistency (e.g., money transfers)
- Defeats purpose of multi-stream atomicity
- Adds significant complexity to domain modeling
- Not suitable for business-critical operations

## References

- ADR-001: Multi-Stream Atomicity Implementation Strategy (atomic multi-stream writes)
- ADR-002: Event Store Trait Design (append operations with version checking)
- ADR-003: Type System Patterns for Domain Safety (Version type)
- ADR-004: Error Handling Hierarchy (ConcurrencyError as Retriable)
- ADR-005: Event Metadata Structure (stream version in metadata)
- REQUIREMENTS_ANALYSIS.md: FR-3.1 Version Tracking
- REQUIREMENTS_ANALYSIS.md: FR-3.2 Conflict Detection
- REQUIREMENTS_ANALYSIS.md: FR-3.3 Automatic Retry
- REQUIREMENTS_ANALYSIS.md: NFR-1.3 Performance Optimization Strategy (correctness first)
