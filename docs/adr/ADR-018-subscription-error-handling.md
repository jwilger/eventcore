# ADR-018: Subscription Error Handling Strategy

## Status

accepted

## Context

EventCore's subscription system (ADR-016) enables applications to build projections and read models by processing event streams. Unlike command execution where infrastructure can automatically retry transient failures (ADR-008), subscription processing errors require application-specific handling because the same error may demand different responses depending on projection semantics.

**The Core Problem:**

When a projection handler fails to process an event, the subscription must decide how to proceed. This decision cannot be made by the library because different projections have different tolerance for:

- **Data completeness**: Some projections require every event (analytics), others tolerate gaps (caches, dashboards)
- **Failure modes**: Deserialization errors vs business logic errors vs external API timeouts have different recovery strategies
- **Consistency requirements**: Financial projections must halt on error; notification handlers may skip failed events

The fundamental constraint is **ordering preservation**—events must never be processed out of order, as projections depend on temporal consistency for correctness.

**Key Forces:**

1. **Application-Specific Semantics**: Projection handlers know whether an error is recoverable, should be skipped, or requires halting the projection—the library cannot make this determination
2. **Ordering Guarantee**: At-least-once delivery (ADR-016) allows duplicate events but forbids reordering—error handling must preserve this invariant
3. **Fail-Fast vs Resilient**: Financial projections must crash on any error (data integrity); monitoring dashboards may skip failures (availability over consistency)
4. **Retry Complexity**: Unlike command retry (uniform OCC conflicts), subscription errors are heterogeneous—retry needs backoff configuration and attempt limits
5. **Silent Failures are Dangerous**: Default behavior must prevent projection drift (missing events without operator awareness)
6. **Developer Experience**: Error handling should be explicit but not burdensome—sensible defaults with opt-in flexibility
7. **Inspired by Proven Patterns**: Elixir's Commanded ecosystem uses projector failure callbacks successfully in production—similar pattern proven in practice

**Why Commands and Subscriptions Differ:**

ADR-008 established automatic retry for commands because version conflicts are uniform infrastructure failures with a single correct response (retry with fresh state). Subscription errors are fundamentally different:

- **Command errors**: Transient infrastructure failures (version conflicts, network timeouts) with uniform handling
- **Subscription errors**: Application-specific failures (business logic, external services, deserialization) with projection-specific handling

Commands modify state (must succeed exactly once); subscriptions observe state (must maintain ordering, may tolerate gaps or require perfect consistency).

**Why This Decision Now:**

ADR-016 established the subscription model but deferred error handling strategy. As applications build projections, they encounter diverse failure scenarios (poison events, transient API failures, schema evolution mismatches) that require flexible error handling. Without a clear strategy, applications will implement inconsistent error handling patterns, and the library risks defaulting to silent failure (the most dangerous choice).

## Decision

EventCore will provide configurable error handling for subscription processing through failure callbacks that enable application-specific recovery strategies while preserving event ordering:

**1. Error Handling Strategies**

Three strategies available to application code:

- **Fatal (Default)**: Stop processing and crash the subscription—safest option, prevents silent projection drift
- **Skip**: Log the error and continue to next event—tolerates gaps, useful for non-critical projections
- **Retry**: Retry the same event with exponential backoff—handles transient failures without losing events

**2. Failure Callback API**

Application provides a callback receiving rich failure context:

- **Event**: The event that caused the failure (for logging, dead letter queues)
- **Error**: The actual error from the projection handler
- **Attempt Count**: Current retry attempt number (enables escalation logic)
- **Stream Position**: Checkpoint position for recovery tracking

Callback returns desired strategy, potentially varying by error type or attempt count.

**3. Default Behavior: Fatal**

Subscriptions without explicit error handling crash on first failure:

