# ADR-026: Subscription Table + Advisory Lock Coordination

## Status

accepted

## Context

ADR-023 proposed a ProjectorCoordinator trait with advisory locks for leader election and a separate heartbeat table for liveness detection. Implementation revealed fundamental complexity issues stemming from the mismatch between session-scoped advisory locks and connection pooling.

**The Implementation Failure:**

When implementing PostgresCoordinator per ADR-023, we encountered an architectural impedance mismatch:

1. PostgreSQL advisory locks are session-scoped—tied to a specific database connection
2. Connection pools want to recycle connections across requests
3. Guards needed to hold connections from the pool to maintain the advisory lock
4. Heartbeat updates needed the same connection to prove the session is alive
5. Connection pool contention created intermittent test failures
6. Debugging connection lifecycle issues became intractable

The core problem: We tried to use ephemeral pooled connections for session-duration coordination primitives.

**Why Elixir/Commanded Doesn't Have This Problem:**

Elixir's EventStore and Commanded frameworks use a fundamentally different architecture:

- Each subscription is a GenServer process with its own dedicated database connection
- Advisory locks are acquired on that dedicated connection
- When the process crashes, the connection closes automatically
- Lock release is automatic—no heartbeat table needed
- Connection lifecycle = lock lifecycle = process lifecycle

The insight: Session-scoped locks work naturally when you own the session (dedicated connection), not when you borrow from a pool.

**The Correct Pattern:**

Looking at EventStore's actual implementation, the pattern is simpler than ADR-023 proposed:

1. **Subscriptions table** tracks checkpoint state: `(subscription_name, last_position)`
2. **Dedicated connection per projector** (not pooled)—owned for the projector's lifetime
3. **Advisory lock on dedicated connection**: `SELECT pg_advisory_lock(hash(subscription_name))`
4. **Session-scoped lock** held as long as connection remains open
5. **Automatic release** when connection closes (crash, shutdown, network partition)
6. **No heartbeat table** needed—connection alive implies lock held
7. **No validity checking** needed—connection lifecycle handles it

**Separation of Concerns:**

The subscriptions table and advisory locks serve different purposes:

- **Subscriptions table**: Checkpoint tracking (WHERE did we process up to?)
- **Advisory lock**: Coordination (WHO is processing right now?)

ADR-023's mistake was overengineering the coordination side with heartbeat tables and validity checks, when PostgreSQL's session lifecycle already provides exactly what we need.

**Key Forces:**

1. **Connection ownership**: Advisory locks require dedicated connections, not pooled
2. **Simplicity**: Fewer moving parts means fewer failure modes
3. **Proven pattern**: EventStore and Commanded have battle-tested this in production
4. **Rust context**: Connection pools are common in Rust web apps, but projectors are long-lived background workers
5. **Resource bounds**: One dedicated connection per projector is acceptable—projector count is low and bounded

## Decision

Use the EventStore/Commanded pattern: subscription table for checkpoints + session-scoped advisory locks on dedicated connections for coordination.

**Implementation Constraints:**

1. **Subscriptions Table**:
   - Schema: `(subscription_name TEXT PRIMARY KEY, last_position BIGINT, updated_at TIMESTAMPTZ)`
   - Purpose: Track WHERE each subscription has processed up to
   - Updated transactionally with projection writes

2. **Dedicated Connection**:
   - Each projector owns a dedicated database connection (not from pool)
   - Connection lifetime = projector lifetime
   - Connection close = automatic lock release

3. **Advisory Lock**:
   - Acquired via `SELECT pg_advisory_lock(hash(subscription_name))`
   - Lock ID derived from subscription name for uniqueness
   - Held for session duration (connection open)
   - Released automatically when connection closes

4. **No Heartbeat Table**:
   - Connection alive = lock held
   - No separate heartbeat tracking needed
   - Stuck projectors detected via application monitoring

5. **No Validity Checking**:
   - If connection dies, lock releases automatically
   - No is_valid() checks in projection loop
   - Simpler error handling

**Supersedes ADR-023:**

This decision supersedes ADR-023's ProjectorCoordinator trait with heartbeat table approach. The trait abstraction added complexity without benefit—PostgreSQL advisory locks are the natural primitive, and other backends have equivalent mechanisms (MySQL GET_LOCK, Redis SETNX, etc.).

## Rationale

**Why Dedicated Connections Not Pooled:**

Session-scoped locks require session ownership. Trying to use pooled connections creates lifecycle mismatches:

- Pool wants to recycle connections → lock released unexpectedly
- Guards hold connections indefinitely → pool starvation
- Heartbeats need same connection → coupling between guard and heartbeat logic

Dedicated connections align lifecycles: connection lifetime = projector lifetime = lock lifetime.

**Why No Heartbeat Table:**

PostgreSQL's session lifecycle already provides liveness detection:

