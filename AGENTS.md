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
18. All work items are tracked in **GitHub Issues**; use `gh issue` CLI for automation.
19. Keep pre-commit hooks green: rerun fmt/clippy/nextest locally until clean before invoking `/commit`.
20. Use Conventional Commits for all git commit messages and PR titles (type/scope: summary) so history stays machine-readable.
21. No repository-specific Cursor or Copilot rules exist—treat this file as the authoritative agent contract.

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

### Workflow for AI Agents

1. **Check open issues**: `gh issue list --state open`
2. **Claim your task**: Add yourself as assignee
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue
5. **Complete**: Close issue when PR merges

### Sub-Issues (Task Lists)

Use GitHub's task list syntax in epic issue bodies:

```markdown
## Sub-Issues

- [ ] #123 - First sub-task
- [ ] #124 - Second sub-task
- [x] #125 - Completed sub-task
```

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

- ✅ Use GitHub Issues for ALL task tracking
- ✅ Use appropriate labels for priority and type
- ✅ Link related issues in descriptions
- ✅ Check `gh issue list` before asking "what should I work on?"
- ✅ Store AI planning docs in `history/` directory
- ❌ Do NOT create markdown TODO lists in code
- ❌ Do NOT duplicate tracking systems
- ❌ Do NOT clutter repo root with planning documents

## Stacked PRs with git-spice

This project uses **git-spice** (`gs`) for managing stacked pull requests.

### Why Stacked PRs?

- Continue working on dependent changes without waiting for PR review/merge
- Break large features into reviewable chunks
- Each PR in a stack = one GitHub issue

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

1. **Create GitHub issue** for each PR in the stack
2. Link issues in PR descriptions
3. **Submit stack**: `gs stack submit`
4. **Update PRs**: After changes, `gs stack submit` again
5. **After ANY merge**: `gs-sync` (or `gs repo sync --restack && gs stack submit`)
6. **Close GitHub issues** as PRs merge

### Shorthand Commands

| Full Command | Shorthand |
|--------------|-----------|
| `gs branch create` | `gs bc` |
| `gs branch checkout` | `gs bco` |
| `gs stack submit` | `gs ss` |
| `gs repo sync --restack` | `gs rs --restack` |
| Sync + restack + submit | `gs-sync` (alias) |
