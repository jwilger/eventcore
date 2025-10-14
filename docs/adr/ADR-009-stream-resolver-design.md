# ADR-009: Stream Resolver Design for Dynamic Discovery

## Status

accepted

## Context

EventCore's command macro (ADR-006) enables static stream declaration via `#[stream]` field attributes, providing compile-time safety for known stream dependencies. However, real-world business logic often requires discovering additional streams at runtime based on the current state of the system. The payment processing example from FR-1.3 illustrates this: a payment command knows its order stream statically but must discover related account or payment method streams by examining order state.

**Key Forces:**

1. **Static Stream Limitations**: Not all stream dependencies are known at command definition time - some depend on runtime state
2. **Atomicity Preservation**: Dynamically discovered streams must participate in ADR-001's atomic multi-stream writes
3. **Version Control Integration**: Discovered streams need optimistic concurrency control (ADR-007) like static streams
4. **Executor Orchestration**: Dynamic discovery requires executor re-execution from ADR-008 to ensure fresh state
5. **Type Safety Maintenance**: Dynamic discovery operates at runtime but must maintain type safety from ADR-003
6. **Performance Impact**: Discovery and re-execution add latency - mechanism must be efficient for common cases
7. **Event Immutability**: Once written, events never change - enabling optimizations in multi-pass discovery
8. **Developer Ergonomics**: Dynamic discovery API should be intuitive and integrate naturally with static stream declarations
9. **Compile-Time vs Runtime Trade-off**: Static streams provide compile-time safety; dynamic streams sacrifice this for flexibility

**Why This Decision Now:**

Dynamic stream discovery is required by FR-1.3 and represents the final piece of EventCore's multi-stream command execution model. This decision completes the command execution architecture by defining how commands transition from static declarations to runtime discovery and how the executor handles discovered streams efficiently. It must be defined before implementing the executor to ensure proper integration with the command execution flow established in ADR-008.

## Decision

EventCore will provide a StreamResolver trait for runtime stream discovery that integrates with static stream declarations and executor orchestration:

**1. StreamResolver Trait**

Commands that need dynamic discovery implement the optional StreamResolver trait:

```rust
// Trait definition (conceptual - shows WHAT, not HOW)
trait StreamResolver {
    fn resolve_additional_streams(
        &self,
        static_state: &State
    ) -> Result<Vec<StreamId>, DiscoveryError>;
}
```

**Characteristics:**
- **Optional**: Commands without dynamic needs don't implement it
- **State-Based**: Receives reconstructed state from static streams to inform discovery
- **Fallible**: Returns Result allowing discovery logic to fail with descriptive errors
- **Pure Function**: Same state produces same discovered streams (deterministic)
- **Type-Safe IDs**: Returns validated StreamId types, not raw strings (ADR-003)

**2. Integration with Static Streams**

Dynamic discovery complements, never replaces, static declarations:

- Commands declare known streams via `#[stream]` attributes (ADR-006)
- Executor reads static streams first, reconstructs state
- If command implements StreamResolver, executor invokes it with reconstructed state
- Discovered streams added to working set
- Complete stream set (static + dynamic) used for remainder of execution

**3. Executor Re-Execution Protocol**

When streams are discovered, executor restarts from read phase (ADR-008) with incremental reading optimization:

**Initial Execution Phase:**
- Phase 1: Extract static stream IDs from command
- Phase 2: Read static streams from version 0, capture versions
- Phase 3: Reconstruct state from static streams only
- **Discovery Check**: If StreamResolver implemented, invoke with static state
- **If streams discovered**: Record discovered stream IDs, proceed to re-execution
- **If no streams discovered**: Continue to Phase 4 (business logic)

**Re-Execution Phase (after discovery):**
- Phase 2 (repeated with incremental reading):
  - **Already-read streams**: Read from (last captured version + 1) onward, append to state
  - **Newly-discovered streams**: Read from version 0, capture versions
  - All reads capture current stream versions for optimistic concurrency
- Phase 3 (repeated): Reconstruct state from ALL events (cumulative from all reads)
- **Discovery Check**: Invoke StreamResolver again with full state
- **If NEW streams discovered**: Record new stream IDs, return to Phase 2 again
- **If no NEW streams** (Set deduplication prevents duplicates): Continue to Phase 4 (business logic)
- Phase 4: Execute business logic with complete state
- Phase 5: Atomically write events with version checking for ALL streams (static + all discovered)

