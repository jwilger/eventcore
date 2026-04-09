# No Dead Code Workarounds

When the compiler reports dead code, the code is dead. Remove it.

## The Rule

**Never write code to satisfy dead code warnings.** If a field, method, type,
or variant is unused, the correct response is to remove it — not to add fake
reads, `drop()` calls, `let _ =` bindings, or response fields that exist only
to suppress the warning.

## Applies To

- Struct fields that are written but never read
- Enum variants that are never constructed
- Methods that are only called from `#[cfg(test)]` (binary crates don't see
  test usage for dead_code analysis)
- Event fields that are persisted but not yet consumed by any projection
- Domain type inner fields (nutype wrappers) whose `into_inner()` is never
  called

## What To Do Instead

1. **Remove the dead code.** If nothing reads a field, remove the field.
2. **If a unit test is the only consumer**, either the unit test is testing
   structure (wrong) or the code needs a non-test consumer. If no non-test
   consumer exists yet, the code is premature — remove it and the test.
3. **If the code is needed by a future slice**, it will be added when that
   slice's test demands it. Do not pre-add fields because "the blueprint says
   they'll be needed."

## Event Sourcing Specifically

Event fields should only be added when a consumer (projection, handler
response, or other non-test code) reads them. The blueprint describes the
complete event model, but implementation is incremental — only add fields that
current tests demand.

When a projection slice (e.g., "View setup status") is implemented, its tests
will fail because the events lack fields. That failure drives adding the fields.

## Why

Workarounds to suppress dead code warnings create:

- Meaningless code that obscures intent
- Maintenance burden for code that exists only to satisfy the compiler
- False confidence that fields are "used" when they're only read by workarounds
- Violation of the outside-in TDD discipline (adding code without a test
  demanding it)
