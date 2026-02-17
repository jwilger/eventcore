---
title: Replace into_inner() with into() for nutype domain types
status: open
priority: 3
issue-type: task
created-at: "2026-02-17T07:49:26.958250-08:00"
---

[migrated from bead eventcore-4i0, type: task]

**Background:** Currently we call `.into_inner()` on nutype domain types when we need the wrapped primitive value. This is verbose and non-idiomatic. Rust's `Into` trait is the standard way to perform type conversions.

**Changes:**
1. Find all locations where `.into_inner()` is called on nutype types
2. Implement `Into<T>` for each nutype domain type (BatchSize, StreamPosition, StreamVersion, etc.)
3. Replace `.into_inner()` calls with `.into()`
4. Ensure tests pass and code compiles

**Example:**
```rust
// Current (verbose)
let size: usize = batch_size.into_inner();

// After (idiomatic)
let size: usize = batch_size.into();
```

**Benefit:** More idiomatic Rust code that follows standard library conventions.

## Acceptance Criteria
Feature: Developer uses idiomatic Rust conversions for domain types

**Note:** This is a refactoring task focused on code quality. The acceptance criteria below describe the desired behavior, but do NOT require automated GWT-style acceptance tests. Success means:
- Existing test suite continues to pass
- Unit tests cover any new Into<T> trait implementations
- Code is more idiomatic

Scenario: Developer converts BatchSize to usize
  Given BatchSize wraps a usize value
  When developer needs the primitive value
  Then developer can call .into() instead of .into_inner()
  And type inference determines the target type

Scenario: Developer converts StreamPosition to u64
  Given StreamPosition wraps a u64 value
  When developer needs the primitive value for calculations
  Then developer can use .into() for conversion
  And code is more concise and idiomatic

Scenario: All into_inner() calls are replaced
  Given codebase has been refactored
  When developer searches for 'into_inner()'
  Then only legitimate uses remain (if any)
  And all nutype conversions use .into()

Scenario: Existing tests pass after refactoring
  Given Into<T> implementations are added
  And all into_inner() calls are replaced with .into()
  When cargo nextest run --workspace runs
  Then all existing tests pass unchanged
  And behavior is preserved
