---
globs: "**/*.rs,Cargo.toml"
---

# Rust Workspace Conventions

## Workspace Structure

| Crate                | Purpose                                                            |
| -------------------- | ------------------------------------------------------------------ |
| `eventcore`          | Main library: `execute()`, `run_projection()`, re-exports          |
| `eventcore-types`    | Shared vocabulary: traits (`EventStore`, `CommandLogic`) and types |
| `eventcore-macros`   | `#[derive(Command)]`, `require!`, `emit!` macro implementations    |
| `eventcore-postgres` | PostgreSQL backend with ACID transactions and advisory locks       |
| `eventcore-sqlite`   | SQLite backend with optional SQLCipher encryption                  |
| `eventcore-memory`   | Zero-dependency in-memory store for tests and development          |
| `eventcore-testing`  | Contract tests, chaos harness, `EventCollector` for testing        |
| `eventcore-examples` | Integration tests demonstrating EventCore patterns                 |

## Domain Types

- Every domain concept has a semantic named type (e.g., `StreamId`, `StreamVersion`)
- **No raw primitives in domain code** — `String`, `bool`, `u32`, `i64`, and
  all other primitive types appear only at IO boundaries. This includes struct
  fields, function parameters, and return types.
- **nutype is required** for all domain newtype definitions. Do not write manual
  newtype boilerplate (struct + impl blocks). Use nutype's built-in validations
  where possible; write custom validations only when necessary. Custom
  validations must be covered by property tests.
- Parse at the boundary, never re-validate inside the domain.
- **Use `Option` for optionality** — when a value may or may not be present,
  use `Option<T>` instead of sentinel values like zero counts, empty strings,
  or boolean flags.

## Event Sourcing

- All state mutations are events via `eventcore`
- Commands MUST implement the `eventcore::CommandLogic` trait — not standalone
  functions that mimic the pattern. See `eventcore-command-pattern.md` for
  details.
- `execute()` is the canonical entry point for running commands
- Read models use `Projector` with checkpoint-based projections
- `EventStore` and `EventReader` traits define the backend contract

## Testing

- Integration tests live in each crate's `tests/` directory, organized by
  feature
- Contract tests in `eventcore-testing` verify backend implementations
- Property tests via `proptest` for every validation function (not optional)
- Unit tests only when drill-down discipline requires narrower scope
- Never test internal structure — only behavior
