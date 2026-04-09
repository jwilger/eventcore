# Outside-In TDD Execution Discipline

This rule governs how code is written during Phase 4 (Implementation). It is
not about plan structure — it is about what you do at the keyboard.

## The Rule

**Never write production code without a failing test demanding it.**

Every line of production code must be preceded by a test run that fails (or
errors) in a way that the production code is intended to fix. "I know I'll need
this" is not a valid reason to write code — the test must prove it.

## The Sequence

1. **Write the Gherkin acceptance scenario.** This is always the first artifact.
2. **Write step definitions** that exercise the system through its real
   boundaries (HTTP, CLI, etc.). Do not stub the system under test.
3. **Run the acceptance test.** It must fail or error — not skip. If it skips,
   the step definitions are not wired. Fix that first.
4. **Read the failure message.** It tells you exactly what is missing.
5. **Write only the minimum production code to change the failure message.**
   One compilation error fixed, one missing route added, one missing type
   defined — then run the test again.
6. **Repeat steps 4–5** until the acceptance scenario passes.
7. **Refactor** with all tests green.

## Acceptance Test Boundaries

Step 2 says "exercise the system through its real boundaries." The real
boundary is whatever the user interacts with:

- **UI features**: Playwright browser automation. The test opens a browser,
  navigates, fills forms, clicks buttons, and asserts on visible content.
- **CLI features**: CLI binary invocation with stdout/stderr/exit-code
  assertions.

Raw HTTP client calls to internal API endpoints (e.g., posting JSON to
`/_setup/*`) are NOT the user's real boundary for UI features. They are
integration tests, not acceptance tests. See
`.claude/rules/acceptance-test-boundaries.md` for the full policy.

## What "Minimum" Means

- If the test fails because a route returns 404, add the route with a stub
  handler. Run the test. Now it will fail for a different reason (wrong status,
  missing body, etc.) — and that new failure drives the next piece.
- If the handler won't compile because a type doesn't exist, define the type.
  Run the test.
- If the core logic function doesn't exist, create it with a `todo!()` body.
  Run the test. The panic tells you what to implement.

Do **not** define multiple types, multiple event variants, core logic, and shell
wiring in one batch. Each of those is a response to a distinct failure.

## Drill-Down to Unit Tests

When an acceptance test failure points at a specific piece of core logic (e.g.,
precondition validation), you may drill down to a unit test:

1. Write a failing unit test for the specific behavior.
2. Implement the minimum code to pass it.
3. Run the acceptance test again to see if it progresses.

This is the only sanctioned reason to write a unit test — it must be driven by
an acceptance-level failure, not by a desire to "cover" code.

## Violations

The following are violations of this rule:

- Writing domain types before a test demands them
- Writing multiple event variants before any test references them
- Writing core logic before the shell handler exists to call it
- Writing a shell handler, core logic, and domain types in one pass before
  running any test
- Creating files "because the plan says to" instead of because a test failed
- Using raw HTTP client calls as acceptance test step definitions for UI features
- Writing step definitions that exercise a path no real user could replicate

## Why

Outside-in TDD provides a feedback loop at every step. Batching production code
defeats that loop and produces speculative design. The test tells you what to
build; you do not tell the test what you already built.