- If connection is alive, lock is held
- If connection dies (crash, network partition), lock releases
- Heartbeat table adds complexity without providing additional guarantees

Stuck projectors (infinite loops, deadlocks) should be detected by application monitoring (metrics, health checks), not infrastructure coordination primitives.

**Why This Is Simpler Than ADR-023:**

| ADR-023 | ADR-026 |
|---------|---------|
| ProjectorCoordinator trait | Direct PostgreSQL advisory lock calls |
| CoordinatorGuard with is_valid() | Lock held while connection open |
| Heartbeat table updates | No heartbeat needed |
| Pooled connections with guards | Dedicated connection per projector |
| Heartbeat timeout configuration | No timeout configuration |
| Contract tests for coordinator trait | Contract tests for lock behavior |

Fewer abstractions, fewer failure modes, proven in production by EventStore/Commanded.

**Why Connection Count Is Acceptable:**

Projectors are bounded and long-lived:

- Typical application: 5-20 projectors
- Each projector: 1 dedicated connection
- Total: 5-20 connections (well within PostgreSQL limits)

This is fundamentally different from connection-per-request web applications where pooling is essential.

**Why EventStore/Commanded Pattern Works:**

This pattern has been proven in production Elixir systems for years. The key insight is aligning three lifecycles:

1. Projector process lifetime
2. Database connection lifetime
3. Advisory lock lifetime

When these align, coordination becomes simple and reliable.

## Consequences

### Positive

- **Simplicity**: One table, one lock, automatic release
- **Reliability**: No heartbeat races, no validity checking edge cases
- **Battle-tested**: Proven pattern from EventStore/Commanded
- **Crash safety**: Connection close releases lock automatically
- **Easier debugging**: Fewer moving parts, clearer failure modes
- **Less configuration**: No heartbeat intervals or timeouts to tune

### Negative

- **Dedicated connections**: One connection per projector (bounded by projector count)
- **PostgreSQL-specific**: Advisory locks are not standardized SQL (but MySQL, Redis have equivalents)
- **No abstraction**: Lost ProjectorCoordinator trait (but added little value)
- **Connection management**: Application must manage dedicated connection lifecycle

### Enabled Future Decisions

- **Other backends**: MySQL GET_LOCK, Redis SETNX, etc. can follow same pattern
- **Monitoring**: Can inspect subscriptions table for stuck projectors via timestamp
- **Testing**: In-memory coordinator can use in-process mutex with same semantics

### Constrained Future Decisions

- **Pool integration**: Cannot use connection pooling for projector coordination
- **Stateless projectors**: Projectors must be long-lived processes, not ephemeral handlers

## Alternatives Considered

### Alternative 1: Keep ADR-023 Pattern, Fix Connection Pool Issues

**Description**: Continue with ProjectorCoordinator trait, solve connection pool problems by documenting connection requirements or adding connection pool configuration.

**Why Rejected**: Fixes symptoms, not root cause. Session-scoped locks fundamentally conflict with connection pooling. EventStore/Commanded prove there's a simpler way. Heartbeat table adds complexity without corresponding benefit.

### Alternative 2: Use Checkpoint Table Conditional Updates Only

**Description**: No advisory locks at all. Use conditional WHERE clause on checkpoint updates for coordination: `UPDATE subscriptions SET last_position = $1 WHERE name = $2 AND last_position < $1`.

**Why Rejected**: Wrong granularity for coordination. All instances would poll and process events, but only one successfully updates checkpoint. This wastes compute (N-1 instances process events pointlessly) and executes side effects N times (emails sent, HTTP calls made) before checkpoint update fails. Need leader election BEFORE processing, not idempotent checkpoint updates AFTER.

### Alternative 3: External Coordination (Redis, etcd, ZooKeeper)

**Description**: Use external coordination service rather than database primitives.

**Why Rejected**: Adds operational dependency. PostgreSQL advisory locks are sufficient and already available. External coordination makes sense for multi-datacenter deployments, but that's not EventCore's target use case.

### Alternative 4: No Coordination, Document Kubernetes Single-Replica Deployment

**Description**: Don't provide coordination primitives. Document that projectors must run as single-replica deployments.

**Why Rejected**: Pushes correctness concern to deployment. Easy to accidentally scale projectors and corrupt data. Library should prevent correctness bugs, not just document how to avoid them.

### Alternative 5: Heartbeat-Based Distributed Locking (Lease Pattern)

**Description**: Use database table with timestamps for lease-based locking: instance writes timestamp, claims lock if no recent timestamp exists, periodically updates timestamp.

**Why Rejected**: Requires clock synchronization. Has race conditions during failover (two instances may briefly both think they have lease). More complex than advisory locks while providing weaker guarantees. Advisory locks are atomic, lease timestamps are not.