- **Fail-Fast Philosophy**: Projection errors indicate bugs or environmental issues requiring operator attention
- **No Silent Data Loss**: Crashing ensures operators discover problems immediately
- **Explicit Opt-In**: Applications that want resilience must consciously choose skip or retry strategies

**4. Ordering Preservation Guarantee**

All strategies preserve temporal ordering:

- **Fatal**: Stops stream, no further processing—no ordering violation possible
- **Skip**: Skips current event, continues to next in order—gap in projection, but ordering maintained
- **Retry**: Retries same event, blocks subsequent events—ordering preserved by waiting

This is non-negotiable: EventCore will never deliver events out of order, regardless of error handling strategy.

**5. Retry Configuration**

Retry strategy requires explicit configuration:

- **Max Attempts**: Hard limit on retry attempts before escalating to Fatal
- **Backoff Policy**: Exponential backoff with configurable base delay and multiplier (similar to ADR-008 command retry)
- **Jitter**: Optional randomization to prevent thundering herd
- **Timeout**: Per-retry attempt timeout (prevents hanging on slow operations)

Retry exhaustion escalates to Fatal to prevent infinite retry loops.

**6. Integration with EventSubscription**

Error handling configured when consuming the subscription stream, not at subscription creation:

- Subscription returns `Stream<Item = E>` (clean separation per ADR-016)
- Application wraps stream with error handling middleware
- Error handling is processing concern, not query concern (orthogonal to SubscriptionQuery)

This maintains ADR-016's separation between event selection (SubscriptionQuery) and event processing (application code).

**7. Observability Integration**

Error handling emits structured events for monitoring:

- **Error logged**: Every failure logged with full context (event, error, strategy decision)
- **Skip counted**: Skipped events increment metric for projection completeness tracking
- **Retry tracked**: Retry attempts and backoff duration tracked for latency monitoring
- **Fatal reported**: Fatal failures reported with sufficient context for debugging

## Rationale

**Why Callback-Based Error Handling:**

Three approaches were considered for error handling control:

1. **Result propagation**: Handler returns `Result<(), E>`, caller decides on error
2. **Error channel**: Out-of-band channel for error communication
3. **Failure callback**: Callback receives failure context, returns strategy

Callback-based approach won because:

- **Rich context**: Callback receives event, error, attempt count—enables informed decisions
- **Inline control flow**: Strategy decision happens synchronously, simplifying reasoning
- **Proven pattern**: Elixir Commanded uses this pattern successfully in production
- **Flexibility**: Callback can implement arbitrary logic (error type matching, attempt escalation, circuit breakers)

Result propagation loses context (which event failed? how many retries occurred?). Error channels complicate control flow (separate goroutine/task monitoring channel, coordination complexity).

**Why Fatal is Default:**

Three default behaviors were considered:

1. **Fatal (crash)**: Stop processing, crash subscription
2. **Skip**: Log and skip failed events
3. **Retry indefinitely**: Keep retrying until success

Fatal is the only safe default:

- **Skip creates silent data loss**: Projections drift from source events without operator awareness—financial projections with missing transactions are silently incorrect
- **Infinite retry hangs**: Permanent failures (deserialization errors, poison events) cause subscription to hang indefinitely
- **Fatal prevents drift**: Crash forces operator intervention, ensures errors are discovered and fixed
- **Fail-fast philosophy**: Systems should crash loudly rather than fail silently—aligns with Rust's panic on unrecoverable errors

Applications that want resilience (skip or retry) must consciously opt in, documenting their tolerance for gaps or retries.

**Why Three Strategies (Not More):**

Additional strategies were considered:

- **Dead letter queue**: Park failed event in separate stream for later processing
- **Circuit breaker**: Stop processing after N consecutive failures
- **Fallback handler**: Invoke alternate handler on primary failure

These are compositional patterns built on top of the three fundamental strategies:

- **Dead letter queue**: Callback implements skip + write to dead letter stream
- **Circuit breaker**: Callback tracks consecutive failures, returns Fatal after threshold
- **Fallback handler**: Callback invokes fallback, returns Skip if fallback succeeds

