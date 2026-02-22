# Claude Code Configuration

<!-- BEGIN MANAGED: bootstrap -->

## Skills-Based Workflow

This project uses skills from `jwilger/agent-skills` for TDD, domain modeling,
code review, and architecture decisions.

**TDD mode:** automated (subagents + agent teams available)

| Command                   | When to use                           |
| ------------------------- | ------------------------------------- |
| `/tdd`                    | Run a TDD cycle (automated or guided) |
| `/code-review`            | Three-stage code review               |
| `/architecture-decisions` | Record an architecture decision       |
| `/domain-modeling`        | Domain review and type design         |
| `/debugging-protocol`     | Systematic 4-phase debugging          |
| `/ticket-triage`          | Evaluate ticket readiness             |

**Git workflow:** standard branches (`git checkout -b`, `git push -u origin`)

<!-- END MANAGED: bootstrap -->