**Incremental Reading Rationale:**
- Events are immutable - already-read events remain valid and unchanged
- Reading from last position avoids re-reading large event histories
- Newly-discovered streams start from beginning (no prior context)
- Significant performance optimization for multi-pass discovery scenarios
- Maintains correctness while dramatically reducing I/O

**4. Atomicity Guarantee Extension**

Discovered streams participate fully in atomic writes:

- Version checking (ADR-007) applies to ALL streams (static + discovered)
- Conflict in ANY stream (static or discovered) triggers retry
- Retry re-executes from Phase 1, rediscovering streams with fresh state
- Atomicity from ADR-001 maintained across entire expanded stream set
- No partial writes possible even with dynamically determined boundaries

**5. Discovery Use Cases**

Dynamic discovery addresses specific patterns:

**Use Case 1 - Conditional Stream Dependencies:**
Payment processing where payment method determines related streams:
- Static stream: order stream
- Discovery: examine order, find payment method, discover account/wallet streams

**Use Case 2 - Many-to-Many Relationships:**
Order fulfillment requiring inventory streams for ordered items:
- Static stream: order stream
- Discovery: examine order items, discover inventory stream per SKU

**Use Case 3 - Multi-Tenant Stream Partitioning:**
Operations requiring tenant-specific streams based on context:
- Static stream: operation stream
- Discovery: determine tenant from operation context, discover tenant partition streams

**6. When NOT to Use Dynamic Discovery**

Static declaration preferred when possible:

- Stream dependencies known at command definition time (use `#[stream]` fields)
- All commands of same type need same streams (static is simpler)
- Performance critical paths (avoid discovery overhead)
- Compile-time safety desired over runtime flexibility

Dynamic discovery reserved for genuinely state-dependent stream requirements.

**7. Error Handling**

Discovery failures are permanent errors (no retry):

- DiscoveryError::InvalidState: static state insufficient to determine streams
- DiscoveryError::InvalidStreamId: discovered stream ID validation failed
- Business logic never executes if discovery fails (fail-fast)
- Clear error messages guide developers to fix discovery logic

**Note on Discovery Bugs**: If discovery logic has a bug that generates infinite unique streams, the Set-based deduplication won't prevent the issue. Such bugs should be caught during testing and fixed in the discovery implementation. The executor uses a `Set<StreamId>` which naturally prevents duplicate streams from being re-read.

## Rationale

**Why Optional Trait (Not Required Method):**

Most commands have fixed stream dependencies knowable at compile time:

- Dynamic discovery is uncommon compared to static declarations
- Requiring all commands to implement resolver adds boilerplate to common case
- Optional trait means simple commands stay simple (NFR-2.1 minimal boilerplate)
- Trait presence signals intent: "this command does runtime discovery"
- Compiler doesn't require implementation when not needed

Alternative (required method on all commands) forces trivial "return empty" implementations.

**Why Discovery Uses Static State (Not Full State Initially):**

Discovery must determine which additional streams to read before full state available:

- Static streams provide initial context for discovery decisions
- Full state requires reading all streams (chicken-and-egg without initial discovery)
- Static state typically contains identifiers or references needed for discovery
- Pattern: static streams identify entities, discovered streams provide related context
- Re-execution with full state allows multi-pass discovery if needed

Alternative (discover from empty state) provides no context for discovery logic.

**Why Re-Execute from Read Phase:**

Discovered streams contain events that affect state reconstruction:

- Business logic must see complete state including events from discovered streams
- Partial state reconstruction would produce incorrect decisions
- Clean execution model: discovery happens during state loading, not after
- Aligns with ADR-008's phase structure: read → apply → discover → read more → apply more
- State determinism maintained: same streams, same events, same state

Alternative (continue with partial state) produces incorrect business logic behavior.

**Why Incremental Stream Reading:**

Event immutability enables significant optimization:

- Events once written never change - already-read events remain valid
- Multi-pass discovery doesn't need to re-read entire stream histories
- Already-read streams: read from last position forward (incremental)
- Newly-discovered streams: read from beginning (full history needed)
- Dramatic I/O reduction for commands with many discovery passes
- Large event streams benefit most from incremental reading
- Maintains correctness while improving performance
- Natural fit with append-only event store model

