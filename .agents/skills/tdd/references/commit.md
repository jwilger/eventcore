# COMMIT Phase -- Atomic Commit for the Completed Cycle

## Goal

Create an atomic git commit that captures the completed RED-GREEN cycle.

## Prerequisites

Before committing, verify:

- [ ] **All tests pass.** Run the full test suite (not just the single
      test). Paste the output.
- [ ] **Domain review approved.** The post-GREEN domain review found no
      violations, or all raised concerns have been resolved.

If either prerequisite is not met, go back and resolve it before
committing.

## Rules

1. **One commit per RED-GREEN cycle.** Each cycle produces exactly one
   commit. Do not batch multiple cycles into a single commit.

2. **Commit message describes the behavior added.** Reference the
   Given/When/Then scenario being implemented. Example:

   ```
   Given a valid email, When creating a user, Then the user is created
   ```

3. **Include test output as evidence.** The test suite output confirms
   the commit captures a working state.

4. **Stage all changes from this cycle.** Include the test file, any
   type definitions created, and the implementation code. Verify nothing
   unrelated is staged.

## Hard Gate

**No new `/tdd red` may begin until this commit exists.**

This is non-negotiable. The commit is the checkpoint that proves the
cycle is complete. Starting a new test without committing the previous
cycle violates the TDD discipline and risks losing work.

## Refactoring

If refactoring is warranted after this commit, do it in a SEPARATE
commit. Never mix behavioral changes (the RED-GREEN cycle) with
structural changes (refactoring) in the same commit.

## Next Step

Cycle complete. Start the next cycle with `/tdd red`, or finish the
task if all scenarios are implemented.
