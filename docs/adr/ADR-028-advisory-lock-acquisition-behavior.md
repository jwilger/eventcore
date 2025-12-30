# ADR-028: Advisory Lock Acquisition Behavior

## Status

accepted

## Context

ADR-026 established that EventCore uses PostgreSQL session-scoped advisory locks on dedicated connections for projector coordination. The decision specified that locks are acquired via `pg_advisory_lock(hash(subscription_name))` and released automatically when the connection closes.

Issue #239's acceptance criteria states: "Second instance blocks until leadership available." However, implementation requires choosing between two PostgreSQL primitives with fundamentally different behaviors:

- `pg_advisory_lock(key)` - Blocks indefinitely until lock acquired
- `pg_try_advisory_lock(key)` - Returns immediately with boolean (true = acquired, false = not acquired)

This decision has significant implications for resource usage, failover timing, and how EventCore integrates with deployment orchestration.

**Forces at Play:**

1. **Infrastructure Neutrality**: EventCore's architectural principle states the library "never assumes a particular business domain" and "owns infrastructure concerns." This extends to not assuming a particular deployment orchestrator (Kubernetes, systemd, bare metal).

2. **Resource Efficiency**: Blocked processes consume memory, file descriptors, and a dedicated database connection while doing no useful work. In cloud environments, this translates to cost.

3. **Orchestrator Integration**: Modern deployment platforms (Kubernetes, systemd, Docker Swarm) have sophisticated restart policies with exponential backoff. A process that exits with an error naturally integrates with these systems.