Alternative (re-read all streams fully each pass) wastes I/O on unchanged events.

**Why Set-Based Stream Deduplication (Not Iteration Limits):**

Natural deduplication prevents unnecessary work:

- `Set<StreamId>` automatically prevents reading same stream twice
- Discovery returning already-known streams is no-op (set ignores duplicates)
- If discovery logic generates infinite unique streams, that's a programming bug
- Programming bugs should be caught in testing, not masked by arbitrary limits
- Iteration limits add complexity without solving actual problem
- Legitimate multi-pass discovery not artificially constrained
- Clean design: trust type system (Set) over runtime checks

Alternative (iteration limits) masks bugs rather than encouraging proper testing.

**Why Discovered Streams Participate in Atomicity:**

Consistency boundaries determined by complete stream set:

- Discovered streams are part of command's consistency boundary (ADR-001)
- Optimistic concurrency must protect ALL streams command depends on
- Partial version checking would allow lost updates on discovered streams
- Atomicity guarantee extends naturally to dynamically determined boundaries
- Business logic operates on full state; writes must be atomic across full state

Alternative (skip version checking for discovered streams) violates consistency guarantees.

**Why Retry Rediscovers Streams:**

Optimistic concurrency retry requires fresh state (ADR-008):

- Stream discovery based on state; stale state produces stale discovery
- Concurrent modifications might change which streams are relevant
- Re-reading static streams means re-executing discovery with fresh context
- Consistency: retry sees current state, not state from initial attempt
- Discovery logic deterministic: same state produces same streams

Alternative (cache discovered streams across retry) uses stale discovery decisions.

**Why Discovery Errors Don't Retry:**

Discovery failure indicates programming error, not transient condition:

- InvalidState: command design flaw (missing required static streams)
- InvalidStreamId: bug in discovery logic producing invalid IDs
- Unlike version conflicts, discovery errors won't resolve on retry
- ADR-004 classification: permanent errors, not retriable failures
- Immediate failure provides clear feedback to fix discovery implementation

Alternative (retry discovery failures) wastes attempts on non-transient errors.

**Why Discovery Complements Static Streams:**

Static and dynamic declarations serve different needs:

- Static streams: compile-time known, type-safe, zero runtime overhead
- Dynamic streams: runtime determined, flexible, pays discovery cost when needed
- Combining both leverages strengths: safety where possible, flexibility where needed
- Most commands purely static (no dynamic overhead)
- Dynamic commands start with static base (always have context for discovery)

Alternative (dynamic-only with empty static set) loses compile-time safety benefits.

**Trade-offs Accepted:**

- **Runtime Overhead**: Discovery adds latency via re-execution (acceptable for flexibility, mitigated by incremental reading)
- **Lost Compile-Time Safety**: Discovered streams not validated until runtime (necessary trade-off)
- **Complexity Increase**: Re-execution protocol more complex than static-only (simplified by Set-based deduplication)
- **Debugging Challenge**: Multi-pass discovery harder to trace than single pass (mitigated by logging)
- **Performance Variability**: Discovered stream count affects latency unpredictably (acceptable for state-dependent logic)
- **Bug Detection Reliance**: Infinite unique stream bugs must be caught in testing (proper testing essential)

These trade-offs are acceptable because:

- Dynamic discovery optional - simple cases avoid overhead entirely
- Flexibility essential for real-world business logic patterns
- Incremental reading dramatically reduces I/O overhead in multi-pass scenarios
- Set-based deduplication simpler than iteration limits
- Many commands are static-only, paying no dynamic overhead
- State-dependent stream requirements are common in practice (payment processing, fulfillment, etc.)
- Proper testing catches discovery bugs before production

## Consequences

**Positive:**

