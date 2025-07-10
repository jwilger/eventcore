# EventCore Implementation Plan

This document outlines the implementation plan for the EventCore multi-stream event sourcing library using a strict type-driven development approach with test-driven implementation.

## Implementation Philosophy

1. **CI/CD First**: Set up continuous integration before any code
2. **Type-First**: Define all types that make illegal states unrepresentable
   - Use `nutype` validation ONLY at library input boundaries
   - Once parsed into domain types, validity is guaranteed by the type system
   - No runtime validation needed within the library - types ensure correctness
3. **Stub Functions**: Create all function signatures with `todo!()` bodies
4. **Property Tests First**: Write property-based tests to verify invariants
5. **Test-Driven Implementation**: Replace `todo!()` with implementations guided by tests
6. **Integration Last**: Add infrastructure only after core logic is complete

## Project Status

EventCore has successfully completed all initially planned phases (1-20), including:

- CI/CD pipeline and project setup
- Core type system with validated domain types
- Command system with type-safe stream access
- Event store abstraction and adapters (PostgreSQL, in-memory)
- Comprehensive testing infrastructure
- Property-based tests for system invariants
- Performance benchmarks and monitoring
- Developer experience improvements (macros, error diagnostics)
- Complete examples (banking, e-commerce)
- Documentation and release preparation
- Expert review improvements and API simplification
- Production hardening and observability features
- Type system optimizations and performance improvements
- Advanced phantom type implementations for compile-time safety
- Complete subscription system with position tracking
- Dead code cleanup and CI fixes

## Codebase Refactoring and Improvements

### Migration to GitHub Issues Complete (2025-07-10)

All refactoring tasks and post-review improvements have been successfully migrated to GitHub Issues:

**Refactoring Issues Created:**
- Issue #53: [CRITICAL] Refactor executor.rs (2,956 lines)
- Issue #54: [HIGH] Refactor cqrs/rebuild.rs::rebuild function (189 lines)
- Issue #75: [HIGH] Refactor resource.rs (1,415 lines)
- Issue #76: [MEDIUM] Refactor errors.rs Clone implementation (243 lines)
- Issue #77: [MEDIUM] Refactor projection_runner.rs (1,318 lines)
- Issue #78: [MEDIUM] Refactor serialization/evolution.rs (1,377 lines)

**Post-Review Improvement Issues Created:**
- Issue #79: [HIGH] Implement Snapshot System for Long-Running Streams
- Issue #80: [HIGH] Enhanced Projection Capabilities for Complex Read Models
- Issue #81: [HIGH] Beginner-Friendly Documentation and Onboarding
- Issue #82: [MEDIUM] Advanced Error Recovery and Poison Message Handling
- Issue #83: [MEDIUM] Performance Optimization and Monitoring
- Issue #84: [MEDIUM] Enhanced Developer Experience
- Issue #85: [LOW] Ecosystem Integration
- Issue #86: [LOW] Multi-Tenant and Scaling Features
- Issue #87: [LOW] Advanced Event Sourcing Patterns

All future work should be tracked through GitHub Issues. This PLANNING.md file now serves as a historical record of completed work.

### Original Refactoring Strategy

Each refactoring will be done in its own PR, with PRs chained off each other to enable continuous work without waiting for human review.

### Refactoring Tasks

All refactoring tasks have been migrated to GitHub Issues. See Issues #53-#78 for details.

### Post-Review Improvements

All post-review improvements have been migrated to GitHub Issues. See Issues #79-#87 for details.


All documented implementation phases have been completed. The project is ready for:
- Production usage (with caveats noted in review)
- Community feedback
- Feature requests
- Performance optimization based on real-world usage patterns

### Recent Maintenance (2025-07-05)
- Reviewed and updated all documentation for consistency
- Fixed outdated Command trait references (now CommandLogic)
- Updated broken documentation links in README.md
- Corrected license information to reflect MIT-only licensing
- Ensured all examples use current API patterns
- Created modern documentation website with mdBook
  - Set up GitHub Pages deployment workflow
  - Implemented custom EventCore branding and responsive design
  - Automated documentation synchronization from markdown sources
  - Configured deployment on releases with version information

### Security Infrastructure Setup (2025-07-05)
- [x] Created SECURITY.md with vulnerability reporting via GitHub Security Advisories
- [x] Improved cargo-audit CI job to use rustsec/audit-check action
- [x] Configured Dependabot for automated dependency updates (Rust and GitHub Actions)
- [x] Created comprehensive CONTRIBUTING.md with GPG signing documentation
- [x] Added security considerations for application developers to SECURITY.md
- [x] Created detailed security guide in user manual (06-security):
  - Overview of security architecture and responsibilities
  - Authentication & authorization patterns
  - Data encryption strategies
  - Input validation techniques
  - Compliance guidance (GDPR, PCI DSS, HIPAA, SOX)