4. **Failover Speed**: Blocking processes provide instant failover when the leader dies (they're already waiting). Try-acquire processes depend on orchestrator restart timing.

5. **Visibility**: A blocked process appears "healthy" to basic health checks (process running, connection open) but isn't doing useful work. This can mask deployment issues.

6. **Commanded/EventStore Precedent**: Elixir's EventStore uses `pg_try_advisory_lock` (non-blocking) and handles failure by having the subscription process retry or report the failure.

**The Core Question:**

Should EventCore's lock acquisition:
- Block indefinitely (fast failover, resource-intensive standby)
- Fail immediately and let the caller/orchestrator handle retry (resource-efficient, potentially slower failover)
- Provide retry logic within the library (middle ground, duplicates orchestrator concerns)

## Decision

Use `pg_try_advisory_lock()` (non-blocking) and return an error when the lock is already held. The library does NOT implement retry logic; callers decide how to handle lock acquisition failure.

**API Shape:**

```rust
/// Attempts to acquire leadership for a projector.
/// Returns Ok(guard) if acquired, Err(LockNotAcquired) if another instance holds the lock.
/// Does NOT block or retry.
pub async fn acquire_leadership(
    conn: &mut PgConnection,
    subscription_name: &str,
) -> Result<LeadershipGuard, CoordinationError>;

pub enum CoordinationError {
    /// Another instance holds the lock. Not an infrastructure failure.
    LockNotAcquired { subscription_name: String },
    /// Database connectivity or other infrastructure error.
    DatabaseError(sqlx::Error),
}
```

**Caller Responsibilities:**

The projection runner (or application code) decides what to do on `LockNotAcquired`:
- Exit the process (recommended for Kubernetes/systemd deployments)
- Sleep and retry (for environments without restart orchestration)
- Log and continue without this projector (for degraded-mode operation)

## Rationale

**Why Try-Acquire, Not Blocking:**

1. **Infrastructure Neutrality**: Blocking assumes the deployment wants standby processes. Try-acquire makes no assumptions - the caller decides. This aligns with EventCore's principle of owning infrastructure concerns without assuming deployment patterns.

2. **Resource Efficiency**: A blocked projector consumes:
   - One dedicated PostgreSQL connection (from a limited pool, typically 100-200 max)
   - Process memory (Rust binary + runtime state)
   - A "slot" in the orchestrator's desired replica count

   For N replicas, N-1 are permanently blocked and wasting resources. With try-acquire + process exit, the orchestrator's backoff ensures only occasional probe attempts.

3. **Orchestrator Alignment**: Kubernetes, systemd, Docker Swarm, and similar platforms have sophisticated restart policies:
   - Exponential backoff (prevents thundering herd on leader failure)
   - Health checks that detect actual functionality, not just "process alive"
   - Resource limits that kill stuck processes

   Blocking processes circumvent these mechanisms. A process that exits with a clear error code integrates naturally.

4. **Visibility and Debugging**: A `LockNotAcquired` error in logs clearly indicates "another instance is the leader." A blocked process provides no signal - it simply sits there. Operators can't distinguish "waiting for leadership" from "stuck."

5. **Commanded/EventStore Precedent**: Elixir's EventStore, the primary inspiration for ADR-026, uses `pg_try_advisory_lock` (the non-blocking variant). The Elixir ecosystem handles retry at the supervision tree level (equivalent to orchestrator-level restart in Rust deployments).

**Why Not Library-Level Retry:**

1. **Duplicates Orchestration**: Every deployment platform already handles "process exited, restart with backoff." Adding this to the library means:
   - Configuring backoff in two places
   - Potential conflicts between library retry and orchestrator restart
   - Testing complexity for retry logic that won't be used in typical deployments

2. **Unclear Termination**: If the library retries, when should it give up? After 10 attempts? 100? Never? Any choice is arbitrary without knowing deployment context. Better to let the orchestrator's policy decide.

3. **EventCore's Role**: The library's job is to provide correct coordination primitives, not deployment automation. Kubernetes is better at restart policies than EventCore should try to be.

**Failover Timing Trade-off:**

Try-acquire has slower failover than blocking:
- Blocking: Instant (process already waiting)
- Try-acquire: Depends on orchestrator restart timing (typically 10-60 seconds with backoff)

This is acceptable because:
1. Projections are eventually consistent by design - seconds of additional latency during failover is rarely critical
2. For scenarios requiring sub-second failover, a dedicated hot-standby pattern with blocking can be implemented at the application level
3. The common case (no failures) is unaffected

**What About Non-Orchestrated Deployments?**

For bare-metal or single-node deployments without Kubernetes/systemd:
- Application code can wrap `acquire_leadership` in a retry loop
- This is simple (~10 lines) and explicit about the deployment's needs
- The library could provide a helper function (not automatic behavior)

## Consequences

### Positive

- **Resource Efficient**: No idle blocked processes consuming connections and memory
- **Orchestrator Native**: Integrates naturally with Kubernetes, systemd restart policies
- **Clear Signals**: `LockNotAcquired` error provides explicit feedback vs silent blocking
- **Simple Implementation**: `pg_try_advisory_lock` is simpler than managing blocked connection state
- **Matches Precedent**: Aligns with Elixir EventStore's proven approach
- **Caller Control**: Applications choose their failure handling strategy

### Negative

- **Slower Failover**: Leadership transfer depends on orchestrator restart timing (10-60s typical)
- **Caller Responsibility**: Applications must handle `LockNotAcquired` (though "exit process" is the common pattern)
- **Documentation Burden**: Must clearly explain the expected deployment pattern

### Enabled Future Decisions

- **Blocking Helper**: Could add `acquire_leadership_blocking()` for specific use cases
- **Retry Helper**: Could provide `acquire_leadership_with_retry(policy)` for non-orchestrated deployments
- **Health Check Integration**: `LockNotAcquired` status can feed into health check endpoints

### Constrained Future Decisions

- **Hot Standby Pattern**: Applications wanting instant failover must implement it themselves
- **Connection Pooling for Waiters**: Blocked connections can't be pooled; this isn't relevant with try-acquire

## Alternatives Considered

### Alternative 1: Blocking Wait (pg_advisory_lock)

**Description**: Use `pg_advisory_lock()` which blocks until the lock is acquired. Second instance sits waiting, provides instant failover when leader dies.

**Why Rejected**:
- Wastes resources (connection, memory, process slot) for potentially hours/days
- Process appears "healthy" but isn't doing work, confusing monitoring
- Circumvents orchestrator restart policies designed for exactly this scenario
- Multiple blocked instances can pile up if deployment scales
- Doesn't match Elixir EventStore precedent

### Alternative 2: Try-Acquire with Library Retry

**Description**: Use `pg_try_advisory_lock()`, but implement retry with configurable backoff in the library. Eventually give up and return error.

**Why Rejected**:
- Duplicates orchestrator functionality
- "Give up after N attempts" is arbitrary without deployment context
- Adds configuration complexity (retry count, backoff params)
- Testing burden for rarely-used code paths
- Violates single-responsibility: library does coordination, orchestrator does lifecycle

### Alternative 3: Configurable Behavior (Blocking vs Try)

**Description**: Let caller choose blocking or try-acquire behavior via parameter or configuration.

**Why Rejected**:
- Increases API surface for questionable benefit
- Blocking behavior has clear downsides in modern deployments
- Applications needing blocking can implement a retry loop themselves
- "Choice" often means "users don't know which to pick"

### Alternative 4: Background Retry Thread

**Description**: Return immediately, spawn background task that retries and calls callback on success.

**Why Rejected**:
- Callback-based API is awkward in Rust async
- Complicates lifecycle management (what if process wants to exit?)
- Implicit background work is hard to reason about
- Doesn't match EventCore's explicit, synchronous API patterns

## References

- [Elixir EventStore - Advisory Lock Namespacing](https://github.com/commanded/eventstore/issues/166)
- [Elixir EventStore - Subscription Lock TTL Discussion](https://github.com/commanded/eventstore/issues/213)
- [Kafka Consumer Group Protocol](https://developer.confluent.io/courses/architecture/consumer-group-protocol/)
- [Kafka Consumer Design](https://docs.confluent.io/kafka/design/consumer-design.html)
