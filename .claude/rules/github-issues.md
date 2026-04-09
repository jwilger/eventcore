# GitHub Issue Conventions

## Repository

`jwilger/eventcore`

## Issue Types

| Type        | Purpose                                                    |
| ----------- | ---------------------------------------------------------- |
| **Task**    | A specific piece of implementation or infrastructure work. |
| **Feature** | A request, idea, or new functionality.                     |
| **Bug**     | An unexpected problem or behavior.                         |
| **Meta**    | Repository process or engineering-foundation work.         |

## Labels

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

## Hierarchy and Relationships

### Parent/Child (Sub-Issues)

Use GitHub's native sub-issue feature for containment. Use the
`sub_issue_write` MCP tool (method: `add`) to create these links. The tool
requires the parent's issue number and the child's numeric issue ID (from the
`id` field in the creation response, not the issue number).

### Blocker Relationships

Use GitHub's native "blocked by" / "blocking" feature for sequencing
dependencies.

These are set via the GraphQL `addBlockedBy` mutation:

```bash
gh api graphql -f query='mutation {
  addBlockedBy(input: {
    issueId: "<blocked-issue-node-id>",
    blockingIssueId: "<blocking-issue-node-id>"
  }) { clientMutationId }
}'
```

Node IDs (the GraphQL `id` field, not the REST `id`) are required. Fetch them
with:

```bash
gh api graphql -f query='{
  repository(owner: "jwilger", name: "eventcore") {
    issue(number: 28) { id }
  }
}'
```

## Issue Assignment

When starting work on an issue, assign it to the current authenticated user.
Use `mcp__plugin_github_github__get_me` to get the current login, then
`mcp__plugin_github_github__issue_write` with `assignees: ["<login>"]` to
claim the issue.

Unassign when work is complete (the issue will be closed) or if work is
abandoned mid-session.

## Post-Merge Issue Hygiene

After a PR is merged, verify that all issues referenced in the PR body
(`Closes #N`, `Fixes #N`, etc.) were actually closed by GitHub. Do not rely on
GitHub's auto-close — it silently fails in some cases (large batches, race
conditions, etc.). Check each referenced issue's state and close any that
remain open.

When closing an issue causes **all** sub-issues of a parent issue to be closed,
ask the user whether the parent issue should also be closed.

## MCP Tools vs CLI

- Use `mcp__plugin_github_github__issue_write` to create and update issues
- Use `mcp__plugin_github_github__sub_issue_write` to link sub-issues
- Use `gh api graphql` (via Bash) for blocker relationships (no MCP tool exists
  for this)
- Use `mcp__plugin_github_github__list_issues` / `issue_read` to read issues
