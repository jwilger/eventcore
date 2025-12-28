# Project: EventCore

This project uses the marvin-sdlc methodology (TDD, Event Sourcing, ADR, Story Planning).

## Project Configuration

- **Mutation testing threshold**: 80% (default, can override)
- **Event model location**: _to be defined (e.g., under `docs/`)_
- **Architecture docs**: `docs/ARCHITECTURE.md`
- **ADRs**: `docs/adr/`

## Git-Spice Workflow (Stacked PRs)

When working on changes that depend on unmerged PRs:

1. **Use git-spice for stacking**: `gs branch create <name>` creates a branch stacked on the current one
2. **Each PR = one beads issue**: Use `discovered-from:<parent-id>` to link dependent issues
3. **Before starting work**: Run `gs repo sync --restack` to ensure branches are up to date
4. **After ANY PR merges**: Run `gs-sync` (alias for `gs repo sync --restack && gs stack submit`)

**Key commands:**
- `gs branch create <name>` - Create stacked branch
- `gs stack submit` - Create/update all PRs in stack
- `gs-sync` - Sync + restack + submit (run after merges)
- `gs log` - View stack with PR status

## Project-Specific Overrides

<!-- Add any project-specific conventions, tooling, or methodology customizations here -->
