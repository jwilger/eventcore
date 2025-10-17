# ADR-011: In-Memory Event Store Crate Location

## Status

accepted (2025-10-17)

## Context

During I-001 implementation, EventCore requires an in-memory event store implementation for testing infrastructure. This raises a fundamental crate organization question: **should InMemoryEventStore be included in the main `eventcore` crate or distributed as a separate `eventcore-memory` crate?**

**Key Forces:**

1. **Developer Onboarding**: New developers need to test commands quickly without complex setup
2. **Dependency Weight**: Library consumers care about dependency size and compilation time
3. **Testing Infrastructure**: All EventCore users need testing capabilities, not just production storage
4. **Industry Patterns**: Successful libraries include testing utilities in main crate (e.g., tokio includes tokio-test)
5. **Storage Adapter Consistency**: PostgreSQL adapter is distributed separately - should in-memory follow same pattern?
6. **Zero-Dependency Testing**: Developers should write integration tests without external services
7. **IR-1.2 Specification Ambiguity**: REQUIREMENTS_ANALYSIS.md IR-1.2 states "SHALL be distributed as separate `eventcore-memory` crate" but doesn't explain WHY

**Current State:**

- IR-1.1 specifies PostgreSQL adapter SHALL be separate `eventcore-postgres` crate (production-only, heavyweight dependencies)
- IR-1.2 specifies in-memory adapter SHALL be separate `eventcore-memory` crate (but this requires reconsideration)
- NFR-4.1 emphasizes pluggable storage abstraction via EventStore trait
- NFR-5.1 mandates library SHALL provide in-memory event store for testing
- I-001 (current increment) includes InMemoryEventStore as foundational infrastructure

**Why This Decision Now:**

I-001 is EventCore's first increment, establishing the initial crate structure. The decision to include or separate InMemoryEventStore affects:

- Developer onboarding time (30-minute target from success criteria)
- Library dependency footprint
- Testing documentation and examples
- Future crate organization precedents

Making this decision now prevents crate reorganization churn and ensures I-001 delivers optimal developer experience.

## Decision

InMemoryEventStore SHALL be included in the main `eventcore` crate, NOT distributed as a separate `eventcore-memory` crate.

**Rationale:**

1. **Zero Heavyweight Dependencies**: InMemoryEventStore uses only `std::collections` (HashMap, BTreeMap) with no external crates
2. **Essential for All Users**: Every EventCore user needs testing infrastructure, making it universal infrastructure, not optional feature
3. **Better Developer Onboarding**: One `cargo add eventcore` provides complete working system (storage + execution), achieving 30-minute onboarding target
4. **Industry Precedent**: Successful Rust libraries include lightweight testing utilities in main crate (tokio::test, serde_json test helpers)
5. **Minimal Compilation Cost**: No additional dependencies means negligible impact on compile time or binary size
6. **Clear Contrast with PostgreSQL**: PostgreSQL separation justified by heavyweight dependencies (sqlx, connection pooling, etc.), not applicable here

**Contrast with PostgreSQL Adapter:**

PostgreSQL adapter DOES belong in separate `eventcore-postgres` crate because:

- **Heavy Dependencies**: sqlx, tokio-postgres, connection pooling libraries add significant compilation time and binary bloat
- **Production-Only Concern**: Development/testing doesn't require PostgreSQL
- **Optional Infrastructure**: Not all users deploy to PostgreSQL (some use other adapters)
- **Environmental Setup**: Requires external PostgreSQL instance, complex configuration

In-memory adapter has NONE of these characteristics.

**Supersedes IR-1.2:**

This decision supersedes REQUIREMENTS_ANALYSIS.md IR-1.2's specification that in-memory adapter be separate. IR-1.2 was written before implementation revealed the dependency profile difference between in-memory and PostgreSQL adapters.

## Consequences

**Positive:**

- **Faster Onboarding**: Developers get complete working system with single dependency
- **Zero External Dependencies**: `cargo test` works immediately without Docker/PostgreSQL setup
- **Simpler Documentation**: Examples show `use eventcore::InMemoryEventStore` without explaining separate crate
- **Reduced Friction**: No decision paralysis about which crates to install for getting started
- **Industry Alignment**: Matches patterns from tokio, serde, and other successful infrastructure libraries
- **Negligible Cost**: No dependency weight penalty, no compilation time impact
- **Testing Dogfooding**: EventCore's own test suite uses InMemoryEventStore, validating its completeness

**Negative:**

- **Slightly Larger Main Crate**: InMemoryEventStore code lives in eventcore crate (mitigated by small code size)
- **Deviation from IR-1.2**: Original requirements specified separate crate (justified by implementation insights)
- **No Opt-Out**: Cannot exclude in-memory store from dependencies (acceptable given zero dependency cost)

**Enabled Future Decisions:**