Providing primitives (Fatal, Skip, Retry) enables applications to compose higher-level patterns without library complexity.

**Why Ordering Must Be Preserved:**

Projections depend on temporal ordering for correctness:

- **Financial ledgers**: Deposits must be processed before withdrawals that depend on them
- **State machines**: State transitions must occur in event order
- **Aggregate queries**: Counts and sums must process all events in order

Out-of-order processing breaks projection invariants. At-least-once delivery (ADR-016) allows duplicate events (idempotent handlers required), but reordering would corrupt projections in ways idempotency cannot fix.

All three strategies preserve ordering:

- **Fatal**: Stops processing, no subsequent events delivered
- **Skip**: Skips current event, continues to next in order (gap, not reorder)
- **Retry**: Retries current event, subsequent events wait (no advancement until success)

This is a hard constraint: EventCore will not add strategies that violate ordering.

**Why Error Handling Differs from Command Retry:**

ADR-008 established automatic retry for command execution because version conflicts are uniform infrastructure failures. Subscription errors are application domain:

- **Commands**: Version conflicts, network timeouts—uniform transient failures with single correct response (retry with fresh state)
- **Subscriptions**: Deserialization errors, business logic failures, external API timeouts—heterogeneous failures with projection-specific responses

Command retry is library behavior (infrastructure); subscription error handling is application behavior (projection semantics). The library provides the mechanism (callback), the application provides the policy (Fatal, Skip, Retry).

**Why Retry Needs Configuration:**

Naive retry (immediate retry, no limit) causes problems:

- **Permanent failures**: Deserialization errors retry forever (poison event blocks subscription)
- **Tight loops**: Immediate retry thrashes CPU and logs (no backoff)
- **Cascading failures**: Multiple failing projections retry simultaneously (thundering herd)

Exponential backoff with max attempts addresses these:

- **Max attempts**: Prevents infinite retry on permanent failures (escalates to Fatal after limit)
- **Exponential backoff**: Spaces retry attempts progressively (reduces contention and log spam)
- **Jitter**: Randomizes backoff to prevent synchronized retries (avoids thundering herd)

This mirrors ADR-008 command retry configuration but applies to application-initiated retry (not automatic).

**Why Skip is Dangerous (But Sometimes Needed):**

Skip creates projection drift—projection missing events that exist in event stream. This is acceptable for:

