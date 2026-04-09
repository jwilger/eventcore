---
globs: crates/**/*.rs,Cargo.toml
---

# Rust Workspace Conventions

## Functional Core / Imperative Shell

- Business logic functions are pure: no IO, no side effects, no database calls
- The imperative shell dispatches IO via the trampoline pattern
- Core functions return `Step<AppEffect, AppResult, T>`
- The shell matches on `AppEffect` variants and executes IO

## Domain Types

- Every domain concept has a semantic named type (e.g., `CustomerId`, `LicenseId`)
- **No raw primitives in domain code** — `String`, `bool`, `u32`, `i64`, and
  all other primitive types appear only at IO boundaries. This includes struct
  fields, function parameters, and return types. If a `bool` represents a domain
  concept (e.g., "server has started"), wrap it in a semantic type.
- **nutype is required** for all domain newtype definitions. Do not write manual
  newtype boilerplate (struct + impl blocks). Use nutype's built-in validations
  where possible; write custom validations only when necessary. Custom
  validations must be covered by property tests.
- Parse at the boundary, never re-validate inside the domain
- **Use `Option` for optionality** — when a value may or may not be present,
  use `Option<T>` instead of sentinel values like zero counts, empty strings,
  or boolean flags. `Option::None` expresses absence; a count of zero hides it.
- **Follow current rules, not existing code** — some older crates (e.g.,
  `sm_licensing`) predate the nutype convention and use manual newtypes. Always
  follow the current rules when writing new code, regardless of what patterns
  exist in the repo.

## Event Sourcing

- All state mutations are events via `eventcore`
- Commands MUST implement the `eventcore::CommandLogic` trait — not standalone
  functions that mimic the pattern. See `eventcore-command-pattern.md` for details.
- The shell MUST use `eventcore::execute()` to run commands
- Read models use `Projector` with checkpoint-based projections
- Each project has its own SQLite database; cross-project data is in the central DB

## Testing

- Acceptance tests use `cucumber` crate with Playwright for UI
- Property tests via `proptest` for every validation function (not optional)
- Unit tests only when drill-down discipline requires narrower scope
- Never test internal structure — only behavior

## UI (sm_ui)

- Atomic Design: quarks → atoms → molecules → organisms → templates → pages
- Components are pure: accept typed inputs, return Maud markup deterministically
- No IO, no database calls, no session access inside sm_ui
- Every reusable component requires property tests