- **Flexible Stream Boundaries**: Consistency boundaries adapt to runtime state, not just compile-time declarations
- **Real-World Patterns Supported**: Payment processing, order fulfillment, multi-tenant scenarios handled naturally
- **Atomicity Preserved**: ADR-001 guarantees extend to dynamically discovered streams
- **Incremental Reading Optimization**: Re-execution reads only new events from already-read streams, dramatically reducing I/O
- **Natural Deduplication**: Set-based approach prevents duplicate stream reads without artificial limits
- **Fail-Fast Discovery**: Invalid discovery logic detected before business logic executes
- **Retry Consistency**: Retries rediscover with fresh state, maintaining correctness
- **Optional Overhead**: Static-only commands pay no discovery cost
- **Type-Safe Discovery**: StreamId validation ensures discovered streams are valid
- **Clean Integration**: Discovery integrates naturally into ADR-008's executor phases
- **Unbounded Legitimate Discovery**: No artificial limits on multi-pass discovery for complex scenarios

**Negative:**

- **Latency Increase**: Re-execution adds overhead for commands using discovery (mitigated by incremental reading)
- **Runtime-Only Validation**: Discovered streams not checked until execution (no compile-time safety)
- **Implementation Complexity**: Executor must coordinate re-execution protocol with incremental reading
- **Debugging Difficulty**: Multi-pass discovery creates non-linear execution flow
- **Potential for Misuse**: Developers might over-use dynamic discovery instead of static declarations
- **Learning Curve**: Dynamic discovery concept adds to EventCore learning requirements
- **Testing Requirement**: Discovery bugs generating infinite unique streams must be caught via testing
- **Performance Unpredictability**: Discovery cost varies with state and discovered stream count

**Enabled Future Decisions:**

- Discovery caching could optimize repeated executions with same state
- Observability metrics can track discovery patterns and pass counts
- Static analysis tools could detect common discovery anti-patterns
- Discovery hints could guide executor (e.g., "discover once" vs "multi-pass expected")
- Lazy stream loading could defer reading discovered streams until actually needed
- Discovery cost monitoring can identify optimization opportunities
- Testing utilities can inject discovery failures for robustness verification
- Incremental reading strategy could be refined based on access patterns

**Constrained Future Decisions:**

- Discovery must remain deterministic (same state produces same streams)
- Discovered streams must participate in atomicity and version checking
- Retry must rediscover streams (cannot cache across retry attempts)
- Discovery errors must remain permanent (non-retriable per ADR-004)
- StreamResolver trait signature must remain stable
- Re-execution protocol integrated into ADR-008 phases cannot be bypassed
- Incremental reading must preserve event immutability guarantees

## Alternatives Considered

### Alternative 1: No Dynamic Discovery - Static-Only Streams

Require all stream dependencies declared statically via `#[stream]` attributes.

**Rejected Because:**

- Cannot model genuinely state-dependent stream requirements (payment processing example)
- Forces workarounds: read unnecessary streams "just in case", or complex command hierarchies
- Violates real-world business patterns where streams determined by data
- Reduces EventCore applicability to constrained subset of use cases
- FR-1.3 explicitly requires dynamic discovery for flexibility
- Static-only approach too rigid for complex business domains
- Developers would need external coordination mechanisms (multiple commands, sagas)

### Alternative 2: Discover All Streams Upfront (Before Initial Read)

Provide command with empty state or metadata only, discover all streams before any reading.

**Rejected Because:**

- Many discovery patterns require examining stream state (need initial read)
- Payment example: must read order to discover payment streams
- Chicken-and-egg: discovery needs state, state needs streams
- Forces less natural command design (pass metadata separately from state)
- Static streams provide natural initial context for discovery
- Pattern would require synthetic "discovery context" separate from command state

### Alternative 3: No Re-Execution - Discover During Business Logic

Allow business logic to discover additional streams on-the-fly during handle() method.

**Rejected Because:**

- Business logic invoked after state reconstruction (too late to read more streams)
- Cannot reconstruct state from discovered streams (already past apply phase)
- Violates separation: discovery is infrastructure concern, handle() is domain logic
- Business logic would see partial state (missing events from discovered streams)
- Breaks ADR-008's clean phase structure
- State inconsistency: business logic operates on incomplete information

### Alternative 4: Full Stream Re-Reading on Discovery

Re-read all streams completely from version 0 on each discovery pass.

**Rejected Because:**

