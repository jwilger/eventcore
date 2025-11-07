# EventCore Agent Guidelines

## Build & Test Commands
- Format: `cargo fmt --all` (required before commit)
- Lint: `cargo clippy --all-targets --all-features` (warnings = errors via `lints.rust`)
- Test all: `cargo test --workspace` or `cargo nextest run --workspace`
- Test single: `cargo test <module>::<test_name>` (e.g., `cargo test single_stream::executes_successfully`)
- Setup: `docker-compose up -d` for Postgres; `nix develop` for reproducible toolchain

## Code Style
- **Rust 2024 edition**: 4-space indent, trailing commas, early returns over nesting
- **Naming**: `snake_case` files/modules, `PascalCase` types/traits, `SCREAMING_SNAKE_CASE` constants/macros
- **Imports**: Use `crate::` for internal, group `std` → external → internal, re-export public API via `lib.rs`
- **Types**: Prefer `nutype` for domain primitives, `thiserror` for errors, associated types over generics (ADR-012)
- **Error handling**: Return `Result<T, CommandError>` from command logic; use `?` operator; document error conditions
- **Traits**: Derive `serde`, `thiserror`, `Debug`, `Clone` where applicable; avoid manual impls

## Testing
- **Integration tests**: `tests/I-NNN-feature_name_test.rs` format (e.g., `I-001-single_stream-command_test.rs`)
- **Unit tests**: `#[cfg(test)] mod tests` within source files
- **Naming**: Descriptive async test names like `executes_successfully_with_valid_state`
- **Coverage**: Test happy path, business rule violations, concurrency conflicts, and error conditions

## Commits & PRs
- **Format**: `feat(I-123): description` or `fix(module): description` (Conventional Commits enforced by pre-commit)
- **Scope**: Reference issue IDs (I-NNN) or ADR numbers in commit scope
- **PRs**: Link issues/ADRs, list verification steps, update CHANGELOG/README for behavior changes

## Issue Tracking (Beads Required)

All planning and increment tracking is handled by the Beads issue tracker (`bd`). Do **not** create ad-hoc markdown TODO lists; they will rot and be ignored. Use the MCP slash commands or the CLI:

Core commands:

- List: `bd list` / `/beads:list`
- Show details: `bd show <issue-id>` / `/beads:show <issue-id>`
- Ready work: `bd ready` / `/beads:ready`
- Blocked: `bd blocked` / `/beads:blocked`
- Start work: `bd update <issue-id> --status in_progress`
- Close work: `bd close <issue-id> --reason "Implemented"` / `/beads:close <issue-id>`
- Create: `bd create "Title" -t feature -p 1` / `/beads:create Title`
- Add dependency: `bd dep add <a> <b> --type blocks` / `/beads:dep add <a> <b> --type blocks`

Session landing checklist:

1. Close or update status for every touched issue.
2. Create issues for any discovered follow-up and link with `discovered-from`.
3. Run `bd ready` and nominate the next issue (include ID + rationale).
4. (Optional) `bd stats` for summary; ensure `.beads/issues.jsonl` is committed and pushed.

Never hand-edit `.beads/issues.jsonl`; rely on the tooling. If Beads tools are unavailable, pause work and request restoration rather than inventing parallel tracking.
