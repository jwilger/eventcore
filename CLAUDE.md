# EventCore

`eventcore` is a type-safe event sourcing library for Rust implementing multi-stream atomic event sourcing with dynamic consistency boundaries. It is a Rust Cargo workspace with 9 published crates (8 library/support crates plus the published examples crate; internal demo, bench, and stress tooling crates are not published) providing the core library, derive macros, multiple backend implementations, examples, and a testing toolkit.

## Workspace Structure

- `eventcore` — Main library: `execute()`, `run_projection()`, public re-exports
- `eventcore-types` — Shared vocabulary: traits (`EventStore`, `CommandLogic`) and types
- `eventcore-macros` — `#[derive(Command)]` derive macro implementation (the `require!` macro is exported from the `eventcore` crate, not here)
- `eventcore-postgres` — PostgreSQL backend with ACID transactions and advisory locks
- `eventcore-sqlite` — SQLite backend with optional SQLCipher encryption
- `eventcore-fs` — File-based, git-mergeable event store adapter
- `eventcore-memory` — Zero-dependency in-memory store for tests and development
- `eventcore-testing` — Contract tests, chaos harness, `EventCollector` for testing
- `eventcore-examples` — Integration tests demonstrating EventCore patterns

## Development Commands

- `cargo build --workspace` — Build all crates
- `cargo nextest run --workspace` — Run all tests (fallback: `cargo test --workspace`)
- `cargo clippy --all-targets --all-features -- -D warnings` — Lint
- `cargo fmt --all` — Format
- `cargo mutants` — Mutation testing (zero surviving mutants required)

## Environment

- Enter `nix develop` for pinned toolchains
- Start Postgres via `docker-compose up -d` only when running postgres backend tests

## Documentation

- Architecture docs: `docs/manual/01-introduction/04-architecture.md`
- Decision history: `docs/adr/`
- System blueprints: `blueprints/` (when they exist)

## Required Development Workflow

**CRITICAL: This workflow is enforced, not advisory.**

Every new feature, behavior change, or significant bug fix MUST follow these phases in order. Skipping a phase requires EXPLICIT user confirmation — never infer intent to skip.

### Phase 1: Product Discovery

Use `the-visionary` agent or `bdd:bdd-collaboration` skill. Turn vague requirements into concrete, measurable product requirements. Output: clear user stories with acceptance criteria.

If the user's request is already a clear, specific requirement with acceptance criteria, confirm that Phase 1 can be skipped.

### Phase 2: Domain Discovery

Use BDD skills (`bdd:bdd-scenarios`, `bdd:bdd-principles`). Model the feature: identify the public API surface, command/event design, trait boundaries, and write integration test scenarios. Output: updated or new blueprint in `blueprints/`.

If the feature maps cleanly to an existing modeled API with existing test scenarios, confirm that Phase 2 can be skipped.

### Phase 3: Technical Planning

Use `technical-coordinator` agent. Break requirements into implementation tasks with dependencies and sequencing. For architectural decisions, use `system-architect` or `solution-architect` agents and record decisions as ADRs in `docs/adr/`.

If the implementation is straightforward and fits existing patterns, confirm that Phase 3 can be skipped.

### Phase 4: Implementation

Use `/core:develop` skill. Follow outside-in TDD:

1. Write a failing integration test that exercises the public API
2. Drill down via TDD (red → green → refactor) until the test passes
3. Every domain concept gets a semantic named type
4. **Self-review before commit**: Before committing, review your changes against REVIEW.md's "What to Block" list and all `.claude/rules/` files. Fix violations before committing.
5. Use `/core:review` for multi-agent code review before merging

### Phase Skipping Rules

- **Default: follow all phases in order**
- **To skip a phase:** Claude must explicitly state which phase would be skipped and why, then ask for confirmation
- **Never skip based on implied intent** — even if the user says "just implement X," Claude must note which phases are being skipped and get a "yes, skip" confirmation
- **Phase 4 (Implementation) can never be skipped**
- **Log the decision:** when a phase is skipped with confirmation, note it in the commit or PR description

## Git Commit Rules

- **Never** add `Co-Authored-By` trailers to commit messages
- Use the `git-storytelling:git-commit-messages` skill when crafting commit messages
- Use Conventional Commits for all messages and PR titles

## Key Architectural Rules

- Commands implement `CommandLogic` with pure `apply` and `handle` methods
- `execute()` is the canonical entry point — no manual fold/append
- Read models (projections) and write models (command state) are separate code paths
- Domain types validated at boundaries, never re-validated (parse-don't-validate)
- Tests describe behavior, not implementation
- Drill-down to unit tests is proof-based, not diagnostic
- No panics in production code — use `Result` propagation

## Work Tracking Across Sessions

### Tiber

Treat `.development-system.toml` and the development-system plugin as the
workflow authority. When `[features].tiber = true`, use Tiber as the sole
project task tracker for all work items and priorities.

### Session Continuity

At the start of each session:

1. Read MEMORY.md for cross-session context about in-progress work
2. Check Tiber for the active task and ordered backlog
3. Scan blueprint statuses for partially completed work (if blueprints exist)
4. Resume where the previous session left off

Before ending a session that has in-progress work, save a memory note describing what was being worked on, what's done, and what remains.

## Han Plugin Usage

This project uses the han marketplace. Key skill mappings:

- Vague requirements → `the-visionary` agent
- Task breakdown → `technical-coordinator` agent
- Architecture decisions → `system-architect` / `solution-architect` agents
- New features → `/core:develop` (end-to-end guided workflow)
- Code review → `/core:review` (multi-agent)
- Bug investigation → `/core:debug`
- Bug fix → `/core:fix`
- Refactoring → `/core:refactor`
- Performance → `/core:optimize`
- Documentation → `documentation-engineer` agent
- Security review → `security-engineer` agent

@AGENTS.md
