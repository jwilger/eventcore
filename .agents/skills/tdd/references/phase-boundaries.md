# Phase Boundary Rules -- File-Type Restrictions

Each TDD phase may edit only specific file types. Violations indicate
drift from the cycle discipline. These rules apply in every mode (guided,
chaining, serial subagents, agent teams).

## RED Phase -- Test Files Only

**Allowed files:**

- Files in directories: `tests/`, `test/`, `__tests__/`, `spec/`
- Files matching: `*_test.*`, `*.test.*`, `*_spec.*`, `*.spec.*`, `test_*.*`

**Forbidden:** Production source files, type definition files, configuration
files, documentation.

**Verification checklist:**

1. Every file edited has a path or name matching a test pattern above.
2. No `src/`, `lib/`, or `app/` files were modified.
3. No type definition files (`.d.ts`, trait files, interface modules) were
   created or changed.
4. The test was run and failure output was pasted.

## GREEN Phase -- Production Implementation Only

**Allowed files:**

- Implementation source files (typically in `src/`, `lib/`, `app/`)
- Filling in stub bodies created by the domain phase

**Forbidden:** Test files (all patterns listed under RED above),
type-definition-only files (adding new type signatures or traits).

**Verification checklist:**

1. No file matching a test pattern was modified.
2. No new type definitions (structs, enums, traits, interfaces) were
   introduced -- only existing stubs were filled in.
3. Changes address the exact failure message from the RED phase.
4. The test was run and passing output was pasted.

## DOMAIN Phase -- Type Definitions Only

**Allowed files:**

- Type definition files: `.d.ts`, trait definitions, interface modules,
  struct/enum declarations, type alias files
- Function signatures with stub bodies (`unimplemented!()`, `todo!()`,
  `raise NotImplementedError`, `pass`)

**Forbidden:** Test files (all patterns listed under RED), implementation
bodies containing real logic.

**Verification checklist:**

1. No test files were modified.
2. No function bodies contain real logic -- only stubs.
3. Type checker or compiler was run and output was pasted.
4. After domain-post-RED: tests compile but still fail at runtime.
5. After domain-post-GREEN: no new files created, only review performed.

## COMMIT Phase -- No File Edits

**Allowed:** `git add`, `git commit`, `git status`, `git diff --cached`.

**Forbidden:** Any file modifications whatsoever. Code is frozen.

**Verification checklist:**

1. `git diff` shows no unstaged changes after the commit.
2. `git diff --cached` (before commit) contains only files from the
   completed cycle.
3. The commit message references the GWT scenario under test.
4. The full test suite was run and all tests pass.

## Quick Reference Matrix

| Phase  | Test files | Type defs (stubs) | Production code | Git ops |
| ------ | ---------- | ----------------- | --------------- | ------- |
| RED    | Edit       | --                | --              | --      |
| DOMAIN | --         | Edit              | --              | --      |
| GREEN  | --         | --                | Edit            | --      |
| COMMIT | --         | --                | --              | Edit    |

`--` means the phase MUST NOT touch that file category.
