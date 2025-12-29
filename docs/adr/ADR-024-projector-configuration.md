# ADR-024: Projector Configuration and Liveness Detection

## Status

superseded

**Superseded by:** ADR-027 (Projector Poll and Retry Configuration) and ADR-026 (Subscription Table + Advisory Lock Coordination)

> **Note:** This ADR is retained for historical context. ADR-027 captures the still-valid poll and event retry configuration. ADR-026 eliminated heartbeat configuration entirely by adopting session-scoped advisory locks on dedicated connections. See ARCHITECTURE.md for current authoritative guidance.

## Context

ADR-021 established poll-based projectors where each Projector polls for events, applies them transactionally, and manages checkpoints. ADR-023 established the ProjectorCoordinator trait for leader election in distributed deployments. Neither ADR explicitly addresses configuration parameters or liveness detection for hung projectors.

Production projector deployments face three distinct categories of failure that require different handling:

**1. Poll Infrastructure Failures**

When the projector polls for events, the underlying infrastructure can fail:
- Database connection errors during `read_events_after()`
- Network timeouts reaching the event store
- Transient unavailability during database failover

These failures are about the polling mechanism itself, not about processing any particular event. The projector cannot make progress until infrastructure recovers, but the failure is typically transient.

**2. Event Processing Failures**

When `apply()` fails for a specific event:
- Read model database constraint violation
- Business logic error in projection
- External service unavailable during side effect

ADR-021 already addresses this: `on_error()` returns `FailureStrategy::Retry`, `Skip`, or `Fatal`. The projector trait callback decides how to handle each event's failure.

**3. Leadership Liveness Failures**

ADR-023's crash safety relies on "guard released on disconnect" - if the leader process terminates, the database connection closes, and advisory locks are released. But what about:
- Infinite loops in projection code
- Deadlocks waiting for resources
- Network partitions where the connection remains open but the process is unreachable

These scenarios represent a **hung projector**: the process holds leadership but makes no progress. Other instances cannot acquire leadership because the lock is held, yet no events are being processed. The system is stuck.

**Why These Are Three Separate Concerns:**

Conflating these categories creates confusion:

- Poll retry is about infrastructure recovery, measured in seconds to minutes
- Event retry is about individual event handling, controlled by application logic
- Liveness is about detecting process health, measured in heartbeat intervals

A projector with a poll backoff of 5 seconds is not the same as one with a 5-second heartbeat. Poll backoff says "check for new events every 5 seconds when idle." Heartbeat says "prove you're alive every 5 seconds or lose leadership."

**Why Per-Projector Configuration:**

Different projectors have different characteristics:

- A real-time notification projector needs sub-second poll intervals
- A daily reporting projector might poll every minute
- A projector doing expensive external API calls needs longer event retry delays
- A projector updating critical financial data needs tight liveness detection
- A projector building non-critical analytics can tolerate longer heartbeat intervals

Global configuration forces all projectors to the lowest common denominator. Per-projector configuration lets each projector match its operational profile.

**Key Forces:**

1. **Separation of Concerns**: Poll retry, event retry, and liveness are orthogonal. Mixing them creates confusing configuration where changing one behavior unexpectedly affects another.

2. **Operational Flexibility**: Production deployments need to tune projectors independently based on their workload characteristics and business criticality.

3. **Defensive Defaults**: Configuration should have sensible defaults that work for most cases. Explicit override should be required for non-standard behavior.

4. **Observability Integration**: Each concern produces different metrics and alerts. Clear separation enables targeted monitoring.

5. **Library Philosophy**: EventCore provides opinionated defaults. Configuration is escape hatches for production needs, not required ceremony.

## Decision

EventCore's projection runner accepts per-projector configuration for three distinct concerns:

**1. Poll Configuration (Infrastructure Level)**

Configuration for the polling loop itself:

- **poll_interval**: Duration between successful polls that returned events (default: 100ms)
- **empty_poll_backoff**: Duration to wait when poll returns no events (default: 1 second, configurable backoff strategy)
- **poll_failure_backoff**: Backoff strategy when `read_events_after()` fails (default: exponential, 100ms base, 30s max)
- **max_consecutive_poll_failures**: After this many failures, projector gives up and exits (default: 10, or "infinite" for never give up)

These parameters control how the projector interacts with the event store when there are no events to process or when infrastructure is degraded.

**2. Event Retry Configuration (Application Level)**

Configuration for `FailureStrategy::Retry` behavior:

- **max_retry_attempts**: Maximum retries before escalating to Fatal (default: 3)
- **retry_delay**: Initial delay between retries (default: 100ms)
- **retry_backoff_multiplier**: Multiplier for subsequent retry delays (default: 2.0)
- **max_retry_delay**: Cap on retry delay (default: 5 seconds)

These parameters control what happens when `on_error()` returns `Retry`. The projector trait callback still decides WHETHER to retry; this configuration controls HOW retries behave.

**3. Heartbeat Configuration (Coordination Level)**

Configuration for liveness detection, part of ProjectorCoordinator:

- **heartbeat_interval**: How often the leader must renew leadership (default: 5 seconds)
- **heartbeat_timeout**: How long before missed heartbeat causes leadership loss (default: 15 seconds, must be > heartbeat_interval)

These parameters are on ProjectorCoordinator, not Projector, because liveness is a coordination concern. The coordinator implementation decides how to implement heartbeats (e.g., updating a timestamp in the checkpoint table, renewing an advisory lock with timeout).

**Architectural Constraints:**

1. **Configuration Structs Are Separate**: Three distinct configuration types, not one combined struct. This makes the separation explicit in code.

