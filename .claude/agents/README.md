# EventCore Agent Team

Custom agent definitions for coordinated multi-agent development.

## Team Model

The **main Claude Code session acts as team lead**. It orchestrates the
4-phase workflow, enforces TDD discipline, writes the shared interface
changes (types, traits, main lib), and dispatches parallel work to
teammates.

## Agents

| Agent                 | Model  | Isolation | Role                                                                   |
| --------------------- | ------ | --------- | ---------------------------------------------------------------------- |
| `domain-modeler`      | opus   | shared    | BDD scenarios, blueprints, API surface design (Phases 1-2)             |
| `backend-implementer` | sonnet | worktree  | Implements trait changes for one backend (postgres, sqlite, or memory) |
| `crate-implementer`   | sonnet | worktree  | Implements changes for a non-backend crate (testing, examples, macros) |
| `reviewer`            | sonnet | worktree  | Reviews changes against project rules (read-only)                      |

## Workspace Dependency Graph

Changes flow through the workspace in this order:

```
eventcore-types       ← shared vocabulary (traits, types)
eventcore-macros      ← derive macro codegen (depends on types)
eventcore             ← main lib: execute(), run_projection(), re-exports
     |
eventcore-testing     ← contract tests defining backend behavioral contract
     |
     ├── eventcore-postgres   ┐
     ├── eventcore-sqlite     ├─ parallel once contract tests exist and fail
     ├── eventcore-memory     │
     └── eventcore-examples   ┘
```

The lead handles the sequential top (types → macros → lib). Contract tests
come next — they define the failing tests that backend agents work against.
Only then are backends and examples dispatched in parallel.

## Usage

### Full Feature Development (Cross-Cutting)

For features that touch the shared interface and multiple downstream crates:

```
Create a team called "feature-name" and spawn:
- domain-modeler: handle Phases 1-2 (BDD scenarios, blueprints)
- reviewer: continuously review changes against project rules

Then after the shared interface is stable, spawn:
- testing-infra: crate-implementer for eventcore-testing (contract tests)

Then after contract tests exist and fail, spawn in parallel:
- backend-postgres: backend-implementer for eventcore-postgres
- backend-sqlite: backend-implementer for eventcore-sqlite
- backend-memory: backend-implementer for eventcore-memory
- examples: crate-implementer for eventcore-examples
```

### Workflow

1. **Phase 1-2**: Main session coordinates with domain-modeler for BDD
   scenarios and blueprint updates
2. **Phase 3**: Main session creates the technical plan
3. **Phase 4 — Shared interface**: Main session implements changes in
   eventcore-types → eventcore-macros → eventcore
4. **Phase 4 — Contract tests**: testing-infra agent writes the contract
   tests that define the new backend behavioral requirements (these must
   exist and fail before backends can work against them)
5. **Phase 4 — Parallel burst**: Main session dispatches backend and
   example work to 4 worktree-isolated agents
6. **Review**: Reviewer checks each agent's output against project rules
7. **Integration**: Main session merges worktrees, runs full test suite

### Partial Teams

Not every feature needs all agents:

- **Backend-only change** (e.g., postgres connection pooling): Single
  backend-implementer subagent, no team needed
- **New contract test**: crate-implementer for testing + 3 backend agents
  to verify their implementations pass
- **Macro change**: Main session handles macros, then dispatches backend
  and example agents to verify nothing broke
- **Documentation/blueprint only**: domain-modeler subagent, no team

## Communication Flow

```
       user
        |
    main session (lead) ──── domain-modeler (exits after Phase 2)
        |
    testing-infra          (contract tests — must complete before backends)
        |
     /  |   \   \
    /   |    \   \
  pg  sqlite  mem examples   (parallel, worktree-isolated)
    \   |    /   /
     \  |   /   /
      reviewer ───────────── messages agents directly about violations
```

## Notes

- All implementation agents use worktree isolation — they work on
  independent copies of the repo
- The reviewer is read-only — flags issues, doesn't fix them
- Domain-modeler exits after Phase 2; its output feeds Phase 3-4
- The lead handles the sequential dependency chain (types → macros → lib)
  before spawning parallel agents
- Teammates can message each other directly for tactical coordination