- Chaos testing utilities can extend InMemoryEventStore in main crate
- Property-based test helpers can build on in-memory infrastructure
- Documentation examples can show complete code with no external setup
- Tutorial can achieve "working command in 30 minutes" with single dependency
- Future storage adapters (Redis, etc.) will follow PostgreSQL pattern (separate crate for heavyweight dependencies)

**Constrained Future Decisions:**

- In-memory store must remain dependency-free to justify inclusion in main crate
- If in-memory store ever needs heavyweight dependencies, must be separated
- New testing utilities should be evaluated for main crate inclusion (if zero dependencies) or separate crate (if dependencies required)
- Cannot remove InMemoryEventStore from main crate without breaking change

## Alternatives Considered

### Alternative 1: Separate eventcore-memory Crate (as specified in IR-1.2)

Distribute InMemoryEventStore as separate `eventcore-memory` crate, parallel to `eventcore-postgres`.

**Rejected Because:**

- **Artificial Symmetry**: PostgreSQL separation justified by dependencies, not organizational preference
- **Developer Friction**: Requires two crate installations for basic usage (`cargo add eventcore eventcore-memory`)
- **Documentation Complexity**: All examples must explain crate relationship
- **No Benefit**: Provides zero dependency savings since in-memory has no dependencies
- **Industry Anti-Pattern**: Successful libraries include lightweight testing infrastructure in main crate
- **Onboarding Barrier**: 30-minute target harder to achieve with multi-crate setup

### Alternative 2: Feature Flag in Main Crate

Include InMemoryEventStore behind feature flag (`eventcore = { version = "0.1", features = ["memory"] }`).

**Rejected Because:**

- **Unnecessary Ceremony**: Feature flag implies optional heavy feature, but in-memory has zero dependencies
- **Opt-In Confusion**: Developers must discover and enable feature, adding friction
- **Testing Complexity**: Library's own tests would require feature flag, confusing contributors
- **No Weight Savings**: Feature flags justify excluding heavy dependencies, not applicable here
- **Default Feature Workaround**: Could make feature default, but then why have flag at all?

### Alternative 3: Examples-Only Distribution

Include InMemoryEventStore only in examples/tests, not in library public API.

**Rejected Because:**

- **Defeats NFR-5.1**: Requirements mandate library SHALL provide in-memory store, not just examples
- **Consumer Testing**: Application developers need InMemoryEventStore for their integration tests
- **Code Duplication**: Forces consumers to implement their own in-memory store or copy example code
- **Missed Value**: In-memory store is valuable library functionality, hiding it wastes potential

### Alternative 4: Workspace with Both Main and Memory Crates

Use Cargo workspace with `eventcore` and `eventcore-memory` as sibling crates.

**Rejected Because:**

- **Workspace Complexity**: Workspace appropriate for multiple production crates, overkill for lightweight testing utility
- **Dependency Management**: Must coordinate versions between crates
- **Release Overhead**: Two crates to publish, version, document
- **Consumer Confusion**: Which crate for getting started?
- **No Benefit**: All workspace downsides with zero dependency savings

### Alternative 5: Both In-Memory and PostgreSQL in Main Crate

Include both storage implementations in main `eventcore` crate.

**Rejected Because:**

- **Heavy Dependencies**: PostgreSQL adapter requires sqlx, tokio-postgres, etc. (significant dependency weight)
- **Unnecessary Bloat**: Many users don't need PostgreSQL (using different backends)
- **Compilation Time**: sqlx macro compilation is expensive, would slow all users
- **Environmental Coupling**: Would require PostgreSQL for development even when working on core library
- **Feature Flag Complexity**: Would need complex feature flags to opt out of PostgreSQL

### Alternative 6: Async Runtime-Specific In-Memory Crates

Separate crates for different async runtimes (`eventcore-memory-tokio`, `eventcore-memory-async-std`).

**Rejected Because:**

- **Premature Optimization**: In-memory store doesn't need async runtime for basic functionality
- **Fragmentation**: Multiple crates for same functionality based on runtime
- **Maintenance Burden**: Must maintain parallel implementations
- **Not Needed Yet**: Can use runtime-agnostic implementation with std::collections
- **Future Flexibility**: Can add async features later if needed without breaking changes

## References

- REQUIREMENTS_ANALYSIS.md: NFR-5.1 Test Infrastructure (library SHALL provide in-memory event store)
- REQUIREMENTS_ANALYSIS.md: IR-1.1 PostgreSQL Adapter (separate crate justified by heavy dependencies)
- REQUIREMENTS_ANALYSIS.md: IR-1.2 In-Memory Adapter (superseded by this ADR's decision)
- REQUIREMENTS_ANALYSIS.md: NFR-4.1 Pluggable Storage (EventStore trait enables multiple implementations)
- PLANNING.md: I-001 Success Criteria (30-minute onboarding target)
- ADR-002: Event Store Trait Design (defines abstraction that InMemoryEventStore implements)
- Industry Examples: tokio (includes tokio::test), serde_json (includes test utilities in main crate)
