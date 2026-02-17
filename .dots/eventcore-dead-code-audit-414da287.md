---
title: Dead code audit and strict workspace lint enforcement
status: open
priority: 2
issue-type: task
created-at: "2026-02-17T07:49:26.950934-08:00"
---

[migrated from bead eventcore-igt, type: task]

## Background

The discovery of vestigial LocalCoordinator code reveals a subtle gap in quality enforcement. The project already has strict linting:

- `#![forbid(dead_code)]` - strictest level, cannot be overridden
- `#![deny(clippy::allow_attributes)]` - prevents `#[allow(...)]`
- build.rs scans for `#[allow]` usage

**How did dead code slip through?**

The underscore prefix (`_coordinator`) is Rust's OFFICIAL escape hatch from dead_code warnings. When you write `_foo`, you're telling the compiler "I know this is unused, suppress the warning." This is legitimate for:
- Implementing traits that require certain fields for Drop behavior
- Placeholder bindings in pattern matching
- RAII guards where the value matters but is never read

But it was MISUSED for vestigial code that should have been removed.

## The Real Problem

1. **Underscore-prefixed fields bypass dead_code** - by design, but easily misused
2. **No periodic audit** - code rots, documentation drifts from reality
3. **No lint catches unused struct fields** - rustc and clippy don't have a lint for "struct field stored but never accessed"
4. **Documentation-code sync is manual** - ARCHITECTURE.md can claim traits exist that don't

## Audit Scope

### Phase 1: Underscore-Prefix Audit

Systematically review EVERY `_foo` pattern in the codebase:

```bash
rg "^\s*_\w+:" --type rust  # Underscore-prefixed struct fields
rg "\b_\w+\b" --type rust   # All underscore-prefixed identifiers
```

For each occurrence, document:
- Location
- Is it genuinely needed? (Drop, trait requirement, pattern binding)
- Or is it suppressing dead code that should be removed?

### Phase 2: Documentation-Code Sync

Verify every type/trait mentioned in docs exists:

1. **ARCHITECTURE.md** - Extract all type names, grep for their definitions
2. **ADR files** - Ensure superseded ADRs don't describe current code
3. **Doc comments** - All `ignore` examples should compile if un-ignored

### Phase 3: Lint Configuration Consolidation

Current state: Each crate has duplicate `#![forbid(...)]` and `#![deny(...)]` blocks.

Migrate to workspace-level configuration (Cargo 1.74+):

```toml
# Root Cargo.toml
[workspace.lints.rust]
dead_code = "forbid"
# ... all current forbids/denies

[workspace.lints.clippy]
allow_attributes = "deny"
# ... add clippy::pedantic baseline
```

Each crate inherits via:
```toml
[lints]
workspace = true
```

Benefits:
- Single source of truth
- Easier to audit and update
- No drift between crates

### Phase 4: Add Missing Lint Coverage

Consider adding:
- `clippy::unused_self` - methods that don't use self
- `clippy::unused_async` - async functions that don't await
- Custom clippy lint or CI script for underscore-prefix audit

## Discovered Items (populate during audit)

| Location | Pattern | Legit? | Resolution |
|----------|---------|--------|------------|
| eventcore/src/projection.rs:245 | `_coordinator` | NO | eventcore-maz |
| eventcore-postgres/tests/common/mod.rs:8 | `#![allow(dead_code)]` | REVIEW | Check if needed |
| docs/ARCHITECTURE.md:48 | ProjectorCoordinator trait | N/A | Trait doesn't exist |
| (add more during audit) | | | |

## Acceptance Criteria

Scenario: All underscore-prefixed items are justified
  Given comprehensive audit completed
  When each _foo pattern is reviewed
  Then each has documented justification in adjacent comment
  Or it is removed as vestigial

Scenario: Workspace lints are consolidated
  Given duplicate lint blocks in each crate
  When migration to [workspace.lints] complete
  Then root Cargo.toml contains single lint configuration
  And all crates use [lints] workspace = true
  And no crate-level forbid/deny/warn blocks exist

Scenario: Documentation matches reality
  Given ARCHITECTURE.md claims ProjectorCoordinator trait exists
  When audit runs
  Then discrepancy is flagged
  And documentation is updated to match code

Scenario: CI catches underscore-prefix abuse
  Given new code with _unused field that could be removed
  When PR review occurs
  Then reviewer is prompted to check underscore-prefix justification