- **Caches**: Missing cache entries cause cache miss, data re-fetched from source
- **Dashboards**: Slightly stale metrics acceptable for monitoring use case
- **Notifications**: Failed notification can be dropped (user doesn't receive duplicate notifications)

Skip is unacceptable for:

- **Financial ledgers**: Missing transaction corrupts account balances
- **Audit logs**: Missing audit event violates compliance requirements
- **Aggregate queries**: Missing event skews counts and sums

Skip must be explicit opt-in with documented risk.

**Trade-offs Accepted:**

- **Developer responsibility**: Applications must implement error callbacks (cannot rely on library defaults forever)—accepted because projection error handling is inherently application-specific
- **Skip risk**: Skip strategy can cause projection drift if misused—accepted because some projections legitimately tolerate gaps
- **Retry latency**: Retry with backoff delays processing of subsequent events—accepted because ordering preservation requires sequential processing
- **Configuration complexity**: Retry configuration (max attempts, backoff policy) adds API surface—accepted because naive retry causes worse problems

## Consequences

### Positive

- **Data integrity by default**: Fatal default prevents silent projection drift without operator awareness
- **Flexibility for application needs**: Callback approach enables projection-specific error handling strategies
- **Ordering guarantee maintained**: All strategies preserve temporal ordering required for projection correctness
- **Proven pattern**: Mirrors Elixir Commanded's successful projector failure callback design
- **Composable strategies**: Primitives (Fatal, Skip, Retry) enable building higher-level patterns (circuit breakers, dead letter queues)
- **Observability built-in**: All error handling strategies emit structured logs and metrics

### Negative

- **Developer must handle errors**: Applications cannot ignore error handling indefinitely (Fatal crashes require intervention)—acceptable because projections are application code, not library magic
- **Skip enables projection drift**: Skip strategy can cause data loss if misused on critical projections—acceptable because some projections legitimately need this flexibility
- **Retry blocks stream**: Retry with backoff delays processing of subsequent events, increasing latency—acceptable because ordering preservation requires sequential processing
- **Configuration required for retry**: Retry strategy needs max attempts and backoff configuration—acceptable because naive retry causes worse problems (infinite loops, tight retry cycles)
- **No automatic error classification**: Library does not classify errors as retriable vs permanent (unlike ADR-008)—acceptable because subscription errors are application domain, not infrastructure

### Enabled Future Decisions

- **Dead letter queue helpers**: Provide utilities for writing failed events to dead letter streams (composes with Skip strategy)
- **Circuit breaker middleware**: Detect consecutive failures and escalate to Fatal after threshold (composes with callback logic)
- **Retry budget**: Limit total retry time across all events in a subscription window
- **Poison event detection**: Identify events that fail consistently across retries, automatically route to dead letter queue
- **Custom backoff strategies**: Support alternate backoff algorithms (linear, exponential with ceiling, adaptive based on error type)
- **Metrics aggregation**: Track skip rates, retry rates, fatal failure rates per projection for operational dashboards
- **Fallback handlers**: Invoke secondary handler on primary failure (composes with Skip strategy)

### Constrained Future Decisions

- **Ordering is non-negotiable**: Cannot add strategies that violate temporal ordering guarantee—projection correctness depends on this
- **Fatal must remain default**: Changing default to Skip or Retry would risk silent data loss in existing projections—breaking change
- **Callback signature stability**: FailureContext structure is part of public API—changes require careful versioning
- **No automatic retry by library**: Subscription errors are not automatically retried like command OCC conflicts—application must opt in via callback
- **Skip does not checkpoint**: Skipped events do not advance checkpoint (projection can resume from before skip)—changing this would be breaking

## Alternatives Considered

### Alternative 1: Return Result from Handler, Library Decides Strategy

**Description**: Projection handlers return `Result<(), ProjectionError>`, library inspects error type and applies fixed strategy (retry on transient, fatal on permanent).

**Why Rejected**:

Assumes library can classify subscription errors into retriable vs permanent, but classification is projection-specific:

- Deserialization error: Skip for cache projection (stale schema), Fatal for financial ledger (data integrity)
- External API timeout: Retry for enrichment projection (transient), Fatal for compliance projection (must process all)
- Business rule violation: Skip for metrics (tolerate gaps), Fatal for state machine (consistency required)

Same error type requires different handling depending on projection semantics. Library cannot make this determination—only application understands projection requirements.

ADR-008 command retry works because version conflicts are uniform infrastructure failures. Subscription errors are application domain concerns.

### Alternative 2: Error Channel for Out-of-Band Handling

**Description**: Subscription returns both event stream and error channel. Application monitors channel separately and decides recovery strategy.

**Why Rejected**:

Error channels complicate control flow and lose context:

- **Separate task required**: Application must spawn task to monitor error channel (coordination complexity)
- **Context loss**: Error channel receives error, but which event failed? What's the current position?
- **Ordering coordination**: How does error handler signal to resume stream processing? Requires additional coordination channel
- **Unclear ownership**: Who owns retry logic? When does stream advance past failed event?

Callback-based approach keeps control flow inline and provides rich context (event, error, attempt count) in single call.

### Alternative 3: Automatic Retry for All Errors (Like Command Retry)

**Description**: Automatically retry all subscription errors with exponential backoff, similar to ADR-008 command retry.

**Why Rejected**:

Uniform retry assumes all errors are transient, but many subscription errors are permanent:

- **Deserialization errors**: Retry won't fix schema mismatch (poison event blocks subscription forever)
- **Business logic errors**: Retry won't change validation outcome (same error every attempt)
- **API 404 errors**: Retry won't make missing resource appear (permanent failure)

Command retry works because OCC conflicts are definitionally transient (another command committed, retry with fresh state succeeds). Subscription errors are heterogeneous—some transient, some permanent, classification depends on projection context.

Automatic retry on permanent failures causes infinite retry loops. Applications know which errors are retriable.

### Alternative 4: Skip by Default (Resilience Over Correctness)

**Description**: Default behavior logs error and skips failed event, maximizing projection availability.

**Why Rejected**:

Skip creates silent data loss—projection missing events without operator awareness:

- **Financial projections**: Missing transactions corrupt account balances silently
- **Compliance projections**: Missing audit events violate regulatory requirements
- **State machines**: Missing events leave projection in inconsistent state

Availability over correctness is valid for some projections (caches, dashboards), but dangerous as default:

- Applications inheriting default may not realize they're losing events
- Projection drift discovered weeks/months later when discrepancies detected
- Recovery requires rebuilding projection from event stream (operational burden)

Fatal default forces conscious decision: "Is this projection safe to skip events?" Applications that answer "yes" opt into Skip explicitly.

### Alternative 5: Strategy Configuration at Subscription Creation

**Description**: Configure error handling when calling `subscribe()`, not when consuming stream.

**Why Rejected**:

Violates ADR-016's separation of concerns—SubscriptionQuery describes event selection, not event processing:

- **Query vs processing**: SubscriptionQuery filters events (stream prefix, event type), error handling is processing concern
- **Multiple consumers**: Same subscription may be consumed by different handlers with different error tolerance (query reuse broken)
- **API pollution**: Subscription API grows to include retry configuration, callback registration (violates single responsibility)

Error handling belongs at consumption point (stream processing), not query definition point (event selection). This maintains clean separation: EventSubscription delivers events, application code processes events with chosen error handling.

### Alternative 6: No Strategies, Just Callback (Ultimate Flexibility)

**Description**: Callback returns boolean: `true` = continue (skip), `false` = stop (fatal). No retry strategy, application implements retry loop.

**Why Rejected**:

Pushes retry complexity to every application:

- **Retry loop boilerplate**: Applications must implement exponential backoff, max attempts, attempt tracking
- **Inconsistent retry**: Different projections implement different retry algorithms (no standardization)
- **Ordering risk**: Application retry loop must carefully preserve ordering (easy to get wrong)
- **Lost benefit**: Library can provide well-tested retry mechanism, avoiding duplication

Providing Retry as built-in strategy gives applications correct retry behavior without boilerplate, while still allowing custom strategies via callback composition.

### Alternative 7: Panic on Error (No Recovery)

**Description**: Projection handler errors panic, crashing the process. No error handling strategies.

**Why Rejected**:

Too inflexible for diverse projection needs:

- **Cache projections**: Can tolerate gaps, don't want crash on deserialization error
- **Notification projections**: Can skip failed notifications, don't want crash on API timeout
- **Development**: Want graceful error handling during iterative projection development

Panic is equivalent to Fatal strategy but with no opt-out. Providing Fatal as default with Skip/Retry opt-in gives safety by default with flexibility when needed.

## References

- **ADR-016**: Event Subscription Model (established EventSubscription trait and at-least-once delivery semantics)
- **ADR-008**: Command Executor and Retry Logic (established automatic retry for command execution, contrasts with application-driven subscription retry)
- **ADR-004**: Error Handling Hierarchy (error classification and context enrichment patterns)
- **Elixir Commanded Projectors**: Inspiration for failure callback pattern
- **Martin Dilger "Understanding Event Sourcing"** Ch4: Projections and error handling considerations
