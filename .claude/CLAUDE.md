# Claude Code Configuration

This project uses the **marvin-sdlc** methodology (TDD, Event Sourcing, ADR, Story Planning) via Claude Code plugins.

## Claude-Specific Overrides

<!-- Add any Claude Code-specific conventions or plugin configurations here -->

<!-- BEGIN SDLC MANAGED SECTION — do not edit manually -->
## SDLC Workflow Quick Reference (v1.2.1)

| Command | When to use |
|---------|-------------|
| `/sdlc:start` | Begin work — auto-detects project state |
| `/sdlc:work` | Start or continue a task |
| `/sdlc:complete` | Mark a task done |
| `/sdlc:pr` | Create PR with three-stage review |
| `/sdlc:review` | Address PR review comments |
| `/sdlc:model` | Domain discovery & workflow modeling |
| `/sdlc:decompose` | Create tasks from event model slices |
| `/sdlc:adr` | Record architecture decisions |
| `/sdlc:recall` | Check auto memory before starting work |
| `/sdlc:remember` | Save conventions/solutions to auto memory |

**Task management:** `dot new`, `dot ls`, `dot start`, `dot done`
**Git workflow:** git-spice (`gs bc`, `gs ss`, `gs-sync`)
<!-- END SDLC MANAGED SECTION -->
