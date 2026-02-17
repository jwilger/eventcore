---
title: Remove vestigial LocalCoordinator and CoordinatorGuard
status: open
priority: 1
issue-type: task
created-at: "2026-02-17T07:49:26.939770-08:00"
---

[migrated from bead eventcore-maz, type: task]

## Background

LocalCoordinator and CoordinatorGuard in `eventcore/src/projection.rs` are vestigial code that was never fully integrated:

1. **Never used**: `_coordinator` field has underscore prefix - Rust's way of saying "unused"
2. **`try_acquire()` never called**: ProjectionRunner::run() stores the coordinator but never calls it
3. **Ceremony without function**: Users must pass LocalCoordinator to ProjectionRunner::new() but it does nothing
4. **Orphaned test**: `local_coordinator_grants_leadership_without_contention` tests code nothing calls

## Context

- ADR-023 proposed ProjectorCoordinator trait for leader election
- ADR-026 superseded it: "Lost ProjectorCoordinator trait (but added little value)"
- ADR-026 approach: Direct advisory locks on dedicated connections, not trait abstraction
- The trait was never implemented, but LocalCoordinator skeleton remained

## Changes Required

### Code Removal (eventcore/src/projection.rs)

1. Remove `CoordinatorGuard` struct (lines 124-148)
2. Remove `LocalCoordinator` struct (lines 150-206)
3. Remove `_coordinator` field from `ProjectionRunner` struct
4. Simplify `ProjectionRunner::new(projector, store)` - remove coordinator parameter
5. Update `with_checkpoint_store()` to not copy `_coordinator`
6. Remove doc comments referencing coordinator

### Test Removal

1. Remove `local_coordinator_grants_leadership_without_contention` test
2. Update all tests that create LocalCoordinator - simplify ProjectionRunner construction

### Documentation Updates

1. Update ARCHITECTURE.md to remove ProjectorCoordinator trait references (lines 48, 51, 555, 659, 708, 718, 737)
2. Remove doc examples showing coordinator usage

## Acceptance Criteria

Scenario: Dead coordinator code is removed
  Given eventcore crate with vestigial LocalCoordinator
  When cleanup is complete
  Then no LocalCoordinator struct exists
  And no CoordinatorGuard struct exists
  And ProjectionRunner::new() takes only projector and store
  And all tests pass without coordinator boilerplate
  And cargo build --workspace succeeds with no warnings

Scenario: Documentation is updated
  Given ARCHITECTURE.md references ProjectorCoordinator trait
  When cleanup is complete
  Then no references to ProjectorCoordinator trait exist
  And doc comments do not reference coordinator pattern

## Future Work (NOT this ticket)

When distributed coordination is needed per ADR-026:
- Add advisory lock acquisition to PostgresCheckpointStore
- Use dedicated connections (not pooled) for projector lifetime
- No trait abstraction needed - direct implementation
