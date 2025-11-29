# EventCore Agent Cheatsheet

1. Enter `nix develop` for pinned toolchains; start Postgres via `docker-compose up -d` only when persistence is required.
2. Format every change with `cargo fmt --all` before attempting a commit or PR.
3. Run `cargo clippy --all-targets --all-features -D warnings` to satisfy the lint gate.
4. Execute the full test suite with `cargo nextest run --workspace` (fallback: `cargo test --workspace`).
5. Target a single unit test via `cargo nextest run --workspace --test <binary> --filter <module::case>` or `cargo test module::case`.
6. Target a single integration spec with `cargo nextest run --test I-0NN-*` or `cargo test --test I-0NN-feature_test.rs`.
7. Use Rust 2024 edition conventions: 4-space indent, trailing commas, and prefer early returns over nested branching.
8. Naming: snake_case modules/functions, PascalCase types/traits/enums, SCREAMING_SNAKE_CASE for consts/macros, descriptive async test names.
9. Import order: std → external crates → internal (prefixed with `crate::`); consolidate re-exports through `lib.rs`.
10. Types: lean on `nutype` for domain primitives, derive `Debug`, `Clone`, `serde`, and `thiserror`; reach for associated types ahead of generics.
11. Errors: use `thiserror` enums, return `Result<T, CommandError>` from command logic, propagate via `?`, and document failure cases.
12. Domain structs should validate invariants in constructors, own their data, and avoid lifetimes when cloning is cheap.
13. Unit tests live beside source in `#[cfg(test)] mod tests`; integration stories live under `tests/I-NNN-*`.
14. Integration scenarios must read like docs—Given/When/Then comments, only public APIs, no private hooks or mocks of internals.
15. Duplication inside tests is acceptable when it mirrors how downstream users compose commands and stores.
16. Prefer existing tracing/logging helpers over ad-hoc `println!` debugging noise.
17. Follow ADR guidance (esp. ADR-012 on domain-first event traits) before adding new abstractions or APIs.
18. All work items flow through Beads (`bd ... --json`); commit `.beads/issues.jsonl` with code changes, never edit it manually.
19. Keep pre-commit hooks green: rerun fmt/clippy/nextest locally until clean before invoking `/commit`.
20. No repository-specific Cursor or Copilot rules exist—treat this file as the authoritative agent contract.
