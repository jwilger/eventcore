# ADR-013: EventStore Contract Testing Approach

## Status

accepted (2025-10-29)

## Context

EventCore's `EventStore` trait defines the contract for pluggable storage backends. This trait is a **primary abstraction boundary** - different implementations provide storage using different technologies (in-memory, PostgreSQL, EventStoreDB, etc.).

**Critical Semantic Behavior:**

Version conflict detection is a **critical semantic contract** of the EventStore trait. When concurrent commands attempt to write to the same stream, the storage backend MUST detect version conflicts and return an error. This behavior is fundamental to EventCore's correctness guarantees (per ADR-001: Multi-Stream Atomicity and ADR-007: Optimistic Concurrency Control).

**The Question:**

How do we ensure that ALL EventStore implementations handle version conflicts correctly?

**Current State:**

During I-001 implementation, `InMemoryEventStore` was implemented with correct version checking behavior:

- Tracks stream versions internally
- Validates expected versions match actual versions before writing
- Returns `EventStoreError::VersionConflict` when versions don't match
- Integration test `concurrent_deposits_detect_version_conflict` verifies this behavior

However, there is **no compile-time enforcement** that future implementations will include version checking. The trait signature alone doesn't prevent an implementor from writing:

```rust
async fn append_events(&self, writes: StreamWrites) -> Result<EventStreamSlice, EventStoreError> {
    // Naive implementation - just writes events without checking versions!
    // This compiles but violates the contract.
    Ok(EventStreamSlice { /* ... */ })
}
```

**The Risk:**

Future backend implementors might:

- Forget to implement version checking
- Implement version checking incorrectly
- Not understand the contract from documentation alone
- Create subtle bugs that only manifest under concurrency

**Forces at Play:**

1. **Type-Driven Development Philosophy**: EventCore follows strict type-driven development (per ADR-003) - "make illegal states unrepresentable at compile time"
2. **Semantic vs Structural Contracts**: Version checking is runtime behavior (semantic), not structural constraint
3. **Backend Implementation Freedom**: Different backends use different mechanisms - PostgreSQL uses ACID transactions, InMemoryEventStore uses mutex-protected comparisons, EventStoreDB uses native optimistic concurrency
4. **Developer Experience**: Backend implementors need clear, unambiguous guidance
5. **Library Maintenance**: Multiple backend implementations expected (InMemoryEventStore exists, PostgreSQL planned in I-005, EventStoreDB potential future)
6. **Trust Boundaries**: Like standard Rust traits (Iterator, Clone), some contracts are documented rather than enforced

**Why This Decision Now:**

I-001 is complete with working `InMemoryEventStore`. I-005 will implement PostgreSQL backend. Before external implementors create custom backends, we must decide: **attempt type-system enforcement OR rely on integration test suite to verify contract compliance?**

This decision affects:

- API design complexity
- Backend implementation flexibility
- Testing infrastructure
- Documentation approach
- Long-term library extensibility

## Decision

EventCore will provide a **reusable contract test suite** for EventStore implementations rather than attempting type-system enforcement of version checking semantics.

**Contract Test Module:**

Create a public `eventcore::testing::event_store_contract_tests` module that exports:

1. **Contract Test Functions**: Reusable test functions that verify EventStore contract compliance
2. **Version Conflict Scenarios**: Tests specifically for version checking behavior under concurrent writes
3. **Multi-Stream Semantics**: Tests verifying atomic version checking across multiple streams
4. **Error Classification**: Tests verifying correct EventStoreError variants returned

**Backend Implementation Requirements:**

All EventStore implementations (in-tree or external) MUST:

1. **Run Contract Test Suite**: Include contract tests in their test suites
2. **Pass All Contract Tests**: Treat contract test failures as implementation bugs
3. **Document Test Compliance**: Reference contract test suite in backend documentation

**Documentation Enhancements:**

1. **EventStore Trait Documentation**: Explicitly document version checking requirements
2. **Implementation Guide**: Reference contract test suite as mandatory for new backends
3. **Example Integration**: Show how to use contract tests in backend test suites

## Rationale

**Why Integration Tests Over Type-System Enforcement:**

**1. Semantic Contracts Are Runtime Behavior**

Version checking is **semantic behavior** (what the code does at runtime), not **structural constraint** (what types are valid at compile-time):

- Type system enforces structural properties: "this function takes two integers"
- Runtime behavior enforces semantic properties: "this function correctly detects version conflicts"
- No type-level encoding can eliminate the trust boundary for runtime behavior
- Integration tests verify actual behavior, not just structural correctness

**2. Type System Cannot Eliminate Trust Boundaries**

Even with sophisticated type-state patterns, backend implementors must be trusted to implement semantics correctly:

- Type-state pattern enforces "you called verify_versions()" not "verify_versions() actually works"
- Proof tokens enforce "you have a token" not "token represents correct verification"
- Type system checks structure, integration tests check behavior
- Similar to standard Rust traits: Iterator must produce items in correct order (documented contract, not type-enforced)

**3. Backend Implementation Freedom**

Different backends use fundamentally different version checking mechanisms:

- **PostgreSQL**: UNIQUE constraints or SELECT FOR UPDATE within ACID transaction
- **InMemoryEventStore**: Mutex-protected integer comparison
- **EventStoreDB**: Native optimistic concurrency control primitives
- **Future Backends**: May use backend-specific approaches

Type-system enforcement would constrain implementation strategies, violating ADR-002's principle that atomicity is an implementation detail.

**4. Integration Tests Provide Stronger Verification**

Contract tests verify **actual behavior** under realistic scenarios:

- Concurrent writes from multiple commands (the real failure mode)
- Version mismatch detection across multiple streams
- Correct error variant returned (EventStoreError::VersionConflict)
- Atomic verification within transaction boundaries

Type-state patterns only verify structural correctness, not behavioral correctness.

**5. Precedent in Rust Ecosystem**

Standard Rust traits rely on documented contracts with test verification:

- **Iterator**: Must produce items in correct order (not type-enforced)
- **Clone**: Must produce equivalent value (not type-enforced)
- **PartialEq**: Must be transitive and symmetric (not type-enforced)
- **EventStore**: Must verify versions atomically (not type-enforced)

EventStore follows this proven pattern.

**6. Complexity Cost of Type-System Enforcement**

Type-state patterns add significant complexity:

- **Type-State Pattern**: Requires split traits with generic type parameters, ceremony around state transitions, double verification (type-level + runtime)
- **Proof Tokens**: Violates atomic version checking (creates race condition window), leaks implementation details, API complexity
- **Split Trait Methods**: Breaks atomic operation model, forces backends to expose internal verification steps

Contract tests provide verification with **zero API complexity cost**.

**Why Reusable Contract Test Suite:**

1. **Single Source of Truth**: Contract requirements defined once, reused everywhere
2. **Consistent Verification**: All backends tested against same scenarios
3. **Documentation Through Tests**: Tests explicitly demonstrate expected behavior
4. **Easy Adoption**: Backend implementors import test module, call contract test functions
5. **Iterative Improvement**: Contract tests can evolve as new edge cases discovered

## Consequences

**Positive:**

- **Zero API Complexity**: EventStore trait remains simple, no type-state ceremony
- **Implementation Freedom**: Backends choose appropriate version checking mechanism
- **Clear Contract Verification**: Tests explicitly demonstrate version checking requirements
- **Reusable Across Implementations**: In-tree and external backends use same test suite
- **Easy to Understand**: Integration tests are more intuitive than type-state patterns
- **No Performance Overhead**: No double-verification or type-state transitions
- **Proven Pattern**: Follows standard Rust trait contract model
- **Iterative Evolution**: Test suite can grow as new scenarios identified

**Negative:**

- **Requires Discipline**: Backend implementors must remember to run contract tests (not compile-time enforced)
- **Discovery Time**: Contract violations discovered at test time, not compile time
- **No IDE Assistance**: Type checker can't guide implementors toward correct implementation
- **Trust Required**: Must trust implementors to include contract tests in their test suites
- **External Implementations**: Cannot force external crates to use contract tests

**Mitigation Strategies:**

1. **Clear Documentation**: EventStore trait docs prominently reference contract test suite
2. **Implementation Guide**: Step-by-step guide shows contract test integration as first step
3. **Example Implementations**: InMemoryEventStore and PostgreSQL backend demonstrate pattern
4. **CI/CD Requirements**: EventCore's own backends MUST pass contract tests in CI
5. **Community Communication**: Announce contract test suite prominently in release notes and docs

**Enabled Future Decisions:**

- Contract test suite can expand with new scenarios (e.g., metadata preservation, concurrent multi-stream writes)
- Performance benchmarks can be added to test suite for regression detection
- Chaos testing utilities can be added for reliability verification
- Contract tests can verify subscription behavior when EventSubscription trait added
- Test suite can include helpers for setting up test scenarios

**Constrained Future Decisions:**

- Contract test functions must remain stable public API (semantic versioning applies)
- New contract requirements must have corresponding test functions
- Breaking changes to contract tests require major version bump
- Test suite must work with minimal dependencies (no heavy test frameworks)

## Alternatives Considered

### Alternative 1: Type-State Pattern with Generic Parameters

Use type-state pattern to encode version verification at compile time:

