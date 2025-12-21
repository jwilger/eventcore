# ADR-023: Projector Coordination for Distributed Deployments

## Status

accepted

## Context

ADR-021 established poll-based projectors where each Projector polls for events, manages its own checkpoint within transactions, and has a unique name for identification. This design works correctly for single-process deployments.

Production systems typically run multiple application instances for availability and load distribution. This creates a fundamental coordination challenge: in a distributed deployment with N application instances, each running the same projector code, how do we ensure correct operation?

**Why Coordination is Required for Correctness:**

Coordination is not an optional optimization. Without it, distributed projectors face correctness failures:

1. **Checkpoint Races**: Two instances updating the same checkpoint create race conditions. Instance A reads checkpoint at position 100, Instance B reads same checkpoint at 100. Both process events 101-110 and write checkpoint at 110. Depending on timing, one write may be lost, causing events 101-110 to be reprocessed on restart. Worse, if checkpoints are written mid-batch, events can be skipped entirely.

2. **Read Model Corruption**: Even with idempotent event handling, concurrent projection writes can corrupt data. Two instances applying the same event simultaneously may interleave their database operations, violating invariants the projection is designed to maintain.

3. **Ordering Violations**: ADR-021 chose sequential processing specifically to guarantee ordering. Parallel processing by multiple instances undermines this fundamental guarantee.

**The Problem Shape: Leader Election, Not Partition Assignment**

This problem is simpler than Kafka-style consumer group coordination because ADR-021's sequential processing choice eliminates partition assignment. The question is not "which instance processes which subset of events" but rather "which single instance processes events for this projector."

This is pure leader election: at most one instance holds leadership for a given projector at any time. When the leader fails, another instance acquires leadership and resumes from the checkpoint.

**Key Forces:**

1. **Backend Independence**: EventCore supports multiple storage backends (PostgreSQL, in-memory, potentially others). The coordination API must not mandate specific infrastructure like ZooKeeper, etcd, or Redis.

2. **Use Available Primitives**: Each backend has natural coordination primitives. PostgreSQL has advisory locks. In-memory stores can use mutexes. Mandating external infrastructure when the database already provides sufficient primitives adds unnecessary operational complexity.

3. **Crash Safety**: If a leader crashes, its leadership must be released automatically. Stale leadership locks would prevent failover, defeating the purpose of running multiple instances.

4. **Library Completeness**: A projection system that only works correctly in single-process mode is incomplete for production use. Coordination is part of the projection system, not an application-level afterthought.

5. **Explicit Single-Process Opt-In**: Single-process deployments should not require external coordination infrastructure, but the choice to run without distributed coordination should be explicit, not a dangerous default.

## Decision

EventCore defines a ProjectorCoordinator trait as a fundamental part of the projection system. Event store backends implement this trait using their available primitives, following the same pattern established for EventStore itself.

**Architectural Constraints:**

1. **Trait-Based Abstraction**: ProjectorCoordinator is a trait, not a concrete implementation. This mirrors how EventStore works: the library defines the API contract, backends provide implementations using their natural primitives.

2. **Required Parameter**: The projection runner requires a coordinator. This makes coordination failures compile-time errors rather than production incidents. Single-process deployments use an explicit LocalCoordinator rather than having "no coordination" as an invisible default.

3. **RAII Guard Pattern**: Leadership acquisition returns a guard that releases leadership when dropped. This ensures crash safety: if the process terminates unexpectedly, leadership is released automatically (via database connection close, process termination, etc.).

4. **Leadership Validity Checking**: Guards must be checkable for continued validity. Leadership can be lost unexpectedly due to network partitions or database failover. The projection loop should exit gracefully when leadership is lost rather than corrupting state.

5. **Backend-Specific Implementations**:
   - PostgreSQL uses session-scoped advisory locks (released automatically on disconnect)
   - In-memory uses process-local mutexes (for testing and single-process deployments)
   - Future backends use their natural primitives

6. **LocalCoordinator for Single-Process**: A coordinator implementation that always grants leadership immediately. Usage is explicit, making "I am running single-process and accept the risks" a deliberate choice.

7. **Contract Tests**: Per ADR-013's pattern, coordinators have contract tests verifying mutual exclusion, crash-safe release, and blocking/non-blocking acquisition semantics.

**What This Decision Does NOT Include:**

- No partition assignment or work distribution within a projector
- No automatic failover orchestration (applications handle restarting projectors)
- No monitoring or metrics (those are application concerns)
- No leader handoff protocol (guard drop is sufficient)

## Rationale

**Why a Trait Rather Than Optional Helper:**

