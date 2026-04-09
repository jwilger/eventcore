# EventCore Agent Cheatsheet

## Project Configuration

- **Language:** Rust (2024 edition)
- **Test runner:** `cargo nextest run --workspace`
- **Build:** `cargo build --workspace`
- **Lint:** `cargo clippy --all-targets --all-features -- -D warnings`
- **Format:** `cargo fmt --all`
- **Mutation testing:** `cargo mutants` (threshold: 80%)
- **Architecture docs:** `docs/ARCHITECTURE.md`
- **ADRs:** `docs/adr/`

## Development Rules

1. Enter `nix develop` for pinned toolchains; start Postgres via `docker-compose up -d` only when running postgres backend tests.
2. Format every change with `cargo fmt --all` before attempting a commit or PR.
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
17. All work items are tracked in **GitHub Issues**; use GitHub MCP tools for automation.
18. Keep pre-commit hooks green: rerun fmt/clippy/nextest locally until clean before committing.
19. Use Conventional Commits for all git commit messages and PR titles (type/scope: summary) so history stays machine-readable.

## Issue Tracking with GitHub Issues

**IMPORTANT**: This project uses **GitHub Issues** for ALL issue tracking.

### Labels

**Priority labels:**

- `P0-critical` - Security, data loss, broken builds
- `P1-high` - Major features, important bugs
- `P2-medium` - Default priority
- `P3-low` - Polish, optimization
- `P4-backlog` - Future ideas

**Type labels:**

- `bug` - Something broken
- `enhancement` - New feature or request
- `task` - Work item (refactoring, tests, tooling)
- `epic` - Large feature with sub-issues
- `chore` - Maintenance (audits, cleanup)
- `research` - Investigation / spike
- `documentation` - Docs improvements

### Quick Reference

**Check for work:**

```bash
gh issue list --label "P1-high"     # High priority issues
gh issue list --assignee @me        # Your assigned issues
gh issue list --state open          # All open issues
```

**Create issues:**

```bash
gh issue create --title "Issue title" --label "enhancement" --label "P2-medium"
```

**Claim and update:**

```bash
gh issue edit 42 --add-assignee @me
gh issue comment 42 --body "Starting work on this"
```

**Complete work:**

```bash
gh issue close 42 --comment "Completed in #PR_NUMBER"
```

## Git Workflow

This project uses standard feature branches with squash merges.

### Branch Workflow

1. Create a feature branch: `git checkout -b type/description`
2. Make commits using Conventional Commits
3. Push and create a PR: `git push -u origin <branch>` then `gh pr create`
4. PRs are squash-merged into `main`