```rust
// Pseudocode concept (not actual implementation)
struct StreamWrites<State> { /* ... */ }
struct Unchecked;
struct Checked;

impl StreamWrites<Unchecked> {
    fn verify_versions(self) -> StreamWrites<Checked> { /* ... */ }
}

trait EventStore {
    async fn append_events(&self, writes: StreamWrites<Checked>) -> Result<...>;
}
```

**Rejected Because:**

1. **Doesn't Eliminate Trust Boundary**: Type system enforces "verify_versions() was called" not "verify_versions() implemented correctly"
2. **Double Verification**: Backends must verify versions at runtime anyway (within transaction), type-state adds ceremony without eliminating runtime check
3. **API Complexity**: Generic type parameters proliferate through codebase, confusing for users
4. **Implementation Constraints**: Forces specific verification flow, prevents backend-specific optimization
5. **No Behavioral Guarantee**: Type-state proves structure, not semantics - integration tests still required

### Alternative 2: Proof Token Pattern

Require backends to return a "version verified" proof token before allowing writes:

```rust
// Pseudocode concept
struct VersionProof { /* opaque */ }

trait EventStore {
    async fn verify_versions(&self, writes: &StreamWrites) -> Result<VersionProof>;
    async fn append_events(&self, writes: StreamWrites, proof: VersionProof) -> Result<...>;
}
```

**Rejected Because:**

1. **Race Condition Window**: Separating verification from append creates time-of-check-to-time-of-use vulnerability
2. **Violates Atomic Operation Model**: ADR-007 requires version check within atomic transaction, not separate operation
3. **Leaks Implementation Details**: Forces backends to expose internal verification steps
4. **API Complexity**: Extra method calls and proof management for every append
5. **No Correctness Improvement**: Backends still trusted to implement verify_versions() correctly

### Alternative 3: Split Trait with Separate Methods

Split EventStore into separate read, verify, and write methods:

```rust
// Pseudocode concept
trait EventStore {
    async fn read_streams(&self, streams: &[StreamId]) -> Result<Events>;
    async fn verify_versions(&self, expected: Versions) -> Result<()>;
    async fn write_events(&self, events: Events) -> Result<()>;
}
```

**Rejected Because:**

1. **Breaks Atomic Operation Model**: Multi-stream atomicity requires single append operation (ADR-001)
2. **Transaction Management Leakage**: Exposes implementation detail of how backends achieve atomicity
3. **Incorrect Semantics**: Version check must happen within write transaction, not before
4. **Backend Complexity**: Backends must manage transaction lifecycle across separate method calls
5. **Doesn't Enforce Correctness**: verify_versions() implementation still trusted

### Alternative 4: Document-Only Contract (No Test Suite)

Rely solely on EventStore trait documentation to communicate version checking requirements, no reusable test infrastructure.

**Rejected Because:**

1. **No Verification Mechanism**: Backend implementors have no way to verify compliance
2. **Documentation Often Overlooked**: Developers may skim trait docs and miss critical requirements
3. **Testing Inconsistency**: Each backend writes own version conflict tests (or forgets to)
4. **Missed Edge Cases**: Without shared test suite, backends may miss non-obvious scenarios
5. **Higher Defect Rate**: No systematic verification increases likelihood of bugs

### Alternative 5: Macro-Generated Implementations

Provide procedural macro that generates EventStore implementation, ensuring version checking included:

```rust
// Hypothetical concept
#[derive(EventStore)]
struct MyBackend { /* ... */ }
```

**Rejected Because:**

1. **Implementation Constraints**: Macro must assume specific internal structure, limiting flexibility
2. **Macro Complexity**: EventStore implementations are non-trivial, macro would be extremely complex
3. **Debugging Difficulty**: Generated code harder to debug and understand
4. **Backend Diversity**: Different backends have fundamentally different structures (SQL vs in-memory)
5. **Inflexibility**: Cannot accommodate backend-specific optimizations or approaches

### Alternative 6: Runtime Contract Wrapper

Provide wrapper type that dynamically validates EventStore contract at runtime:

```rust
// Hypothetical concept
struct ContractEnforcingEventStore<T: EventStore> {
    inner: T,
    // runtime validation logic
}
```

**Rejected Because:**

1. **Performance Overhead**: Double-checks every operation (once in wrapper, once in backend)
2. **Runtime Detection**: Finds bugs at runtime in production, not during testing
3. **Doesn't Prevent Bugs**: Wrapper can't "fix" incorrect backend implementation
4. **Complexity**: Requires sophisticated runtime validation logic
5. **Test Suite Better**: Contract tests find bugs during development without production overhead

## References

- ADR-001: Multi-Stream Atomicity Implementation Strategy
- ADR-002: Event Store Trait Design
- ADR-003: Type System Patterns for Domain Safety
- ADR-007: Optimistic Concurrency Control Strategy
- Story eventcore-1: Public testing::event_store_contract_tests module implementation
