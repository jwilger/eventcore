# Outside-In TDD Execution Discipline

This rule governs how code is written during implementation. It is not about
plan structure — it is about what you do at the keyboard.

## The Rule

**Never write production code without a failing test demanding it.**

Every line of production code must be preceded by a test run that fails (or
errors) in a way that the production code is intended to fix. "I know I'll need
this" is not a valid reason to write code — the test must prove it.

## The Sequence

1. **Write a failing integration test** that exercises the public API. This is
   always the first artifact.
2. **The test calls the public API** — `execute()`, `run_projection()`, trait
   methods, or macro-generated code. Do not test internals directly at this
   level.
3. **Run the test.** It must fail or error — not skip. If it doesn't compile,
   that counts as a failure.
4. **Read the failure message.** It tells you exactly what is missing.
5. **Write only the minimum production code to change the failure message.**
   One compilation error fixed, one missing type defined, one trait method
   stubbed — then run the test again.
6. **Repeat steps 4–5** until the integration test passes.
7. **Refactor** with all tests green.

## What "Minimum" Means

- If the test fails because a type doesn't exist, define the type with minimal
  fields. Run the test. Now it will fail for a different reason — and that new
  failure drives the next piece.
- If a trait method doesn't exist, create it with a `todo!()` body. Run the
  test. The panic tells you what to implement.
- If `execute()` returns the wrong error, implement the specific validation.
  Run the test.

Do **not** define multiple types, multiple event variants, core logic, and trait
implementations in one batch. Each of those is a response to a distinct failure.

## Drill-Down to Unit Tests

When an integration test failure points at a specific piece of internal logic
(e.g., stream version conflict detection), you may drill down to a unit test:

1. Write a failing unit test for the specific behavior.
2. Implement the minimum code to pass it.
3. Run the integration test again to see if it progresses.

This is the only sanctioned reason to write a unit test — it must be driven by
an integration-level failure, not by a desire to "cover" code.

## Violations

The following are violations of this rule:

- Writing domain types before a test demands them
- Writing multiple event variants before any test references them
- Writing core logic before the public API entry point exists to call it
- Writing trait implementations, core logic, and types in one pass before
  running any test
- Creating files "because the plan says to" instead of because a test failed
- Writing unit tests that exercise internals without an integration-level
  failure driving the need

## Why

Outside-in TDD provides a feedback loop at every step. Batching production code
defeats that loop and produces speculative design. The test tells you what to
build; you do not tell the test what you already built.