Making coordination optional treats a correctness concern as a deployment concern. If uncoordinated projectors can corrupt data, then running them accidentally in production is a bug. Requiring a coordinator parameter transforms "forgot to add coordination" from a production incident into a compile error.

**Why Backend-Specific Implementations:**

Different backends have different primitives available:

| Backend | Natural Primitive | Crash Behavior |
|---------|------------------|----------------|
| PostgreSQL | Advisory locks | Released on disconnect |
| In-Memory | Tokio mutex | Released on drop |
| MySQL | GET_LOCK() | Released on disconnect |
| SQLite | File locks | Released on close |

Mandating external coordination (ZooKeeper, etcd) when the database already provides sufficient primitives adds operational complexity disproportionate to the problem.

**Why RAII Guard Pattern:**

Rust's ownership system makes RAII natural. The guard pattern ensures leadership is released even if projection code panics or returns early. This matches Rust idioms and prevents leadership leaks that would block failover.

**Why Leadership Validity Checking:**

Database connections can fail. Network partitions can occur. The projection loop needs to detect when leadership is lost and exit gracefully rather than continuing to process events without coordination. Checking guard validity enables clean shutdown and proper failover.

**Why Not Partition Assignment:**

ADR-021 explicitly chose sequential processing to guarantee event ordering. Partition assignment would allow parallel processing, which contradicts that fundamental design choice. If horizontal scaling is needed, deploy multiple projector types (each with its own leader), not multiple workers for the same projector.

For users who need Kafka-style partition-based scaling, an escape hatch exists: create a single-threaded projector that pushes events onto a Kafka (or similar) log, then let that external system handle partitioning and parallel consumption. EventCore handles the leader-elected, sequential projection to the external system; that external system provides the partition assignment semantics.

**Why Explicit LocalCoordinator:**

A no-op coordinator makes sense for single-process deployments and testing. But it should be explicit: `LocalCoordinator` clearly communicates "I know this only works with one instance." This is safer than having `None` as a coordinator or skipping coordination entirely with a boolean flag.

## Consequences

### Positive

- Correctness by construction: cannot run uncoordinated projectors accidentally
- Backend independence: no mandated infrastructure beyond the event store
- Crash safety: leadership released automatically on process failure
- Testability: mock coordinators for unit tests, real coordinators for integration tests
- Consistent pattern: follows the same trait-based approach as EventStore

### Negative

- Required parameter: even single-process deployments must provide a coordinator (LocalCoordinator)
- Backend implementation burden: each backend must implement ProjectorCoordinator
- New abstraction: another trait to understand and implement

### Enabled Future Decisions

- Additional backends can implement the trait with their primitives
- Coordination monitoring could be added to guard implementations
- Leadership handoff protocols could extend the guard if needed

### Constrained Future Decisions

- Poll-based assumption: coordinator design assumes poll-based projectors from ADR-021
- No partition assignment: would require different trait design to add later

## Alternatives Considered

### Alternative 1: Optional Coordination with PostgreSQL-Specific Helper

**Description**: Make coordination optional, provide a PostgreSQL advisory lock helper, document Kubernetes deployment patterns for other cases.

**Why Rejected**: Treats correctness concern as deployment concern. Easy to accidentally run uncoordinated projectors in production. Doesn't follow the trait-based pattern established by EventStore. Users must remember to add coordination rather than having the compiler enforce it.

### Alternative 2: Coordination Built Into EventStore Trait

**Description**: Add coordination methods directly to the EventStore trait.

**Why Rejected**: Conflates two separate concerns (storage vs coordination). Some deployments may want different coordination than their storage provides. Makes EventStore trait larger and harder to implement.

### Alternative 3: External Coordination Only (ZooKeeper/etcd/Consul)

**Description**: Require external coordination infrastructure, don't use database primitives.

**Why Rejected**: Adds operational complexity disproportionate to the problem. PostgreSQL advisory locks are sufficient and already available. Forces infrastructure that many deployments don't need.

### Alternative 4: Checkpoint-Based Coordination

**Description**: Use optimistic locking on checkpoint updates for coordination.

**Why Rejected**: Wrong granularity. Checkpoint updates are per-event; leadership is per-session. Creates race conditions between checkpoint read and update. Doesn't handle the "who starts processing" problem, only "who finishes."

### Alternative 5: Documentation-Only Approach

**Description**: Document coordination patterns but provide no library support.

**Why Rejected**: Every user reinvents the same coordination logic. Advisory lock implementation is simple enough to include. Library-provided contract tests ensure correctness across implementations.
