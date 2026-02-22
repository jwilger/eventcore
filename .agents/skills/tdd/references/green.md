# GREEN Phase -- Make the Failing Test Pass

## Goal

Write the MINIMAL production code to make the current failing test pass.
Nothing more.

## File Restrictions

**You may ONLY edit production/implementation files.**

Production files live in `src/`, `lib/`, `app/` directories or are
otherwise clearly implementation code.

**You must NOT edit:** test files, type-only definition files (those
with only stubs created by the domain phase), or any file in `tests/`,
`__tests__/`, `spec/`, or `test/` directories.

## Rules

1. **Address ONLY the exact failure message.** Read the specific error
   from the test output. Ask: "What is the SMALLEST change that
   addresses THIS SPECIFIC message?" Make only that change.

2. **One small change at a time.** Make a single change. Run tests.
   Check the result. Repeat until the test passes.

3. **Run tests after EACH change.** Paste the output every time. Never
   claim tests pass without pasted evidence.

4. **Stop IMMEDIATELY when the test passes.** Do not add error handling,
   flexibility, or features not demanded by the test. Do not refactor.
   Do not anticipate future tests. Stop.

5. **Delete unused/dead code.** If your change makes any existing code
   unreachable or unnecessary, remove it.

6. **Fill stubs, do not redefine types.** You implement method bodies
   for types that the domain phase created. When you encounter
   `unimplemented!()` or `todo!()`, replace it with the simplest code
   that passes the test. If compilation fails because a type is
   undefined (not just unimplemented), stop -- the domain phase should
   have created it.

## What NOT to Do

- Do not touch test files (that is the RED phase's job)
- Do not add methods not called by tests
- Do not implement validation not required by the failing test
- Do not keep dead code
- Do not add imports or helpers before addressing the actual failure

## Evidence Required

Provide all of the following before moving on:

- **Files modified** and the **specific change** made
- **Test output** showing the test passes (pasted, not described)

## Next Step

Now invoke `/tdd domain` for post-implementation domain review.
