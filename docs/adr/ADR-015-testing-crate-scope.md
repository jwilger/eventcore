# ADR-015: eventcore-testing Crate Scope and Publication

## Status

accepted (2025-12-01)

## Supersedes

- ADR-013: EventStore Contract Testing Approach (packaging details only)

## Context

ADR-013 assumed the reusable EventStore contract test suite would live under `eventcore::testing::event_store_contract_tests` within the primary `eventcore` crate. While that naming aligned with the crate layout at the time, two new forces have emerged while shipping the chaos harness in this pull request:

1. **Production Dependency Footprint** – The core `eventcore` crate is pulled into application binaries, so adding test-only helper modules there brings along transitive dependencies (chrono, rand, tracing test hooks, etc.) that production code never uses.
2. **Crate Boundary Clarity** – Third-party EventStore implementations need an explicit test helper surface that they can add as a `dev-dependency` without also depending on the full command executor stack. Keeping helpers inside `eventcore` makes it difficult to express that intent in `Cargo.toml`.
3. **Publishing Cadence** – Contract tests, fixtures, and chaos tooling evolve at a faster cadence than the command executor. Bundling them into one crate forces synchronized releases even when only the helper APIs change.

These forces mean we need an explicit crate dedicated to testing surfaces so that production consumers continue to depend on a minimal artifact while backend authors can opt in to heavier helper utilities.

## Decision

1. **Introduce `eventcore-testing` Crate** – Ship a sibling crate whose Rust identifier is `eventcore_testing`. It exposes only test helper APIs and may carry dev-oriented dependencies that the main crate avoids.
2. **Move Existing and Planned Helpers** –
   - The newly implemented chaos harness lives under `eventcore_testing::chaos`.
   - Future modules referenced in ADR-013 (`event_store_contract_tests`, builders, fixtures, and assertion helpers) will also reside inside this crate when they are implemented.
3. **Update Documentation and Samples** – Guides should direct users to add `eventcore-testing` as a `dev-dependency` and import helpers via `eventcore_testing::<module>` rather than `eventcore::testing::<module>`.
4. **Versioning Discipline** – The crate ships under the same version number as `eventcore`, and release automation treats the pair as a lockstep bundle so consumers can rely on matching semver tags.
5. **Partial Supersession of ADR-013** – ADR-013 still governs the semantics of EventStore contract tests, but all packaging guidance in that ADR is superseded by this decision. Any future updates about helper module placement must reference ADR-015.

## Rationale

- **Cargo Semantics** – Rust users expect test helpers to be opt-in `dev-dependencies`. A separate crate aligns with that mental model and avoids leaking extra features into production builds.
- **Smaller Install Surface** – Keeping the main crate focused on command execution avoids unnecessary features, compile times, and binary size increases for adopters that never touch the helpers.
- **Clear Ownership** – The testing crate can evolve, gain new modules, and accept heavier dependencies without forcing unrelated API churn in `eventcore`.
- **Backwards Compatibility** – Splitting the helpers out means we can eventually deprecate any lingering `eventcore::testing::*` exports without a breaking change to the main crate.
- **Documentation Accuracy** – Explicitly naming the crate prevents ambiguous references in guides and ADRs, which previously led to inconsistencies.

## Consequences

### Positive

- Backend authors and integration tests gain a single crate to depend upon for helpers, chaos tooling, and future contract test harnesses.
- Production applications remain insulated from helper-only dependencies and compile-time costs.
- Release tooling can publish helper updates without touching the executor when no shared code changed.
- Documentation can give precise Cargo snippets (`eventcore-testing = { version = "x", dev-dependencies = true }`).

### Negative / Trade-offs

- Contributors must touch two crates (and keep their versions in sync) when helper APIs depend on executor changes.
- Downstream users must remember to add another dependency for tests, which increases cognitive overhead slightly.
- IDE searchability splits between crates, so cross-references require jumping between packages.

## Follow-Up Actions

1. Audit documentation to ensure references to `eventcore::testing::*` are migrated to `eventcore_testing::*` only after the corresponding helper modules exist in the new crate.
2. Track the implementation of the contract test harness described in ADR-013 as a follow-on work item inside the `eventcore-testing` crate.
3. Update release tooling (`release-plz`, changelog scripts) to treat `eventcore` and `eventcore-testing` as a paired publish set.
