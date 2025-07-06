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

## HIGHEST PRIORITY: Codebase Refactoring

**Critical Issue**: The codebase contains several very long functions and files that are difficult to understand, maintain, and test. This creates barriers for both human developers and LLM assistance.

**Refactoring Strategy**: Each refactoring will be done in its own PR, with PRs chained off each other to enable continuous work without waiting for human review.

### Critical Refactoring Tasks (Must Complete All)

#### 1. Refactor executor.rs (2,956 lines) - **CRITICAL**
**Problem**: Massive single file containing multiple distinct responsibilities
**Tasks**:
- [x] Extract `execute_once` function (157 lines) - Split into pipeline stages (Implemented functional approach with type-state StreamDiscoveryContext)
- [ ] Extract `execute_type_safe` function (139 lines) - Share common patterns
- [ ] Extract `prepare_stream_events_with_complete_concurrency_control` function (140 lines) - Separate validation logic
- [ ] Split executor.rs into modules:
  - [ ] `executor/core.rs` - Core execution logic
  - [ ] `executor/retry.rs` - Retry and circuit breaker logic
  - [x] `executor/stream_discovery.rs` - Stream discovery iteration logic (Created with type-state pattern)
  - [ ] `executor/validation.rs` - Command validation
  - [ ] `executor/context.rs` - Execution context management

#### 2. Refactor cqrs/rebuild.rs::rebuild function (189 lines) - **HIGH**
**Problem**: Complex rebuild logic with multiple responsibilities
**Tasks**:
- [ ] Extract event processing pipeline
- [ ] Extract checkpoint management
- [ ] Extract error handling patterns
- [ ] Create separate functions for each rebuild phase

#### 3. Refactor resource.rs (1,415 lines) - **HIGH**
**Problem**: Single file handling all resource lifecycle patterns
**Tasks**:
- [ ] Extract phantom type definitions to `resource/types.rs`
- [ ] Move concrete implementations to `resource/implementations.rs`
- [ ] Create `resource/lifecycle.rs` for acquisition/release patterns
- [ ] Create `resource/pool.rs` for resource pooling
- [ ] Create `resource/monitor.rs` for resource monitoring

#### 4. Refactor errors.rs Clone implementation (243 lines) - **MEDIUM**
**Problem**: Massive manual Clone implementation for error types
**Tasks**:
- [ ] Examine if #[derive(Clone)] can be used instead
- [ ] Split error types into categories (validation, concurrency, infrastructure)
- [ ] Reduce complexity of error type hierarchy

#### 5. Refactor projection_runner.rs (1,318 lines) - **MEDIUM**
**Problem**: Complex projection processing logic
**Tasks**:
- [ ] Extract event processing pipeline
- [ ] Separate retry logic into `projection/retry.rs`
- [ ] Move monitoring/metrics to `projection/monitoring.rs`
- [ ] Extract task management patterns

#### 6. Refactor serialization/evolution.rs (1,377 lines) - **MEDIUM**
**Problem**: Schema evolution logic is complex and monolithic
**Tasks**:
- [ ] Split migration logic into separate handlers
- [ ] Extract version compatibility checking
- [ ] Create separate modules for different evolution strategies

### Refactoring Process Rules

**IMPORTANT**: Each refactoring task must be completed in its own PR, with PRs chained off each other. This allows continuous work without waiting for human review. To prevent drift and handle merge conflicts effectively:
- Regularly sync feature branches with the main branch to incorporate the latest changes.
- Rebase feature branches onto the main branch before creating new PRs to ensure compatibility.
- Resolve merge conflicts promptly and verify that all integration tests pass after resolving conflicts.

1. **Start with executor.rs refactoring** - This is the most critical
2. **Create feature branch for each refactoring** - Use descriptive names like `refactor-executor-extract-pipeline`
3. **Chain PRs** - Each subsequent PR branches off the previous one
4. **Maintain existing public APIs** - No breaking changes during refactoring
5. **Ensure comprehensive tests** - All integration tests must pass
6. **Document refactoring decisions** - Each PR should explain the refactoring rationale
7. **Continue until all tasks complete** - Work through the entire list systematically

### Testing Strategy for Refactoring

1. **Before refactoring**: Ensure comprehensive integration tests exist
2. **During refactoring**: Maintain existing public APIs
3. **After refactoring**: Verify no performance regressions
4. **Property-based tests**: Ensure refactored code maintains invariants

**DO NOT PROCEED WITH POST-REVIEW IMPROVEMENTS** until all refactoring tasks are complete.

## Next Phase: Post-Review Improvements (ON HOLD)

Based on the comprehensive expert review (see REVIEW.md), the following priority improvements have been identified:

### High Priority (Blocks broader adoption)