- Wastes I/O re-reading events that haven't changed (events are immutable)
- Performance degradation for commands with large event histories
- Multi-pass discovery becomes increasingly expensive with stream size
- Doesn't leverage fundamental event store property (append-only immutability)
- Incremental reading provides same correctness with dramatically better performance
- No correctness benefit over incremental reading (same events, same state)
- Inefficiency particularly severe for multi-tenant or high-volume streams

**Current approach (incremental reading) is superior**: Already-read events remain valid, only new events need reading.

### Alternative 5: Two-Phase Discovery (Declare Then Read)

Separate discovery from reading: first declare all needed streams, then executor reads them.

**Rejected Because:**

- Complicates API: separate declaration phase adds ceremony
- Same outcome as current approach but more complex protocol
- No benefit over discovering then re-executing read phase
- Forces developers to think about discovery as separate concern from state
- Current approach more natural: discover while loading state, continue loading
- Additional phase increases implementation complexity

### Alternative 6: Discovered Streams Skip Version Checking

Apply optimistic concurrency only to static streams, not discovered streams.

**Rejected Because:**

- Violates consistency guarantees - lost updates on discovered streams possible
- ADR-001 atomicity compromised: partial writes could occur
- Discovered streams part of command's consistency boundary (must be protected)
- Business logic operates on discovered stream state (version checking necessary)
- Inconsistent behavior: some streams protected, others not
- Defeats purpose of multi-stream atomicity

### Alternative 7: Discovery via Static Configuration File

External configuration file maps command types to potential streams for runtime selection.

**Rejected Because:**

- Configuration separated from command logic (maintainability issue)
- Cannot express state-dependent logic in static configuration
- Runtime selection logic still needed (doesn't eliminate dynamic aspect)
- Adds complexity: configuration loading, validation, synchronization
- Less type-safe than trait-based approach
- Configuration drift risk (file doesn't match command needs)
- Violates locality principle (related concerns should be together)

### Alternative 8: Pre-Compute All Possible Streams (Pessimistic)

Read all streams that might possibly be needed, filter during business logic.

**Rejected Because:**

- Wastes resources reading unnecessary streams in most cases
- Defeats optimization of reading only needed streams
- Many-to-many scenarios could require reading enormous stream sets
- Performance degradation: every command reads maximum possible streams
- Violates pay-for-what-you-use principle
- Lock contention amplified (lock all possible streams, not just needed ones)
- Cannot enumerate "all possible" streams in many scenarios

### Alternative 9: Discovery Returns State Directly (Not Stream IDs)

Instead of returning stream IDs, resolver provides pre-loaded state from discovered streams.

**Rejected Because:**

- Violates separation of concerns: resolver becomes state loader
- Cannot integrate with EventStore trait (resolver doesn't have access)
- Version capture for optimistic concurrency impossible (resolver loads outside executor)
- Atomicity coordination breaks down (who tracks versions?)
- Executor loses control over read consistency and version tracking
- Retry protocol unclear: how to reload discovered state?

### Alternative 10: Macro-Generated Discovery from Annotations

Extend `#[stream]` attributes to specify discovery rules declaratively.

**Rejected Because:**

- Discovery logic often complex (not expressible in simple attribute syntax)
- Pushes business logic into macro annotations (wrong separation)
- Macro becomes overly complex handling all discovery patterns
- State-dependent logic requires runtime code, not compile-time generation
- Less flexible than trait-based runtime approach
- Doesn't eliminate runtime discovery (just moves it to generated code)
- Over-complicates macro for niche feature

## References

- ADR-001: Multi-Stream Atomicity Implementation Strategy (atomicity guarantees across discovered streams)
- ADR-003: Type System Patterns for Domain Safety (StreamId validation for discovered streams)
- ADR-006: Command Macro Design (static stream declaration via #[stream] attributes, trait separation)
- ADR-007: Optimistic Concurrency Control Strategy (version checking for discovered streams)
- ADR-008: Command Executor and Retry Logic (re-execution protocol, phase structure)
- REQUIREMENTS_ANALYSIS.md: FR-1.3 Dynamic Stream Discovery
- REQUIREMENTS_ANALYSIS.md: FR-1.4 Atomic Multi-Stream Writes
- REQUIREMENTS_ANALYSIS.md: NFR-2.1 Minimal Boilerplate
- REQUIREMENTS_ANALYSIS.md: NFR-2.2 Compile-Time Safety
