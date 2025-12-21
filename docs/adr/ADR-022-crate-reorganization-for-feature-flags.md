# ADR-022: Crate Reorganization for Feature Flag-Based Adapter Re-exports

## Status

accepted (2025-12-20)

## Context

EventCore currently requires users to add multiple dependencies when using storage adapters:

```toml
[dependencies]
eventcore = "0.1"
eventcore-postgres = "0.1"
```

This creates friction for developers who expect the ergonomic pattern common in the Rust ecosystem:

```toml
[dependencies]
eventcore = { version = "0.1", features = ["postgres"] }
```

However, implementing this pattern reveals a fundamental structural problem: **circular dependencies**.

**The Circular Dependency Problem:**

If we add a `postgres` feature flag to `eventcore` that re-exports `eventcore-postgres`:

1. `eventcore` would depend on `eventcore-postgres` (via the feature flag)
2. `eventcore-postgres` already depends on `eventcore` (for `EventStore` trait, `StreamId`, `Event`, etc.)

This creates an unresolvable cycle that Cargo cannot compile.

**Current Workspace Structure:**

```
eventcore/                    (workspace root AND package)
  Cargo.toml                 ([workspace] + [package] combined)
  src/                       (library source - EventStore trait, types, InMemoryEventStore)
  eventcore-macros/          (proc-macros, depends on nothing)
  eventcore-postgres/        (depends on eventcore for traits)
  eventcore-testing/         (depends on eventcore for types)
```

**Forces at Play:**

1. **Developer Ergonomics**: Single dependency with feature flags is the expected pattern (tokio, sqlx, reqwest all use this)
2. **Dependency Graph Integrity**: Cargo requires acyclic dependency graphs
3. **Backward Compatibility**: Existing `use eventcore::{EventStore, StreamId, ...}` must continue working
4. **Separation of Concerns**: Heavy database dependencies should remain isolated (per ADR-011)
5. **Testing Crate Independence**: `eventcore-testing` is a dev-dependency, not a runtime feature flag (per ADR-015)
6. **Future Extensibility**: Additional adapters (MySQL, SQLite, Redis) should follow the same pattern

## Decision

Reorganize the workspace to introduce an `eventcore-types` crate that holds shared traits and type definitions, allowing `eventcore` to depend on adapters via feature flags without circular dependencies.

**Target Structure:**

```
eventcore/                    (workspace root only, NOT a package)
  Cargo.toml                 ([workspace] only)
  eventcore/                 (main crate with implementation + feature flags)
    Cargo.toml
    src/lib.rs               (execute(), InMemoryEventStore, retry logic, re-exports)
  eventcore-types/           (shared traits and types)
    Cargo.toml
    src/                     (EventStore trait, Event trait, StreamId, etc.)
  eventcore-macros/          (all macros: derive(Command), require!)
  eventcore-postgres/        (now depends on eventcore-types, not eventcore)
  eventcore-testing/         (now depends on eventcore-types, not eventcore)
```

**What Goes Where:**

| `eventcore-types` | `eventcore` | `eventcore-macros` |
|-------------------|-------------|-------------------|
| `EventStore` trait | `execute()` function | `#[derive(Command)]` |
| `Event` trait | `InMemoryEventStore` | `require!` macro |
| `CommandLogic`, `CommandStreams` | Retry logic | |
| `StreamResolver`, `MetricsHook` | `RetryPolicy`, `BackoffStrategy` | |
| `StreamId`, `StreamVersion` | `ExecutionResponse`, `RetryContext` | |
| `StreamWrites`, `StreamWriteEntry` | | |
| `EventStreamReader`, `EventStreamSlice` | | |
| `EventStoreError`, `Operation` | | |
| `CommandError`, `NewEvents` | | |
| `StreamDeclarations`, `StreamDeclarationsError` | | |

**Key Design Points:**

1. **`eventcore-types` contains the shared vocabulary** - Traits and types that define the EventCore API. This is the minimal set needed by adapter implementations.

2. **`eventcore` remains the main crate** - Contains implementation logic (`execute()`, `InMemoryEventStore`, retry mechanics) plus re-exports from `eventcore-types`. Its `lib.rs` is essentially:
   ```rust
   pub use eventcore_types::*;

   #[cfg(feature = "macros")]
   pub use eventcore_macros::Command;

   #[cfg(feature = "postgres")]
   pub use eventcore_postgres as postgres;

   // Plus: execute(), InMemoryEventStore, RetryPolicy, etc.
   ```

3. **Adapter crates depend on `eventcore-types`** - This breaks the cycle:
   - `eventcore` -> `eventcore-types` (always)
   - `eventcore` -> `eventcore-postgres` (via feature flag)
   - `eventcore-postgres` -> `eventcore-types` (no cycle!)

4. **`eventcore-testing` remains a separate crate** - Per ADR-015, it is a dev-dependency, not a feature flag. Feature flags are for runtime dependencies.

5. **`eventcore-macros` contains all macros** - Both the `#[derive(Command)]` proc-macro and the `require!` declarative macro live here.

## Rationale

**Why not just keep separate crates?**

The current approach works but creates unnecessary friction:
- Users must discover and add multiple dependencies
- Documentation must explain which crates to combine
- Version synchronization is manual
- The pattern diverges from Rust ecosystem norms (tokio, sqlx, reqwest)

**Why `eventcore-types` rather than `eventcore-core`?**

