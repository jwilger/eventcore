# ADR-008: Command Executor and Retry Logic

## Status

accepted

## Context

EventCore commands require orchestration that coordinates stream reading, state reconstruction, business logic execution, and atomic event persistence. Under concurrent load, optimistic concurrency conflicts (ADR-007) are inevitable when multiple commands access the same streams. Manual retry logic in every command would violate NFR-2.1 (minimal boilerplate) and create inconsistent handling across applications.

**Key Forces:**

1. **Orchestration Complexity**: Commands involve multiple phases (read streams, apply events, handle logic, write events) that must be coordinated correctly
2. **Automatic Conflict Resolution**: ADR-007's optimistic concurrency produces ConcurrencyError on version conflicts - these should retry automatically without developer intervention
3. **Developer Experience**: Retry logic is infrastructure concern, not business logic - developers shouldn't implement it repeatedly
4. **Retry Policy Configuration**: Different applications have different contention characteristics requiring configurable retry behavior (max attempts, backoff strategy)
5. **Metadata Preservation**: Correlation and causation IDs from ADR-005 must remain consistent across retry attempts
6. **Integration Point**: Executor bridges all prior ADRs - EventStore (002), type system (003), error handling (004), metadata (005), command traits (006), and OCC (007)
7. **Error Propagation**: Non-retriable errors (validation, business rules) must fail immediately without retry, per ADR-004 classification
8. **State Freshness**: Each retry must re-read streams to ensure fresh state, not retry with stale data
9. **Contention Management**: Exponential backoff reduces lock contention by spacing retry attempts

**Why This Decision Now:**

The command executor is the central component that brings together all previous architectural decisions into a cohesive execution model. This decision defines the developer-facing API for command execution, automatic retry semantics, and the boundary between infrastructure (executor) and business logic (commands). It must be defined before implementing the executor to ensure consistent behavior and clear separation of concerns.

## Decision

EventCore will provide a CommandExecutor that orchestrates complete command execution with automatic retry on version conflicts:

**1. Executor Responsibilities**

The CommandExecutor handles all infrastructure concerns:

- **Stream Resolution**: Extract stream IDs from command via CommandStreams trait (ADR-006)
- **State Loading**: Read events from EventStore and reconstruct state via CommandLogic::apply (ADR-002, ADR-006)
- **Business Logic Invocation**: Call CommandLogic::handle with reconstructed state
- **Event Persistence**: Write generated events atomically to EventStore with version checking (ADR-002, ADR-007)
- **Automatic Retry**: Retry entire flow on ConcurrencyError (ADR-004, ADR-007)
- **Metadata Generation**: Populate correlation and causation IDs consistently across retries (ADR-005)
- **Error Classification**: Distinguish retriable vs permanent failures and handle appropriately (ADR-004)

**2. Command Execution Flow**

Complete execution follows a structured multi-phase pattern:

