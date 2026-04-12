# Refactor Sequencing: Add Before Remove

When migrating functionality from one API surface to another, establish the
new reachability path before removing the old one.

## The Rule

**Do not remove old reachability until the new reachability is in place.**
Dead code that appears solely because the replacement path has not been built
yet signals an incomplete migration, not code to delete.

## The Sequence

1. **Add** the new way to reach the functionality (new parameters, config
   structs, free-function wrappers, re-exports through the intended API).
2. **Update tests** to exercise the functionality through the new path.
3. **Remove** the old way (reduce visibility, remove re-exports, delete the
   old entry point).
4. **Now** dead code analysis correctly identifies truly dead code — anything
   the new API does not reach is genuinely unused and safe to remove.

## The Anti-Pattern

1. Remove the old reachability (`pub` to `pub(crate)`, delete re-exports).
2. Compiler reports dead code (because no new path exists yet).
3. Follow the dead-code rule and delete the "dead" code.
4. Functionality is lost — the new path was never built.

The dead-code rule is correct in step 3: unused code should be removed. The
error is in step 1: removing the old path before the new path exists made
working code appear dead.

## Example

```rust
// WRONG: remove old path, then delete "dead" internals
// Step 1: make ProjectionRunner pub(crate)
// Step 2: compiler says PollMode is dead → delete PollMode
// Result: continuous polling is gone from the codebase

// RIGHT: add new path, then remove old path
// Step 1: add poll_mode parameter to run_projection()
// Step 2: update tests to use run_projection(projector, &backend, config)
// Step 3: make ProjectionRunner pub(crate)
// Step 4: compiler is silent — PollMode is still reached through run_projection
```

## Applies To

- Reducing public API surface (layered API cleanup)
- Moving types between crates
- Replacing direct struct construction with free functions
- Any refactor where "old way" and "new way" coexist temporarily

## Why

This is the same principle as outside-in TDD: you do not remove the old
implementation until the new one replaces it. The strangler fig pattern
applies to API evolution — wrap and replace before you remove.
