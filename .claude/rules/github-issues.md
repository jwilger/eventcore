# GitHub Issue Conventions

## Repository

`slipstream-consulting/stochastic_macro`

## Issue Types

This org has custom issue types configured. Always set the `type` field when creating issues.

| Type         | Purpose                                                                                           |
| ------------ | ------------------------------------------------------------------------------------------------- |
| **Journey**  | Top-level workflow from the event model. One per blueprint workflow.                              |
| **Slice**    | A vertical pattern slice inside a journey (State Change, State View, Automation, or Translation). |
| **Scenario** | A concrete Given-When-Then scenario inside a slice.                                               |
| **Task**     | A specific piece of implementation or infrastructure work.                                        |
| **Feature**  | A request, idea, or new functionality not yet modeled.                                            |
| **Bug**      | An unexpected problem or behavior.                                                                |
| **Meta**     | Repository process or engineering-foundation work.                                                |

## Hierarchy and Relationships

### Parent/Child (Sub-Issues)

Use GitHub's native sub-issue feature for containment:

- **Journey → Slice**: every Slice is a sub-issue of its Journey
- **Slice → Scenario**: if individual scenarios are tracked, they are sub-issues of their Slice

Use the `sub_issue_write` MCP tool (method: `add`) to create these links. The tool requires the parent's issue number and the child's numeric issue ID (from the `id` field in the creation response, not the issue number).

### Blocker Relationships

Use GitHub's native "blocked by" / "blocking" feature for sequencing dependencies between slices.

These are set via the GraphQL `addBlockedBy` mutation:

```bash
gh api graphql -f query='mutation {
  addBlockedBy(input: {
    issueId: "<blocked-issue-node-id>",
    blockingIssueId: "<blocking-issue-node-id>"
  }) { clientMutationId }
}'
```

Node IDs (the GraphQL `id` field, not the REST `id`) are required. Fetch them with:

```bash
gh api graphql -f query='{
  repository(owner: "slipstream-consulting", name: "stochastic_macro") {
    issue(number: 28) { id }
  }
}'
```

### When to Add Blockers

Add a "blocked by" link when a slice cannot be implemented until another slice is complete — i.e., the Gherkin `Given` preconditions of one slice depend on events produced by another slice.

## Issue Assignment

When starting work on an issue, assign it to the current authenticated user.
Use `mcp__plugin_github_github__get_me` to get the current login, then
`mcp__plugin_github_github__issue_write` with `assignees: ["<login>"]` to
claim the issue. This signals to other sessions and collaborators that the
issue is actively being worked on.

Unassign when work is complete (the issue will be closed) or if work is
abandoned mid-session.

## Issue Body Conventions

- Reference the blueprint file and slice name: `blueprints/<workflow>.md → Pattern Slices → <slice name>`
- Include the Gherkin scenario in a fenced code block
- Include the "Completeness Focus" points from the blueprint
- For UI-facing slices, name the first-party screen
- Note implementation status if code already exists
- **Do not** duplicate parent/child or blocker relationships in the body text — those are tracked via GitHub's native link features

## Creating Issues from Blueprints

When creating issues for a new workflow blueprint:

1. Create the **Journey** issue first (type: Journey)
2. Create each **Slice** issue (type: Slice)
3. Wire sub-issue links (Slice → Journey parent)
4. Wire blocker links based on Given-precondition dependencies
5. Close any slices that are already implemented (state_reason: completed)

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
- Use `gh api graphql` (via Bash) for blocker relationships (no MCP tool exists for this)
- Use `mcp__plugin_github_github__list_issues` / `issue_read` to read issues