#### 1. Snapshot System Implementation
**Problem**: No built-in support for snapshots makes massive streams potentially problematic
**Solution**: 
- [ ] Design snapshot strategy for long-running streams
- [ ] Implement automatic snapshot creation based on event count thresholds
- [ ] Add snapshot restoration capabilities to state reconstruction
- [ ] Document snapshot lifecycle and best practices

#### 2. Enhanced Projection Capabilities for Complex Read Models
**Problem**: Limited support for building projections that need to correlate events across multiple streams
**Solution**:
- [ ] Add stream pattern subscriptions (e.g., subscribe to all "customer-*" streams)
- [ ] Implement event correlation framework for related events (by correlation_id, causation_id)
- [ ] Create projection composition patterns for building complex views
- [ ] Add temporal windowing for time-based event correlation
- [ ] Document patterns for multi-stream projections (e.g., order history, reconciliation)

#### 3. Beginner-Friendly Documentation and Onboarding
**Problem**: Steep learning curve identified as major adoption barrier
**Solution**:
- [ ] Create "EventCore in 15 minutes" quick start guide
- [ ] Add progressive complexity examples (simple → intermediate → advanced)
- [ ] Develop interactive tutorial with common patterns
- [ ] Create migration guide from traditional event sourcing

### Medium Priority (Production enhancements)

#### 4. Advanced Error Recovery and Poison Message Handling
**Problem**: Production systems need robust error handling strategies
**Solution**:
- [ ] Implement dead letter queue patterns for failed events
- [ ] Add automatic retry with exponential backoff
- [ ] Create error quarantine and manual recovery workflows
- [ ] Document operational runbooks for common failure scenarios

#### 5. Performance Optimization and Monitoring
**Problem**: Need better production performance insights and tuning
**Solution**:
- [ ] Add detailed performance metrics and dashboards
- [ ] Implement connection pool optimization for PostgreSQL adapter
- [ ] Create performance profiling tools for command execution
- [ ] Add memory usage monitoring and optimization

#### 6. Enhanced Developer Experience
**Problem**: Complex type system creates friction for new developers
**Solution**:
- [ ] Improve macro error messages with actionable suggestions
- [ ] Add IDE integration and LSP support for better tooling
- [ ] Create debug utilities for command and projection development
- [ ] Implement development-mode warnings for common mistakes

### Low Priority (Future enhancements)

#### 7. Ecosystem Integration
**Problem**: Limited integration with popular Rust web frameworks and tools
**Solution**:
- [ ] Create official Axum integration crate
- [ ] Add Actix Web integration examples
- [ ] Develop Tower middleware for HTTP APIs
- [ ] Create integration with popular serialization formats

#### 8. Multi-Tenant and Scaling Features
**Problem**: Enterprise adoption may require multi-tenancy support
**Solution**:
- [ ] Design tenant isolation strategies
- [ ] Implement tenant-scoped stream access
- [ ] Add horizontal scaling documentation
- [ ] Create cluster deployment examples

#### 9. Advanced Event Sourcing Patterns
**Problem**: Missing some advanced event sourcing capabilities
**Solution**:
- [ ] Implement event upcasting and schema migration
- [ ] Add support for event encryption at rest
- [ ] Create event archival and retention policies
- [ ] Implement advanced causality tracking

## Implementation Priority

1. **Start with #1 (Snapshots)** - This directly addresses the most significant technical limitation
2. **Follow with #3 (Documentation)** - Reduces adoption barriers for new users
3. **Then #2 (Enhanced Projections)** - Enables complex read models while maintaining proper CQRS separation
4. **Address production items (#4-6)** - As real-world usage patterns emerge

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

## Pull Request Workflow

This project uses a **pull request-based workflow**. Direct commits to the main branch are not allowed. All changes must go through pull requests for review and CI validation.

### Key Points

1. **Create feature branches** for logical sets of related changes
2. **CI/CD workflows only run on PRs**, not on branch pushes
3. **PR template must be filled out** - enforced by PR validation workflow
4. **Keep PRs small and focused** for easier review

### Workflow Steps

1. Create a new branch from main
2. Make your changes following development process rules
3. Push your branch
4. Create a PR using `mcp__github__create_pull_request` with **ALL template sections**:
   - Description (what and why)
   - Type of Change (select appropriate type only)
   - Testing checklist (**leave unchecked for human review**)
   - Performance Impact (if applicable)
   - Security Checklist (**leave unchecked for human review**)
   - Code Quality checklist (**leave unchecked for human review**)
   - Reviewer Checklist (**leave unchecked for human review**)
   - Review Focus
5. Monitor CI and address any failures (PR validation will fail until all checklists are checked)
6. Address review feedback by replying to comments with `-- @claude` signature
7. Merge when approved, CI passes, and all checklists are checked by humans

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