**Phase 1 - Stream Resolution:**
- Extract stream IDs from command using CommandStreams trait
- Support both static streams (from #[stream] fields) and dynamic streams (via StreamResolver)
- Build complete set of streams to read

**Phase 2 - Read with Version Capture:**
- Read events from all identified streams via EventStore::read
- Capture current version for each stream (optimistic concurrency baseline)
- Empty streams have version 0

**Phase 3 - State Reconstruction:**
- Fold events through CommandLogic::apply to build current state
- State represents command's view of the world at captured versions

**Phase 4 - Business Logic Execution:**
- Invoke CommandLogic::handle with reconstructed state
- Business logic validates rules and produces events
- Events prepared but not yet committed

**Phase 5 - Atomic Write with Version Check:**
- Attempt EventStore::append with expected versions
- Backend verifies ALL stream versions match expectations atomically
- Success: events committed, execution complete
- ConcurrencyError: proceed to retry logic
- Other error: propagate immediately (no retry)

**3. Automatic Retry Logic**

On ConcurrencyError from optimistic concurrency conflict:

**Retry Decision:**
- Check current retry attempt count against configured maximum
- If under limit: initiate retry with backoff
- If at limit: fail with descriptive error indicating max retries exceeded

**Exponential Backoff:**
- First retry: short delay (e.g., 10ms)
- Subsequent retries: exponentially increasing delay (e.g., 20ms, 40ms, 80ms)
- Configurable base delay and multiplier
- Reduces contention by spacing concurrent retry attempts
- Optional jitter to prevent thundering herd

**Retry Execution:**
- Return to Phase 2 (read with fresh version capture)
- Re-read ALL streams to get fresh state (never retry with stale data)
- Re-execute business logic with fresh state
- Attempt atomic write again with new versions
- Repeat until success or max retries exceeded

**4. Metadata Consistency Across Retries**

Correlation and causation IDs remain stable:

- **Correlation ID**: Generated once at executor invocation, preserved across all retries
- **Causation ID**: Typically the command ID, remains constant across retries
- **Consistent Context**: Same logical operation regardless of retry count
- **Timestamp**: Set at commit time (Phase 5), not at initial attempt - represents when event actually persisted

**5. Error Handling Strategy**

Executor distinguishes error categories per ADR-004:

**Retriable Errors (Automatic Retry):**
- ConcurrencyError: Version conflict, expected under concurrent load
- Network timeouts (if storage backend indicates retriable)
- Retry with backoff until success or max attempts

**Permanent Errors (Immediate Failure):**
- ValidationError: Invalid command data
- CommandError::BusinessRuleViolation: Business logic rejection
- EventStoreError::Permanent: Non-retriable storage failures
- Propagate immediately to caller, no retry attempts

**6. Configuration and Extensibility**

Executor behavior is configurable:

- **Max Retry Attempts**: Default reasonable (e.g., 5), configurable per deployment
- **Backoff Strategy**: Base delay, multiplier, optional jitter
- **Timeout**: Overall command execution timeout (across all retries)
- **Custom Retry Policies**: Applications can provide custom retry logic for specific command types

**7. Developer Experience**

From developer perspective, retry is transparent:

```rust
// Developer code - no retry logic needed
let result = executor.execute(transfer_command).await?;

// Executor handles:
// - Reading streams
// - Applying events
// - Calling business logic
// - Atomic writes
// - Automatic retry on conflicts
// - All infrastructure concerns
```

Developer implements only CommandLogic (apply, handle) - executor handles everything else.

## Rationale

**Why Executor Handles Retry (Not Commands):**

Retry logic is pure infrastructure concern with no business logic:

- Same retry strategy applies to all commands (conflicts are infrastructure failures)
- Duplicating retry logic in every command violates DRY principle
- Inconsistent retry behavior across commands creates unpredictable system behavior
- Developers shouldn't need to understand OCC internals to use EventCore
- Aligns with NFR-2.1 (minimal boilerplate) - infrastructure handles infrastructure concerns
- Enables centralized retry policy management and monitoring

Alternative (manual retry) forces every command to implement same infrastructure pattern.

**Why Retry Entire Execution Flow:**

Version conflicts mean command operated on stale state - partial retry insufficient:

- State reconstruction must use fresh events (can't reuse stale state)
- Business logic decision may change with fresh state (different events produced)
- Stream versions captured during read phase establish OCC baseline
- Retrying only write phase would use stale versions (conflict again)
- Clean separation: retry from read phase ensures consistent semantics

**Why Exponential Backoff:**

Linear retry intervals under contention cause problems:

- Multiple conflicting commands retry simultaneously → immediate re-conflict
- "Thundering herd" where all retry at same instant wastes resources
- Exponential backoff spaces retry attempts progressively
- Reduces database contention by staggering retry timing
- Standard practice in distributed systems (database retries, network retries, etc.)
- Jitter further randomizes timing to prevent synchronized retries

Alternative (immediate retry) causes thrashing under high contention.

**Why Metadata Preserved Across Retries:**

Correlation and causation track logical operations, not physical attempts:

- Correlation ID groups all events from single business operation
- Retry attempts are part of same operation (same correlation ID)
- Causation ID identifies command that triggered events (unchanged by retry)
- Distributed tracing tools see single operation with potential delays (retries invisible)
- Consistency with tracing semantics: operation identity independent of retry count

Alternative (new correlation ID per retry) fragments operation traces incorrectly.

**Why Re-Read Streams on Retry:**

Optimistic concurrency conflicts indicate concurrent modifications:

- Another command committed events since our initial read
- State may have changed in ways affecting business logic
- Reusing old state would produce events based on stale information
- Business rules may reject operation with fresh state
- Re-reading ensures correctness - command sees current state

Alternative (retry with stale state) defeats purpose of optimistic concurrency control.

**Why Error Classification Prevents Retry:**

Not all errors are transient - some failures are permanent:

- Validation errors won't succeed on retry (invalid data remains invalid)
- Business rule violations depend on command inputs (retry produces same rejection)
- Permanent storage errors indicate configuration issues (retry futile)
- Wasting retry attempts on permanent failures delays error feedback
- ADR-004 classification enables correct handling strategy

Alternative (retry everything) wastes resources and delays failure feedback.

**Why Configurable Retry Policy:**

Different applications have different contention characteristics:

- Low-contention systems need fewer retries (conflicts rare)
- High-contention systems need more aggressive retry (conflicts common)
- Production vs development environments have different tolerance for latency
- Regulatory systems may have strict timeout requirements
- Applications know their performance requirements better than library defaults

Alternative (fixed retry policy) creates one-size-fits-all constraints.

**Trade-offs Accepted:**

- **Hidden Retry Cost**: Developers may not realize retry overhead under contention (mitigated by observability)
- **Latency Under Contention**: Retries increase command latency when conflicts occur (acceptable for correctness)
- **Resource Usage**: Failed attempts consume CPU and database resources (necessary cost of optimistic concurrency)
- **Timeout Complexity**: Overall timeouts must account for retry delays (configurable timeout mitigates)
- **Debugging Indirection**: Errors may come from later retry attempt, not initial attempt (metadata and logging address this)

These trade-offs are acceptable because:

- Automatic retry is core value proposition (developer experience)
- Latency under contention preferable to manual retry implementation
- Resource cost of retry negligible compared to developer time implementing manual retry
- Configurable policies allow tuning for application characteristics
- Observability and metadata enable debugging despite retry indirection

## Consequences

**Positive:**

- **Zero Retry Boilerplate**: Developers write no retry logic - executor handles automatically
- **Consistent Behavior**: All commands use same proven retry strategy
- **Transparent OCC**: Developers benefit from optimistic concurrency without understanding internals
- **Configurable Policy**: Applications tune retry behavior for their contention characteristics
- **Fresh State Guarantee**: Re-reading streams ensures business logic operates on current state
- **Metadata Consistency**: Correlation and causation preserved across retries for accurate tracing
- **Proper Error Handling**: Permanent failures fail fast, retriable failures retry appropriately
- **Reduced Contention**: Exponential backoff reduces database lock contention
- **Observable**: Retry attempts and conflicts visible in metrics and logs
- **Integration Point**: Brings together all prior ADRs into cohesive execution model

**Negative:**

- **Hidden Complexity**: Retry logic invisible to developers may cause surprise under high contention
- **Latency Variability**: Retry delays create variable command latency (unpredictable timing)
- **Resource Amplification**: High conflict rates multiply resource usage (each retry consumes resources)
- **Debugging Challenge**: Multi-attempt executions harder to debug than single attempts
- **Timeout Tuning**: Applications must set timeouts accounting for retry delays
- **Eventual Failure Possible**: Max retry limit means eventual failure under sustained contention

**Enabled Future Decisions:**

- Circuit breaker patterns can detect sustained high conflict rates and prevent retry storms
- Monitoring dashboards can track retry rates, conflict patterns, contention hot spots
- Custom retry strategies can be injected for specific command types or scenarios
- Adaptive backoff can adjust based on observed conflict rates
- Retry budgets can limit total retry time across all commands in a request
- Chaos testing can inject conflicts to verify retry behavior
- Performance optimization can identify and partition high-contention streams
- Observability integrations can correlate retries with business metrics

**Constrained Future Decisions:**

- Executor must always retry ConcurrencyError (fundamental to automatic retry guarantee)
- Retry must re-read streams from Phase 2 (cannot cache state across retries)
- Correlation and causation IDs must remain constant across retry attempts
- Permanent errors must never trigger retry (ADR-004 classification binding)
- Retry configuration must support at minimum: max attempts, base delay, multiplier
- Executor interface must remain stable (breaking changes affect all consumers)
- Retry logic must remain infrastructure concern (cannot leak into CommandLogic implementations)

## Alternatives Considered

### Alternative 1: No Automatic Retry - Expose Conflicts to Developers

Executor detects version conflicts but returns error immediately, requiring developers to implement retry.

**Rejected Because:**

- Violates NFR-2.1 (minimal boilerplate) - every command needs retry implementation
- Inconsistent retry behavior across applications (each developer implements differently)
- High probability of incorrect implementations (missing backoff, retrying permanent errors)
- Retry is infrastructure concern with no business logic - shouldn't be developer responsibility
- Standard event sourcing pattern is automatic retry on version conflicts
- Defeats purpose of having executor orchestrate infrastructure concerns
- Adds cognitive load and error potential for no benefit
- EventCore value proposition includes handling OCC gracefully

### Alternative 2: Fixed Retry Policy (No Configuration)

Executor retries with hardcoded policy (e.g., always 5 attempts, fixed backoff).

**Rejected Because:**

- Different applications have vastly different contention characteristics
- Low-contention systems don't need aggressive retry (wastes time on rare conflicts)
- High-contention systems need more retries to achieve acceptable success rates
- Development vs production environments have different latency tolerances
- Performance requirements vary by use case (batch processing vs user-facing)
- One-size-fits-all policies create suboptimal behavior for most applications
- Configuration enables tuning based on observed behavior in production

### Alternative 3: Retry Only Write Phase (Don't Re-Read Streams)

On conflict, retry only the EventStore::append operation with same events and incremented versions.

**Rejected Because:**

- Defeats purpose of optimistic concurrency control (detect stale state)
- Business logic executed on stale state may produce incorrect events
- Stream state may have changed in ways that affect business rules
- Version numbers from initial read are outdated (conflict again)
- Cannot increment versions without knowing current version (requires re-read)
- Correctness compromised - events based on stale state are wrong
- Violates event sourcing principle: state reconstruction determines behavior

### Alternative 4: Pessimistic Locking (Acquire Locks Before Reading)

Replace optimistic concurrency with lock acquisition, eliminating version conflicts and retry.

**Rejected Because:**

- ADR-007 already rejected pessimistic locking for performance reasons
- Contradicts EventCore's optimistic concurrency design
- Serializes all access to locked streams (poor concurrency)
- Multi-stream lock acquisition risks deadlocks
- Long computations block other commands unnecessarily
- Violates EventCore's performance goals and event sourcing principles
- Retry is simpler and performs better under typical contention levels

### Alternative 5: Linear Backoff Instead of Exponential

Use fixed delay between retry attempts (e.g., always wait 50ms).

**Rejected Because:**

- Multiple conflicting commands retry at synchronized intervals → re-conflict immediately
- Doesn't reduce contention (all retries hit database simultaneously)
- Under high load, creates retry "waves" that amplify contention
- Exponential backoff spreads retries over time, reducing lock contention
- Linear backoff performs poorly in distributed systems literature
- No advantage over exponential approach (simpler but inferior behavior)

### Alternative 6: Separate Retry Infrastructure (Retry Queue/Service)

Failed commands enqueued to retry service instead of in-process retry.

**Rejected Because:**

- Massive complexity increase (additional infrastructure service, queue, coordination)
- Latency explosion (async retry instead of synchronous)
- User experience suffers (command doesn't complete in single request)
- Correlation complexity (how does caller know when retry succeeds?)
- Retry service becomes new failure point and scaling bottleneck
- Overkill for handling expected transient failures under normal operation
- In-process retry is simpler, faster, and sufficient for optimistic concurrency

### Alternative 7: No Metadata Preservation (New Correlation ID Per Retry)

Generate new correlation ID for each retry attempt.

**Rejected Because:**

- Fragments operation traces across multiple correlation IDs
- Distributed tracing tools cannot reconstruct complete operation flow
- Debugging becomes impossible (can't find all events from single operation)
- Violates tracing semantics (correlation groups logical operation, not attempts)
- Retry is implementation detail of single operation (should be transparent)
- ADR-005 establishes correlation as operation identifier (unchanged by infrastructure retries)

### Alternative 8: Retry at EventStore Trait Level Instead of Executor

Move retry logic into EventStore trait implementations instead of executor.

**Rejected Because:**

- EventStore responsible for storage operations, not command orchestration
- Cannot re-read streams and re-execute business logic from EventStore level
- Business logic (CommandLogic::handle) not accessible from EventStore
- State reconstruction happens at executor level, not storage level
- Mixing concerns (storage abstraction + retry orchestration) in single trait
- Different storage backends would implement retry differently (inconsistent behavior)
- Executor is natural orchestration point for multi-phase command execution

### Alternative 9: Configurable Retry via Command Trait Method

Commands implement optional retry_policy() method to customize behavior per command.

**Rejected Because:**

- Retry policy is infrastructure configuration, not business logic
- Forces every command to specify retry policy (or rely on defaults anyway)
- Same retry policy typically applies across all commands in application
- Complicates command implementations with infrastructure concerns
- Application-level configuration cleaner than per-command methods
- Retry tuning based on observation, not command-specific logic
- Adds boilerplate to every command for rarely-used customization

### Alternative 10: No Retry Limit (Infinite Retry)

Retry indefinitely until success, without max attempts limit.

**Rejected Because:**

- Under sustained contention or misconfiguration, commands never terminate
- Resource exhaustion (infinite retry consumes CPU, database connections)
- User experience suffers (timeouts without feedback)
- Debugging impossible (stuck commands don't surface errors)
- Timeout eventually forces termination anyway (better to fail explicitly)
- Provides no pressure to fix underlying contention issues
- Max retry limit enables graceful degradation and clear error reporting

## References

- ADR-001: Multi-Stream Atomicity Implementation Strategy (atomic multi-stream writes)
- ADR-002: Event Store Trait Design (read and append operations)
- ADR-003: Type System Patterns for Domain Safety (validated types)
- ADR-004: Error Handling Hierarchy (Retriable vs Permanent classification)
- ADR-005: Event Metadata Structure (correlation and causation IDs)
- ADR-006: Command Macro Design (CommandStreams and CommandLogic traits)
- ADR-007: Optimistic Concurrency Control Strategy (version conflicts as retriable)
- REQUIREMENTS_ANALYSIS.md: FR-3.3 Automatic Retry
- REQUIREMENTS_ANALYSIS.md: NFR-2.1 Minimal Boilerplate
- REQUIREMENTS_ANALYSIS.md: NFR-2.2 Compile-Time Safety