- [x] Reorganized documentation structure (renumbered operations to 07, reference to 08)
- [x] Created comprehensive COMPLIANCE_CHECKLIST.md mapping to OWASP/NIST/SOC2/PCI/GDPR/HIPAA
- [x] Added pull request template with security and performance review checklists
- [x] Created PR validation workflow to enforce template usage
- [x] Added compliance checklist reference to security manual
- [x] Consolidated documentation to single source (symlinked docs/manual to website/src/manual)
- [x] Updated PR template to remove redundant pre-merge checklist and add Review Focus section
- [x] Updated PR validation workflow to require Review Focus section
- [x] Added GitHub Copilot instructions for automated PR reviews aligned with our checklists
- [x] Fixed doctest compilation error in resource.rs
- [x] Added doctests to pre-commit hooks to prevent future doctest failures
- [x] Updated CLAUDE.md and PLANNING.md to reflect GitHub MCP server integration for all GitHub operations
- [x] Updated CLAUDE.md and PLANNING.md to document PR-based workflow and clarify that CI only runs on PRs
- [x] Updated pre-commit hook to auto-format and stage files instead of failing
- [x] Removed redundant "run all tests" requirement from commit process (pre-commit hooks handle this)
- [x] Consolidated duplicate PR workflow sections in CLAUDE.md
- [x] Added PR template requirements and validation workflow documentation
- [x] Added PR feedback response process using gh GraphQL API for threaded replies
- [x] Enhanced todo list structure documentation to reinforce workflow and prevent process drift
- [x] Updated PR validation workflow to require ALL checklist items be checked by humans
- [x] Added documentation clarifying that checklists must NOT be pre-checked by automation
- [x] Improved PR validation to auto-convert to draft if submitter checklists incomplete
- [x] Reduced validation noise by skipping draft PRs and avoiding redundant comments
- [x] Added debug logging to troubleshoot workflow section detection
- [x] Fixed PR validation workflow to include synchronize trigger for new commits
- [x] Fixed regex pattern to properly capture subsections in checklists
- [x] Added better error handling for draft conversion API calls
- [x] Updated workflow message to acknowledge GitHub API limitation on draft conversion
- [x] Implemented GraphQL API for draft conversion as alternative to REST API limitation
- [x] Set up workflow to use PAT (PR_DRAFT_PAT secret) for draft conversion capability
- [x] Removed notification sound from CLAUDE.md and PLANNING.md per user request
- [x] Simplified PR template by consolidating multiple checklists into single Submitter Checklist (Issue #23)

### Dependency Updates (2025-07-05)
- [x] Merged PR #3: Update actions/configure-pages from v4 to v5
- [x] Merged PR #4: Update codecov/codecov-action from v3 to v5
- [x] Fixed rand crate v0.9.1 deprecation errors in PR #5:
  - Updated `thread_rng()` to `rng()` in executor.rs, testing/chaos.rs, and retry.rs
  - Updated `gen()` to `random()` and `gen_range()` to `random_range()`
  - Fixed ThreadRng Send issue in stress tests by generating random numbers outside async block
- [x] Fixed OpenTelemetry v0.30.0 API breaking changes in PR #5:
  - Updated `Resource::new()` to `Resource::builder()` pattern
  - Removed unnecessary runtime parameter from `PeriodicReader::builder()`
  - Added required `grpc-tonic` feature to opentelemetry-otlp dependency
- [x] Fixed bincode v2.0.1 API breaking changes in PR #6:
  - Updated to use `bincode::serde::encode_to_vec()` and `bincode::serde::decode_from_slice()` APIs
  - Added "serde" feature to bincode dependency in Cargo.toml
  - Replaced deprecated `bincode::serialize()` and `bincode::deserialize()` functions
  - All tests passing with new bincode v2 API

### Dependency Updates (2025-07-05)
- [x] Merged PR #3: Update actions/configure-pages from v4 to v5
- [x] Merged PR #4: Update codecov/codecov-action from v3 to v5
- [x] Fixed rand crate v0.9.1 deprecation errors in PR #5:
  - Updated `thread_rng()` to `rng()` in executor.rs, testing/chaos.rs, and retry.rs
  - Updated `gen()` to `random()` and `gen_range()` to `random_range()`
  - Fixed ThreadRng Send issue in stress tests by generating random numbers outside async block
- [x] Fixed OpenTelemetry v0.30.0 API breaking changes in PR #5:
  - Updated `Resource::new()` to `Resource::builder()` pattern
  - Removed unnecessary runtime parameter from `PeriodicReader::builder()`
  - Added required `grpc-tonic` feature to opentelemetry-otlp dependency

### Release Workflow Fixes (2025-07-07)
- [x] Fixed PostgreSQL test configuration (missing TEST_DATABASE_URL) in PR #31
- [x] Fixed documentation sync script symlink issue in PR #31
- [x] Fixed Cargo.toml version specifications for crates.io publishing in PR #33
- [x] Fixed CSS directory creation for documentation builds in PR #33
- [x] Fixed additional Cargo.toml syntax errors and dependency versions in PR #33:
  - Fixed `rand.workspace = true` syntax error to `rand = { workspace = true }`
  - Added missing version to eventcore-memory dev-dependency
  - Implemented workspace dependencies for internal crates to enable automatic lockstep versioning
  - Added all internal crates to workspace.dependencies in root Cargo.toml with path + version
  - Updated all internal dependency references to use `{ workspace = true }`
  - Eliminates manual version updates when bumping workspace version for releases
- [x] Fixed version conflicts preventing re-release after partial crates.io publishing:
  - v0.1.0 release failed after publishing eventcore-macros but before publishing other crates
  - Deleted failed v0.1.0 release and tag from GitHub
  - Bumped workspace version to 0.1.1 to avoid crates.io version conflicts
  - Updated all internal crate versions to 0.1.1 in workspace dependencies
- [x] Fixed circular dependency preventing v0.1.1 release:
  - eventcore-macros v0.1.1 required eventcore v0.1.1 in dev-dependencies but publishing order is macros-first
  - Removed eventcore dev-dependency from eventcore-macros (only used for placeholder tests)
  - Deleted failed v0.1.1 release and tag to allow clean re-release attempt
  - This eliminates the circular dependency that prevented successful crates.io publishing
- [x] Fixed crates.io publishing order to resolve dev-dependency circular dependency:
  - Issue: eventcore has eventcore-memory and eventcore-postgres as dev-dependencies
  - Problem: Release workflow was trying to publish eventcore before its dev-dependencies existed on crates.io
  - Solution: Updated publishing order to macros → memory → postgres → eventcore
  - This ensures all dev-dependencies are available before eventcore is published
  - Created PR #42 with proper template format to implement the fix
- [x] Added PR template compliance rules to CLAUDE.md:
  - Added requirement to always read .github/pull_request_template.md before creating PRs
  - Added steps to use exact template content and keep HTML comments hidden
  - Added CRITICAL RULE #3 to prevent future template violations
  - This ensures all future PRs follow the project's template requirements

### Workflow Improvements (2025-07-07)
- [x] Added PR validation workflow debouncing:
  - Problem: PR validation workflow triggered on every checkbox change, creating excessive CI runs
  - Solution: Added 30-second sleep delay when PR is edited to allow multiple checkbox changes
  - Also added 5-minute comment deduplication to avoid spam when users check multiple boxes
  - This reduces CI noise while maintaining validation effectiveness
- [x] Fixed PR description HTML comment handling:
  - Problem: HTML comments in PR template were being escaped and shown as visible text
  - Initial solution: Tried to preserve HTML comments as hidden, but kept failing
  - Final solution: Updated CLAUDE.md to instruct stripping out ALL HTML comments
  - HTML comments are instructions for automation, not content for the PR
  - Clean PR descriptions now contain only the visible template structure
- [x] Enhanced PR template usage instructions:
  - Problem: PR descriptions were not following template structure exactly
  - Solution: Updated CLAUDE.md to emphasize using template VERBATIM
  - Must copy all checkboxes, headers, and structure exactly as written
  - Only fill in description content areas, never modify template structure
- [x] Replaced PR validation workflow and template with Definition of Done bot:
  - Removed PR validation workflow that was causing excessive friction
  - Removed PR template in favor of automatic DoD checklist
  - Added dod.yaml configuration with project checklist items
  - Added definition-of-done.yml workflow to automatically add checklists to PRs
  - Updated CLAUDE.md to reflect new PR workflow without templates

### Dependency Management (2025-07-07)
- [x] Fixed Dependabot creating PRs for internal workspace crates:
  - Problem: Dependabot was suggesting version updates for our own workspace crates
  - Cause: Internal crates listed in workspace.dependencies with version numbers
  - Initial solution: Added ignore rules in dependabot.yml for all internal workspace crates
  - Better solution: Removed version numbers from internal workspace dependencies
  - Cargo automatically infers versions from workspace.package.version for crates.io
  - Now only need to update version in one place: workspace.package.version
  - Removed Dependabot ignore rules as they're no longer needed

### Development Process Improvements (2025-07-07)
- [x] Added CRITICAL RULE #4 to CLAUDE.md:
  - Always stop and ask for help rather than taking shortcuts that violate rules
  - When faced with obstacles, must ask user for guidance
  - Especially important when tempted to use --no-verify or bypass safety checks
  - Emphasized that it's better to ask for help than violate safety rules

### Development Documentation Improvements (2025-07-08)
- [x] Reorganized CLAUDE.md for better LLM effectiveness:
  - Moved critical rules to the very top for immediate visibility
  - Added comprehensive table of contents with task-based quick reference
  - Consolidated Development Process Rules right after Project Overview
  - Added reminder callouts at key decision points throughout the file
  - Added emoji indicators for better visual scanning
  - Created final critical reminders section at the end
  - Optimized for single-file context to ensure all rules are always visible

## Pull Request Workflow

This project uses a **pull request-based workflow**. Direct commits to the main branch are not allowed. All changes must go through pull requests for review and CI validation.

### Key Points

1. **Create feature branches** for logical sets of related changes
2. **CI/CD workflows only run on PRs**, not on branch pushes
3. **Definition of Done checklist** will be automatically added to PRs
4. **Keep PRs small and focused** for easier review

### Workflow Steps

1. Create a new branch from main
2. Make your changes following development process rules
3. Push your branch
4. Create a PR using `mcp__github__create_pull_request` with a clear description
5. Monitor CI and address any failures
6. Address review feedback by replying to comments with `-- @claude` signature
7. Merge when approved, CI passes, and humans have verified the Definition of Done checklist

## Active Development

All active development is now tracked through GitHub Issues. See the [Issues page](https://github.com/jwilger/eventcore/issues) for current work items.

## Development Process Rules

When working on this project, **ALWAYS** follow these rules:

1. **BROKEN CI BUILDS ARE HIGHEST PRIORITY** - If CI is failing on your PR, stop all other work and fix it immediately.
2. **Review @PLANNING.md** to discover the next task to work on.
3. **Create a new branch** for the task if starting fresh work.
4. **IMMEDIATELY use the todo list tool** to create a todolist with the specific actions you will take to complete the task.
5. **ALWAYS include "Update @PLANNING.md to mark completed tasks" in your todolist** - This task should come BEFORE the commit task to ensure completed work is tracked.
6. **Insert a task to "Run relevant tests (if any) and make a commit"** after each discrete action that involves a change to the code, tests, database schema, or infrastructure. Note: Pre-commit hooks will run all checks automatically.
7. **The FINAL item in the todolist MUST always be** to "Push your changes to the remote repository and create/update PR with GitHub MCP tools."

### CRITICAL: Todo List Structure

**This structure ensures Claude never forgets the development workflow:**

Your todo list should ALWAYS follow this pattern:
1. Implementation/fix tasks (the actual work)
2. "Update @PLANNING.md to mark completed tasks" 
3. "Make a commit" (pre-commit hooks run all checks automatically)
4. "Push changes and update PR"

For PR feedback specifically:
1. Address each piece of feedback
2. "Reply to review comments using gh GraphQL API with -- @claude signature"
3. "Update @PLANNING.md to mark completed tasks"
4. "Make a commit"
5. "Push changes and check for new PR feedback"

**Why this matters**: The todo list tool reinforces our workflow at every step, preventing process drift as context grows.

### CI Monitoring Rules

After creating or updating a PR:
1. **CI runs automatically on the PR** - No need to trigger manually
2. **Use GitHub MCP tools to monitor the CI workflow** on your PR
3. **If the workflow fails** - Address the failures immediately before continuing
4. **If the workflow passes** - PR is ready for review

We now have access to GitHub MCP server which provides native GitHub integration. Use these MCP tools:

- `mcp__github__get_pull_request` - Check PR status including CI checks
- `mcp__github__list_workflow_runs` - List recent workflow runs for the repository
- `mcp__github__get_workflow_run` - Get details of a specific workflow run
- `mcp__github__list_workflow_jobs` - List jobs for a workflow run to see which failed
- `mcp__github__get_job_logs` - Get logs for failed jobs to debug issues

### Commit Rules

**BEFORE MAKING ANY COMMIT**:
1. **Ensure @PLANNING.md is updated** - All completed tasks must be marked with [x]
2. **Include the updated PLANNING.md in the commit** - Use `git add PLANNING.md`

**COMMIT MESSAGE FORMAT**:
- **NO PREFIXES** in subject line (no "feat:", "fix:", "refactor:", etc.)
- **Subject line**: Maximum 50 characters, imperative mood
- **Body lines**: Maximum 72 characters before hard-wrapping
- **Focus on WHY, not just what/how** - Explain the reasoning and motivation
- Example:
  ```
  Add subscription system with position tracking
  
  Expert review identified missing subscription capabilities as a major
  gap preventing production usage. Without real-time event processing,
  projections cannot stay current and users lose audit trail benefits.
  
  Implement comprehensive subscription system with automatic position
  tracking, checkpointing, and replay functionality. This enables
  real-time read models and eliminates polling-based workarounds.
  
  All integration tests pass with PostgreSQL backend.
  ```

**NEVER** make a commit with the `--no-verify` flag. All pre-commit checks must be passing before proceeding.