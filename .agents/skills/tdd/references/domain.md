# DOMAIN Phase -- Domain Review and Type Definitions

The domain phase runs TWICE per cycle: once after RED and once after
GREEN. Its focus depends on which mode is active.

## File Restrictions

**You may ONLY edit type definition files.**

Type definition files contain structs, enums, traits, interfaces, type
aliases, and function signatures with stub bodies (`unimplemented!()`,
`todo!()`, `raise NotImplementedError`).

**You must NOT edit:** test files, implementation bodies with real logic,
or any file that is not a type definition.

---

## After RED -- Review Test and Create Types

### Review the Test

Check the test for:

- **Primitive obsession:** Does the test use raw `String`, `i32`, or
  similar where a domain-specific newtype should exist?
- **Naming:** Do type and variable names reflect domain language?
- **Boundary placement:** Is behavior being tested at the right layer?
- **Event types with runtime state:** If the test references event
  types, verify their fields contain domain facts only -- no file paths,
  hostnames, PIDs, or working directories.

### Pushback Protocol

You CAN push back to RED if the test violates domain principles.
Pushback rules:

- Pushback MUST include a concrete suggestion (e.g., "use `EmailAddress`
  instead of `String`")
- Bounded to ONE round -- RED incorporates the suggestion or disagrees
  with rationale
- If disagreement, the orchestrator decides. No further rounds.
- Accept whatever RED produces in the revision.

### Create Minimal Type Definitions

After review (or if no concerns):

1. Create type definitions for ALL types referenced by the test: core
   domain types, repository traits, infrastructure types, error types.
2. Use `unimplemented!()`, `todo!()`, or equivalent for function bodies.
   NEVER implement logic.
3. Run the type checker or compiler and paste the output.

### Done When

Tests COMPILE but still FAIL -- the failure is now an assertion failure
or `todo!()`/`unimplemented!()` panic, NOT a compilation error. The
failure mode has shifted from "missing types" to "missing implementation."

### Evidence Required

- Files created and types defined
- Compilation status (pasted output)
- Confirmation that tests compile but still fail at runtime

### Next Step

Now invoke `/tdd green` to implement the minimal code.

---

## After GREEN -- Review Implementation

### Review for Domain Violations

Check the implementation for:

- **Structural vs semantic types:** Using `NonEmptyString` where
  `OrderId` should exist.
- **Domain boundary violations:** Logic in the wrong layer.
- **Type system shortcuts:** Bypassing type safety for convenience.
- **Validation in wrong places:** Validation that belongs at the
  boundary leaking into the domain or vice versa.

### VETO POWER

You have VETO POWER over designs that violate domain modeling
principles. When you identify a violation:

1. State the violation clearly.
2. Propose the specific alternative.
3. Explain the impact of leaving it as-is.

Do NOT back down from valid domain concerns to avoid conflict. Max 2
rounds of debate, then escalate to the user.

### Done When

Types are clean, no domain violations found, and all tests still pass.

### Evidence Required

One of:

- **No violations:** "Reviewed -- no domain violations. Proceed to commit."
- **Violation found:** "DOMAIN CONCERN RAISED: [violation], [location],
  [proposed alternative], [rationale]"

### Next Step

Now invoke `/tdd commit` to commit the completed cycle.
