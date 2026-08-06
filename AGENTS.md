# EventCore Agent Cheatsheet

## Project Configuration

- **Language:** Rust (2024 edition)
- **Test runner:** `cargo nextest run --workspace`
- **Build:** `cargo build --workspace`
- **Lint:** `cargo clippy --all-targets --all-features -- -D warnings`
- **Format:** `cargo fmt --all`
- **Mutation testing:** `cargo mutants` (zero surviving mutants required)
- **Architecture docs:** `docs/manual/01-introduction/04-architecture.md`
- **ADRs:** `docs/adr/`

## Workflow Authority

- Treat `.development-system.toml` and the development-system plugin as the
  authoritative workflow configuration; do not duplicate its delivery rules
  here.
- When `[features].tiber = true`, use Tiber as the sole project task tracker.

## Development Rules

1. Enter `nix develop` for pinned toolchains; start Postgres via `docker-compose up -d` only when running postgres backend tests.
2. Format every change with `cargo fmt --all` before attempting a commit.
3. Run `cargo clippy --all-targets --all-features -- -D warnings` to satisfy the lint gate.
4. Execute the full test suite with `cargo nextest run --workspace` (fallback: `cargo test --workspace`).
5. Target a single test via `cargo nextest run --workspace -E 'test(module::case)'` or `cargo test module::case`.
6. Target a single integration test file via `cargo nextest run --test feature_name_test`.
7. Use Rust 2024 edition conventions: 4-space indent, trailing commas, and prefer early returns over nested branching.
8. Naming: snake_case modules/functions, PascalCase types/traits/enums, SCREAMING_SNAKE_CASE for consts/macros, descriptive async test names.
9. Import order: std -> external crates -> internal (prefixed with `crate::`); consolidate re-exports through `lib.rs`.
10. Types: lean on `nutype` for domain primitives, derive `Debug`, `Clone`, `serde`, and `thiserror`; reach for associated types ahead of generics.
11. Errors: use `thiserror` enums, return `Result<T, CommandError>` from command logic, propagate via `?`, and document failure cases.
12. Domain structs should validate invariants in constructors, own their data, and avoid lifetimes when cloning is cheap.
13. Unit tests live beside source in `#[cfg(test)] mod tests`; integration tests live in each crate's `tests/` directory, organized by feature.
14. Integration tests must read like docs — Given/When/Then comments, only public APIs, no private hooks or mocks of internals.
15. Duplication inside tests is acceptable when it mirrors how downstream users compose commands and stores.
16. Prefer existing tracing/logging helpers over ad-hoc `println!` debugging noise.
17. Keep pre-commit hooks green: rerun fmt/clippy/nextest locally until clean before committing.
18. Use Conventional Commits for commit messages so history stays machine-readable.
