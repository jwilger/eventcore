# EventCore Agent Guidelines

## Build & Test Commands

- Format: `cargo fmt --all` (required before commit)
- Lint: `cargo clippy --all-targets --all-features` (warnings = errors via `lints.rust`)
- Test all: `cargo nextest run --workspace` (default; fallback: `cargo test --workspace` if nextest isn't available)
- Test single: `cargo nextest run --workspace --test <name>` (preferred; fallback: `cargo test <module>::<test_name>`)
- Test single integration: `cargo nextest run --test <file>` (fallback: `cargo test --test <file>`)
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
- **Integration style**: Exercise EventCore strictly through public APIs (e.g., `execute`, real stores, domain types). No private state access, no bespoke hooks.
- **Narrative structure**: Organize integration scenarios with explicit Given / When / Then comments and short explanatory notes so the test reads like documentation.
- **Developer-realistic code**: Define commands and helpers the way library consumers would (struct literals, minimal traits). Duplication is fine if it mirrors real usage.
- **Red-first expectation**: Add integration tests before the feature exists and call out when they intentionally fail or don't compile yet.
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

## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Git-friendly: Auto-syncs to JSONL for version control
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Quick Start

**Check for ready work:**

```bash
bd ready --json
```

**Create new issues:**

```bash
bd create "Issue title" -t bug|feature|task -p 0-4 --json
bd create "Issue title" -p 1 --deps discovered-from:bd-123 --json
```

**Claim and update:**

```bash
bd update bd-42 --status in_progress --json
bd update bd-42 --priority 1 --json
```

**Complete work:**

```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task**: `bd update <id> --status in_progress`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`
6. **Commit together**: Always commit the `.beads/issues.jsonl` file together with the code changes so issue state stays in sync with code state

### Auto-Sync

bd automatically syncs with git:

- Exports to `.beads/issues.jsonl` after changes (5s debounce)
- Imports from JSONL when newer (e.g., after `git pull`)
- No manual export/import needed!

### MCP Server (Recommended)

If using Claude or MCP-compatible clients, install the beads MCP server:

```bash
pip install beads-mcp
```

Add to MCP config (e.g., `~/.config/claude/config.json`):

```json
{
  "beads": {
    "command": "beads-mcp",
    "args": []
  }
}
```

Then use `mcp__beads__*` functions instead of CLI commands.

### Managing AI-Generated Planning Documents

AI assistants often create planning and design documents during development:

- PLAN.md, IMPLEMENTATION.md, ARCHITECTURE.md
- DESIGN.md, CODEBASE_SUMMARY.md, INTEGRATION_PLAN.md
- TESTING_GUIDE.md, TECHNICAL_DESIGN.md, and similar files

**Best Practice: Use a dedicated directory for these ephemeral files**

**Recommended approach:**

- Create a `history/` directory in the project root
- Store ALL AI-generated planning/design docs in `history/`
- Keep the repository root clean and focused on permanent project files
- Only access `history/` when explicitly asked to review past planning

**Example .gitignore entry (optional):**

```
# AI planning documents (ephemeral)
history/
```

**Benefits:**

- ✅ Clean repository root
- ✅ Clear separation between ephemeral and permanent documentation
- ✅ Easy to exclude from version control if desired
- ✅ Preserves planning history for archeological research
- ✅ Reduces noise when browsing the project

### Important Rules

- ✅ Use bd for ALL task tracking
- ✅ Always use `--json` flag for programmatic use
- ✅ Link discovered work with `discovered-from` dependencies
- ✅ Check `bd ready` before asking "what should I work on?"
- ✅ Store AI planning docs in `history/` directory
- ❌ Do NOT create markdown TODO lists
- ❌ Do NOT use external issue trackers
- ❌ Do NOT duplicate tracking systems
- ❌ Do NOT clutter repo root with planning documents

For more details, see README.md and QUICKSTART.md.
