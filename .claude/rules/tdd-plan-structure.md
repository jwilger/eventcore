# Implementation Plans Must Follow TDD Cycles

Implementation plans (Phase 3 and Phase 4) must be structured as iterative
red-green-refactor cycles, not as layer-by-layer construction.

## Wrong: Waterfall Layers

```
Step 1: Define all domain types
Step 2: Define all events
Step 3: Implement all command logic
Step 4: Write unit tests
Step 5: Write acceptance tests
```

This is not TDD. It batches implementation before tests and does not drive
design from failing tests.

## Right: Outside-In TDD Cycles

```
Step 1: Write failing acceptance scenario → run → see it fail
Step 2: Identify first compilation/runtime failure
Step 3: Write failing unit test for that specific need
Step 4: Write minimum code to pass the unit test
Step 5: Refactor
Step 6: Repeat steps 3-5 until the acceptance scenario passes
```

## How This Applies to Plans

- Plans describe the **acceptance scenarios** that will be written first
- Plans describe the **design decisions** and **module structure** for context
- Plans do **not** prescribe a sequence of "build X, then build Y, then test"
- The actual implementation sequence emerges from the TDD cycle: each failing
  test tells you what to build next
- Domain types, events, and command logic are created **as needed** to make
  failing tests pass, not as upfront bulk construction

## Why

Writing all types and logic before tests leads to:

- Speculative design that may not match what the tests actually need
- Missed opportunities for simpler implementations
- Code that exists without test coverage
- Loss of the feedback loop that TDD provides
