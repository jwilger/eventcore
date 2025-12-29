# ADR-027: Projector Poll and Retry Configuration

## Status

accepted

## Supersedes

ADR-024 (Projector Configuration and Liveness Detection)

## Context

ADR-024 established per-projector configuration for three concerns: poll configuration, event retry configuration, and heartbeat configuration. ADR-026 subsequently eliminated the need for heartbeat configuration entirely by adopting session-scoped advisory locks on dedicated connections, where connection lifecycle provides liveness detection automatically.

ADR-024's heartbeat configuration section is now obsolete and marked as superseded within that document. However, ADR-024's poll and event retry configuration remain valid and are actively implemented. This creates confusion: ADR-024 is partially valid, with inline notes indicating which sections still apply.

Rather than maintaining a partially-superseded ADR, this ADR captures the still-valid configuration concerns as a clean, standalone decision that reflects the current architecture.

**Two Distinct Configuration Concerns:**

Production projector deployments face two categories of concern that require separate configuration:

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

ADR-021 established that `on_error()` returns `FailureStrategy::Retry`, `Skip`, or `Fatal`. The projector trait callback decides WHETHER to retry; configuration controls HOW retries behave.

**Why These Are Separate Concerns:**

Conflating these categories creates confusion:

- Poll retry is about infrastructure recovery, measured in seconds to minutes
- Event retry is about individual event handling, controlled by application logic

A projector with a poll backoff of 5 seconds is not the same as one with a 5-second event retry delay. Poll backoff says "check for new events every 5 seconds when idle." Event retry delay says "wait 5 seconds before retrying a failed event."

**Why Per-Projector Configuration:**

Different projectors have different characteristics:

- A real-time notification projector needs sub-second poll intervals
- A daily reporting projector might poll every minute
- A projector doing expensive external API calls needs longer event retry delays
- A projector updating critical financial data prioritizes correctness over speed

Global configuration forces all projectors to the lowest common denominator. Per-projector configuration lets each projector match its operational profile.

**Coordination Handled Separately:**

Per ADR-026, coordination uses session-scoped advisory locks on dedicated connections. This eliminated heartbeat configuration entirely:
- No `heartbeat_interval` parameter
- No `heartbeat_timeout` parameter
- No heartbeat table updates
- Connection alive = leadership held

Stuck projector detection is handled via application monitoring (metrics, health checks on subscriptions table timestamps), not infrastructure coordination primitives.

## Decision

EventCore's projection runner accepts per-projector configuration for two distinct concerns:

**1. Poll Configuration (Infrastructure Level)**

Configuration for the polling loop itself:

- **poll_interval**: Duration between successful polls that returned events (default: 100ms)
- **empty_poll_backoff**: Duration to wait when poll returns no events (default: 1 second)
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

**Architectural Constraints:**

1. **Configuration Structs Are Separate**: Two distinct configuration types (`PollConfig`, `EventRetryConfig`), not one combined struct. This makes the separation explicit in code.

2. **Builders with Defaults**: Configuration uses builder pattern with sensible defaults. Most projectors work with zero configuration.

3. **Per-Projector Override**: The projection runner accepts configuration per projector instance, not globally.

4. **Coordination Is Not Configuration**: Per ADR-026, coordination uses session-scoped advisory locks on dedicated connections. There is no coordination configuration to expose.

**What This Decision Does NOT Include:**

- No specific backoff algorithm mandate (implementations can choose exponential, linear, or constant)
- No circuit breaker patterns (application concern, not library concern)
- No automatic projector restart (orchestration concern - Kubernetes, systemd, etc.)
- No metrics emission (observability integration is separate)
- No heartbeat configuration (superseded by ADR-026)

## Rationale

**Why Two Separate Configuration Types:**

Each concern has different units, different defaults, and different operational meaning:

| Concern | Unit | Typical Range | Changed When |
|---------|------|---------------|--------------|
| Poll interval | Duration | 50ms - 60s | Latency requirements change |
| Event retry delay | Duration | 100ms - 30s | Transient failure patterns change |

Combining them into one struct suggests they're related when they're not. A change to poll interval should not require reviewing retry settings.

**Why Per-Projector Configuration:**

Consider a system with three projectors:

1. **UserNotificationProjector**: Needs sub-second latency for email triggers. poll_interval=50ms.

2. **AnalyticsSummaryProjector**: Batches updates every minute. poll_interval=60s.

3. **AuditLogProjector**: Must never lose events. max_retry_attempts=10, retry_delay=1s.

Global configuration cannot serve all three. Per-projector configuration lets each match its requirements.

**Why max_consecutive_poll_failures:**

A projector that cannot reach the event store indefinitely is not useful. At some point, it should exit and let the orchestration layer (Kubernetes, systemd) restart it. The restart might get a different database connection, hit a different replica, or benefit from infrastructure recovery.

"Infinite" mode is available for projectors in environments without orchestration, but the default assumes orchestration exists.

**Why No Heartbeat Configuration:**

ADR-026 established that session-scoped advisory locks on dedicated connections provide liveness detection through connection lifecycle:

- Connection alive = lock held = projector active
- Connection dies = lock released = leadership available

No heartbeat interval or timeout is needed. This is simpler and more reliable than heartbeat-table approaches, as proven by EventStore/Commanded in production.

## Consequences

### Positive

- Clear mental model: two concerns, two configuration types
- Operational flexibility for diverse projector requirements
- Sensible defaults minimize required configuration
- Per-projector tuning enables optimal performance per workload
- No heartbeat configuration complexity (handled by ADR-026)

### Negative

- More configuration surface area than single combined struct
- Users must understand why configuration is split (additional learning curve)
- Per-projector configuration requires more deployment configuration management

### Enabled Future Decisions

- Configuration presets for common patterns (e.g., "realtime", "batch", "critical")
- Dynamic configuration reload for operational flexibility
- Configuration validation helpers

### Constrained Future Decisions

- Cannot easily add global configuration that overrides per-projector settings
- Cannot reintroduce heartbeat configuration without reconsidering ADR-026

## Alternatives Considered

### Alternative 1: Single Combined Configuration

**Description**: One configuration struct with all parameters: poll_interval, retry_delay, etc.

**Why Rejected**: Conflates unrelated concerns. Users changing poll behavior must confront retry settings. No clear separation makes documentation and mental model harder. Different concerns evolve independently - combined config couples them.

### Alternative 2: Global Configuration with Per-Projector Override

**Description**: Define global defaults, let projectors override specific fields.

**Why Rejected**: Creates two places to look for configuration. Ordering of precedence becomes question. Most projectors should be independent - global defaults suggest they share characteristics when they often don't.

### Alternative 3: Keep ADR-024 with Inline Supersession Notes

**Description**: Continue using ADR-024 with inline notes marking the heartbeat section as superseded.

**Why Rejected**: Partially-superseded ADRs create confusion. Readers must mentally filter which sections apply. A clean ADR capturing only the valid concerns is clearer. ADRs should be historical facts, not evolving documents.

### Alternative 4: Merge Into ADR-026

**Description**: Add configuration details to ADR-026 since it already discusses projection coordination.

**Why Rejected**: ADR-026 is about coordination (advisory locks, dedicated connections). Poll and retry configuration are separate concerns. Merging would conflate coordination with configuration, the same mistake ADR-024 made with heartbeat configuration.