2. **Builders with Defaults**: Configuration uses builder pattern with sensible defaults. Most projectors work with zero configuration.

3. **Heartbeat Is Coordinator Responsibility**: The ProjectorCoordinator trait includes heartbeat semantics. The projection runner calls `guard.heartbeat()` periodically; the coordinator implementation decides what that means.

4. **Guard Validity Incorporates Heartbeat**: `guard.is_valid()` from ADR-023 returns false if heartbeat has been missed. The projection loop already checks this.

5. **Per-Projector Override**: The projection runner accepts configuration per projector instance, not globally.

**What This Decision Does NOT Include:**

- No specific backoff algorithm mandate (implementations can choose exponential, linear, or constant)
- No circuit breaker patterns (application concern, not library concern)
- No automatic projector restart (orchestration concern - Kubernetes, systemd, etc.)
- No metrics emission (observability integration is separate)

## Rationale

**Why Three Separate Configuration Types:**

Each concern has different units, different defaults, and different operational meaning:

| Concern | Unit | Typical Range | Changed When |
|---------|------|---------------|--------------|
| Poll interval | Duration | 50ms - 60s | Latency requirements change |
| Event retry delay | Duration | 100ms - 30s | Transient failure patterns change |
| Heartbeat interval | Duration | 1s - 30s | Infrastructure SLAs change |

Combining them into one struct suggests they're related when they're not. A change to poll interval should not require reviewing heartbeat settings.

**Why Per-Projector Configuration:**

Consider a system with three projectors:

1. **UserNotificationProjector**: Needs sub-second latency for email triggers. poll_interval=50ms, heartbeat_interval=2s (critical path).

2. **AnalyticsSummaryProjector**: Batches updates every minute. poll_interval=60s, heartbeat_interval=30s (tolerates delay).

3. **AuditLogProjector**: Must never lose events. max_retry_attempts=10, retry_delay=1s (prioritizes correctness over speed).

Global configuration cannot serve all three. Per-projector configuration lets each match its requirements.

**Why Heartbeat Is Coordinator Responsibility:**

ADR-023 established that coordination is a backend concern because different backends have different primitives. The same applies to heartbeats:

- PostgreSQL can use `pg_advisory_lock` with timeout, or update a timestamp column
- In-memory can use tokio timeout on mutex
- External coordinators (etcd, Consul) have built-in TTL mechanisms

Making heartbeat part of the coordinator trait lets each backend use its natural primitives.

**Why Heartbeat Timeout > Heartbeat Interval:**

If heartbeat_interval=5s and heartbeat_timeout=5s, a single missed heartbeat causes leadership loss. Network jitter or GC pause would cause unnecessary failovers.

The timeout should be 2-3x the interval to tolerate transient delays while still detecting truly hung projectors.

**Why max_consecutive_poll_failures:**

A projector that cannot reach the event store indefinitely is not useful. At some point, it should exit and let the orchestration layer (Kubernetes, systemd) restart it. The restart might get a different database connection, hit a different replica, or benefit from infrastructure recovery.

"Infinite" mode is available for projectors in environments without orchestration, but the default assumes orchestration exists.

## Consequences

### Positive

- Clear mental model: three concerns, three configuration types
- Operational flexibility for diverse projector requirements
- Sensible defaults minimize required configuration
- Heartbeat prevents stuck leadership in distributed deployments
- Per-projector tuning enables optimal performance per workload

### Negative

- More configuration surface area to document and test
- Users must understand why configuration is split (additional learning curve)
- Heartbeat adds overhead to coordinator implementations
- Per-projector configuration requires more deployment configuration management

### Enabled Future Decisions

- Configuration presets for common patterns (e.g., "realtime", "batch", "critical")
- Dynamic configuration reload for operational flexibility
- Configuration validation helpers (e.g., ensure timeout > interval)

### Constrained Future Decisions

- Cannot easily add global configuration that overrides per-projector settings
- Heartbeat mechanism is fixed per coordinator implementation

## Alternatives Considered

### Alternative 1: Single Combined Configuration

**Description**: One configuration struct with all parameters: poll_interval, retry_delay, heartbeat_interval, etc.

**Why Rejected**: Conflates unrelated concerns. Users changing poll behavior must confront retry settings. No clear separation makes documentation and mental model harder. Different concerns evolve independently - combined config couples them.

### Alternative 2: Global Configuration with Per-Projector Override

**Description**: Define global defaults, let projectors override specific fields.

**Why Rejected**: Creates two places to look for configuration. Ordering of precedence becomes question. Most projectors should be independent - global defaults suggest they share characteristics when they often don't.

### Alternative 3: Heartbeat as Projector Trait Method

**Description**: Add `heartbeat_interval(&self) -> Duration` to Projector trait.

**Why Rejected**: Heartbeat is a coordination concern, not a projection concern. The Projector trait is about event processing. Mixing coordination into the trait conflates concerns. Coordinator implementations need flexibility in HOW heartbeat works, not just interval.

### Alternative 4: No Heartbeat (Rely on TCP Keepalive)

**Description**: Trust that database connections will eventually timeout on hung processes.

**Why Rejected**: TCP keepalive is measured in minutes (default often 2 hours on Linux). A hung projector blocks failover for far too long. Application-level heartbeat provides tighter bounds. Also, some hangs (deadlock with DB connection alive) keep the connection healthy at TCP level.

### Alternative 5: Heartbeat via Separate Process

**Description**: Run a watchdog process that monitors projector health externally.

**Why Rejected**: Adds operational complexity. Requires configuring and deploying additional infrastructure. The coordination layer already exists - adding heartbeat to it is simpler than adding a parallel monitoring system.
