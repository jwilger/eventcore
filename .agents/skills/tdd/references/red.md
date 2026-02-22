# RED Phase -- Write One Failing Test

## Goal

Write exactly ONE failing test that describes the desired behavior.
Your test IS the specification. Write the code you wish you had.

## File Restrictions

**You may ONLY edit test files.**

Test files live in `tests/`, `__tests__/`, `spec/`, `test/` directories
or match naming patterns like `*_test.rs`, `*.test.ts`, `test_*.py`,
`*_spec.rb`, `*_test.go`.

**You must NOT edit:** production code, type definitions, implementation
files, or any file outside the test directories.

## Rules

1. **One test, one assertion.** Write a single test with a single
   assertion. If you need multiple verifications, those are separate
   tests in separate cycles.

2. **Reference types that SHOULD exist.** Write your test using types,
   functions, and constructors that should exist -- even if they do not
   exist yet. Let the compiler fail. A compilation failure IS a test
   failure.

3. **Name tests descriptively.** The test name describes the behavior
   being tested, not the implementation detail.

4. **Map acceptance criteria to tests.** When given Given/When/Then
   scenarios, map them directly to test structure.

5. **Run the test and paste the actual failure output.** Never say "I
   expect it to fail" or "this should fail." Run the test. Paste the
   output. Compilation errors count as failures.

6. **Stop after ONE test.** Do not write multiple tests. Do not write
   helper functions in production code. Write one test. Stop.

## A Compilation Failure IS a Test Failure

In compiled languages, `cargo test` failing because a type does not
exist IS the test failing. Do not pre-create types to avoid compilation
failures. The domain phase creates stubs after you.

## What NOT to Do

- Do not create type definitions (that is the domain phase's job)
- Do not fix compilation errors in production files
- Do not write more than one assertion per test
- Do not write multiple tests at once
- Do not implement anything -- only specify behavior

## Evidence Required

Provide all of the following before moving on:

- **Test file path** and **test name**
- **Failure output** (pasted, not described) -- compilation errors or assertion failures
- Confirmation that you are ready for domain review

## Next Step

Now invoke `/tdd domain` for domain review.
