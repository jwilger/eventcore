# EventCore Planning

**ALL planning, story tracking, and increment management is done in Beads.**

## Using Beads for Planning

**View all issues:**

```bash
bd list
```

**View specific issue details (design, acceptance criteria, dependencies):**

```bash
bd show <issue-id>
# Example: bd show eventcore-004
```

**Find ready work:**

```bash
bd ready
```

**View blocked items:**

```bash
bd blocked
```

**Update issue status:**

```bash
bd update <issue-id> --status <status>
```

**Close completed work:**

```bash
bd close <issue-id> --reason "Completed because..."
```

**View project statistics:**

```bash
bd stats
```

## For AI Assistants

- **DO NOT** maintain separate planning in Markdown files
- **DO** use Beads MCP tools (`/beads:*` commands) for all issue operations
- **DO** reference issue IDs (e.g., `eventcore-004`) in commit messages and documentation
- **DO** query Beads for current status before starting work on any increment

## Historical Note

This file previously contained detailed increment specifications through version 3.0 (2025-10-14). All that content has been migrated to Beads issues with full acceptance criteria in Gherkin format.

For any questions about stories, increments, tasks, or planning, consult Beads - it is the single source of truth.
