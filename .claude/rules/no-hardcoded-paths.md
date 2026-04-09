# No Hardcoded Paths in Repo-Committed Files

Files committed to the repository must not contain user-specific or
machine-specific absolute paths.

## What This Covers

- `.claude/rules/` files
- `CLAUDE.md` and `CLAUDE.local.md`
- `REVIEW.md`
- Any other checked-in configuration or documentation

## Examples

**Wrong:** `~/.claude/projects/-home-jwilger-projects-stochastic-macro/memory/`

**Right:** "the project's Claude Code memory directory" or "the per-project
memory files managed by Claude Code"

## Why

These files are shared via git. Paths that embed a username, home directory, or
machine-specific project-path encoding will break for any other contributor.