A types-focused crate provides a cleaner semantic split:
- **Clear naming**: "types" signals "shared vocabulary" rather than "the real implementation"
- **Smaller scope**: Only traits and type definitions, not all library code
- **Natural boundary**: Types/traits vs implementation logic is a well-understood separation
- **Adapter-friendly**: Adapter authors know exactly what they need to depend on

The `eventcore` crate remains the "main" crate with implementation logic (`execute()`, `InMemoryEventStore`, retry mechanics), while `eventcore-types` provides the shared vocabulary that all crates in the ecosystem use.

**Why not feature flags on the current structure?**

Impossible due to the circular dependency. The `eventcore-postgres` crate needs types from `eventcore`, and adding a feature flag that depends on `eventcore-postgres` creates the cycle.

**Why not move PostgreSQL code into the main crate behind a feature flag?**

This violates ADR-011's principle: heavy dependencies (sqlx, postgres drivers, connection pooling) should not bloat the main crate. Users who never use PostgreSQL should not pay the compile-time cost.

**Why is `eventcore-testing` not a feature flag?**

Per ADR-015, testing utilities are dev-dependencies with their own transitive dependencies (rand, test harnesses). Feature flags should enable runtime capabilities, not test infrastructure. The `eventcore-testing` crate remains a separate `dev-dependency` that users explicitly opt into.

**Why move `require!` macro to `eventcore-macros`?**

Consolidating all macros in one crate simplifies the dependency graph and keeps macro-related code together. The `require!` macro has no dependencies on implementation details, making it a natural fit alongside `#[derive(Command)]`.

## Consequences

### Positive

- **Single-Dependency Ergonomics**: Users can enable PostgreSQL with a feature flag
- **Backward Compatible**: `use eventcore::{EventStore, StreamId}` works unchanged
- **Ecosystem Alignment**: Matches patterns from major Rust libraries
- **Clean Dependency Graph**: No circular dependencies, clear hierarchy
- **Extensible**: Future adapters (MySQL, SQLite) follow the same pattern
- **Consistent with ADR-011**: Heavy deps remain isolated in separate crates
- **Consistent with ADR-015**: Testing stays a dev-dependency, not a feature
- **Clear Semantics**: `eventcore-types` name clearly communicates its purpose
- **Adapter Author Friendly**: Implementing a new adapter only requires depending on `eventcore-types`

### Negative

- **Workspace Complexity**: Additional crate to maintain (`eventcore-types`)
- **Published Internal Crate**: `eventcore-types` must be published to crates.io; while not strictly "internal", users should prefer depending on `eventcore`
- **Version Coordination**: Multiple crates (`eventcore`, `eventcore-types`, `eventcore-postgres`) must stay version-aligned
- **Migration Effort**: One-time refactoring to move types and update dependencies
- **Macro Relocation**: Moving `require!` to `eventcore-macros` is a minor breaking change for users who disabled the `macros` feature

### Future Implications

- Additional storage adapters can be added as feature flags (`sqlite`, `mysql`, `redis`)
- The `eventcore-types` crate API must remain stable as all adapters depend on it
- Adapter authors may choose to depend directly on `eventcore-types` rather than the full `eventcore` crate
- New types needed by adapters must be added to `eventcore-types`, not individual adapter crates

## Alternatives Considered

### Alternative 1: Keep Current Structure (Status Quo)

Maintain separate crates with explicit dependencies.

**Why Rejected:**
- Poor developer ergonomics
- Diverges from Rust ecosystem patterns
- Requires documentation to explain crate relationships
- Users frequently confused about which crates to add

### Alternative 2: Move All Adapters into Main Crate with Feature Flags

Put PostgreSQL implementation directly in `eventcore` behind a feature flag.

**Why Rejected:**
- Violates ADR-011 (heavy dependencies in main crate)
- Increases compile time for all users
- Bloats main crate with infrastructure-specific code
- Makes the main crate responsible for all adapter maintenance

### Alternative 3: Workspace-Level Feature Flags

Use Cargo workspace features to coordinate feature propagation.

**Why Rejected:**
- Workspace-level features are not yet stable in Cargo
- Would still require solving the circular dependency
- Adds complexity without solving the core problem

### Alternative 4: Full Core Crate (eventcore-core)

Extract ALL library code into `eventcore-core`, making `eventcore` a pure facade that only re-exports.

**Why Rejected:**
- Moves too much code unnecessarily - `execute()`, `InMemoryEventStore`, retry logic don't need to be shared
- `eventcore-core` name suggests "the real library" which may confuse users
- Larger internal crate to maintain
- `eventcore-types` provides cleaner semantic boundary (types vs implementation)

### Alternative 5: Traits-Only Crate (eventcore-traits)

Extract only traits into a shared crate, keeping all type definitions in main crate.

**Why Rejected:**
- Too narrow - adapter crates need types (`StreamId`, `StreamWrites`) not just traits
- "traits" name is misleading since the crate would need to include types too
- `eventcore-types` (the chosen approach) is essentially this but with a more accurate name and scope

### Alternative 6: Use a Virtual Manifest at Root

Make workspace root a virtual manifest (no `[package]`) but keep `eventcore` as a subdirectory.

**Why Rejected:**
- This IS part of the chosen approach, so not rejected
- Listed here to clarify: the virtual manifest is combined with the `eventcore-types` extraction

## References

- ADR-011: In-Memory Event Store Crate Location (principle: heavy deps in separate crates)
- ADR-015: eventcore-testing Crate Scope (testing is dev-dependency, not feature)
- Rust API Guidelines on crate organization
- tokio, sqlx, reqwest as examples of feature-flag-based adapter patterns
