# EventCore Agent Cheatsheet

1. Enter `nix develop` for pinned toolchains; start Postgres via `docker-compose up -d` only when persistence is required.
2. Format every change with `cargo fmt --all` before attempting a commit or PR.
3. Run `cargo clippy --all-targets --all-features -- -D warnings` to satisfy the lint gate.
4. Execute the full test suite with `cargo nextest run --workspace` (fallback: `cargo test --workspace`).
5. Target a single unit test via `cargo nextest run --workspace --test <binary> '<module::case>'` or `cargo nextest run --workspace --test <binary> -E 'test(<module::case>)'` (or `cargo test module::case`).
6. Target a single integration spec with `cargo nextest run --test I-NNN-*` or `cargo test --test I-NNN-feature_test.rs`.
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
20. Use Conventional Commits for all git commit messages and PR titles (type/scope: summary) so history stays machine-readable.
21. No repository-specific Cursor or Copilot rules exist—treat this file as the authoritative agent contract.

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
- ✅ Always use `--json` flag for programmatic bd commands
- ✅ Link discovered work with `discovered-from` dependencies
- ✅ Check `bd ready` before asking "what should I work on?"
- ✅ Store AI planning docs in `history/` directory
- ❌ Do NOT create markdown TODO lists
- ❌ Do NOT use external issue trackers
- ❌ Do NOT duplicate tracking systems
- ❌ Do NOT clutter repo root with planning documents

For more details, see README.md and QUICKSTART.md.

## Stacked PRs with git-spice

This project uses **git-spice** (`gs`) for managing stacked pull requests.

### Why Stacked PRs?

- Continue working on dependent changes without waiting for PR review/merge
- Break large features into reviewable chunks
- Each PR in a stack = one beads issue (use `discovered-from` links)

**Stacks are about code dependencies, not feature relationships.** Valid use cases:

- A single feature broken into reviewable parts
- **Unrelated tickets where later work depends on earlier code changes**
- Experimental work that builds on pending changes

### Quick Reference

**Initialize (first time per repo clone):**

```bash
gs repo init
gs auth    # Authenticate with GitHub
```

**Create a stack:**

```bash
gs branch create feature-part-1    # First branch in stack
# ... make changes, commit ...
gs branch create feature-part-2    # Stacks on part-1
# ... make changes, commit ...
```

**Submit stack as PRs:**

```bash
gs stack submit    # Creates/updates all PRs in stack
```

**After ANY PR merges (run regularly):**

```bash
gs repo sync --restack    # Sync + restack ALL branches in one command
gs stack submit           # Update remaining PRs
```

Or use the alias: `gs-sync` (syncs, restacks, and submits)

**Navigation:**

```bash
gs branch checkout <name>   # or: gs bco <name>
gs stack            # Show current stack
gs log              # Show stack with PR status
```

### Workflow Integration

1. **Create beads issue** for each PR in the stack
2. Use `discovered-from:<parent-id>` for dependent issues
3. **Submit stack**: `gs stack submit`
4. **Update PRs**: After changes, `gs stack submit` again
5. **After ANY merge**: `gs-sync` (or `gs repo sync --restack && gs stack submit`)
6. **Close beads issues** as PRs merge

### Shorthand Commands

| Full Command | Shorthand |
|--------------|-----------|
| `gs branch create` | `gs bc` |
| `gs branch checkout` | `gs bco` |
| `gs stack submit` | `gs ss` |
| `gs repo sync --restack` | `gs rs --restack` |
| Sync + restack + submit | `gs-sync` (alias) |
