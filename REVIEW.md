# Code Review Guidelines

## Review Philosophy

Every commit must comply with the project's architectural invariants, testing strategy, and code quality rules. There are no "minor" violations. Every change is either fully compliant or requires an explicit human-approved exception with the formal annotation below.

## Violation Exception Protocol

ANY intentional violation of these rules requires ALL of the following in a source code comment at the violation site:

```
// EXCEPTION: [invariant or rule name]
// Approved by: [human name]
// Date: [YYYY-MM-DD]
// Reason: [why this violation is necessary]
// Revisit: [condition under which this should be revisited]
```

**The `Approved by` field requires real human approval.** Claude (or any AI agent)
must ask the human for explicit approval before writing an EXCEPTION annotation.
The human must confirm in conversation before the annotation is written. Autonomous
work authorization does not extend to fabricating human approvals — ever.

Violations without this annotation MUST be rejected. No implied exceptions. No "we'll fix it later" without the formal annotation.

## Architectural Invariants

Every invariant below is a hard rule. Verify each applicable item on every commit.

### 1. Event Sourcing Core

- [ ] Commands implement `CommandLogic` with pure `apply` (fold events into state) and `handle` (state to new events or error).
- [ ] Commands implement `CommandStreams` to declare consistency boundaries (typically via `#[derive(Command)]`).
- [ ] All events originate from a command's `handle()` method via `eventcore::execute()`. No direct event construction or store appending.
- [ ] `execute()` is the canonical entry point for running commands. No manual fold/append.
- [ ] Read models (projections) and write models (command state) are separate code paths, even when logic is currently identical.
- [ ] `CommandLogic::apply()` is not exposed as a public function for consumer-level reads.

### 2. Domain Types

- [ ] Raw primitives (`String`, `u32`, `bool`, etc.) do not appear in domain-level function signatures or struct fields.
- [ ] Every domain concept has a semantic named type defined with `nutype` (e.g., `StreamId`, `StreamVersion`).
- [ ] Domain types are parsed at the boundary and never re-validated downstream. A domain type value is proof of validity.
- [ ] `Option<T>` is used for optionality, not sentinel values (zero counts, empty strings).

### 3. Testing

- [ ] New behavior has a failing integration test BEFORE implementation begins.
- [ ] Tests fail for the expected reason before any implementation code is written.
- [ ] Drill-down to unit tests is proof-based: triggered by "this failing test does not isolate a single function where a minimal change will green it" — never by diagnostic difficulty.
- [ ] Only the minimum code needed to address the current failure is written before re-running the test.
- [ ] Tests describe behavior only. No assertions on internal data structures, private functions, or execution paths.
- [ ] Property tests (`proptest`) exist for EVERY custom validation function. This is not optional.
- [ ] Contract tests in `eventcore-testing` verify backend implementations against the `EventStore` behavioral contracts.
- [ ] Integration tests exercise the public API (`execute()`, `run_projection()`, trait methods, macro-generated code).

### 4. Error Handling

- [ ] All error types use `thiserror::Error` derive, not manual `Display` implementations.
- [ ] Command business rule errors are typed enums, not string literals.
- [ ] No `expect()`, `unwrap()`, or `panic!()` in production code (non-test, non-static-init).
- [ ] Error messages use kebab-case machine-readable identifiers.

### 5. API Design

- [ ] Command state fields are private with behavior exposed through methods.
- [ ] Public API is clean: types, traits, and functions that consumers need are re-exported through `lib.rs`.
- [ ] Borrows (`&T`) are used instead of `clone()` where the receiver does not need ownership.

### 6. Repository Conventions

- [ ] No user-specific or machine-specific absolute paths in repo-committed files.
- [ ] Conventional Commits format for all commit messages and PR titles.
- [ ] Dependencies managed via `cargo add`/`cargo remove`, not manual Cargo.toml edits.
- [ ] Pre-commit hooks pass (fmt, clippy, nextest).

## Code Quality Rules

### Rust Conventions

- No raw primitives crossing IO boundaries into domain types.
- All domain types use `nutype` with validation.
- Parse at the boundary, never re-validate inside domain.
- Commands implement `CommandLogic` with pure `apply` and `handle`.
- Read models use `Projector` with checkpoint-based projections.
- Prefer borrows over clones; prefer associated types over generics.

### Testing Requirements

- New behavior MUST have a failing integration test first.
- All existing tests MUST pass before adding new ones.
- Drill-down to unit tests only when proof-based narrowing requires it.
- Property tests for EVERY custom validation function (not optional, enforced).
- Tests MUST fail for the expected reason before implementation.
- Tests describe behavior, never internal structure.
- Only minimum code written per test cycle; rerun before writing more.

## What to Block

The review MUST reject commits that:

1. Bypass `execute()` to manually fold events or append to the store
2. Use raw primitives where domain types should exist
3. Re-validate domain types after construction
4. Skip testing requirements (no integration test, missing property tests for validation)
5. Add implementation code without a failing test first
6. Commit with failing tests
7. Share `CommandLogic::apply()` functions between write model (command execution) and read model (projections)
8. Expose command state fields as `pub` instead of providing behavior methods
9. Use `expect()`, `unwrap()`, or `panic!()` in production code (non-test, non-static-init)
10. Use string literals for command business rule errors instead of typed error enums
11. Use sentinel values (zero counts, empty strings) instead of `Option<T>` for optional data
12. Use `clone()` where a borrow (`&T`) would suffice
13. Violate any architectural invariant without a complete EXCEPTION annotation
14. Add dead code workarounds to suppress compiler warnings

## What to Allow

The review should NOT block:

- Documentation-only changes
- Configuration file changes (Cargo.toml, docker-compose.yml)
- Build script and CI modifications
- Properly annotated exceptions with the complete EXCEPTION protocol
- Refactoring that preserves all existing test behavior
