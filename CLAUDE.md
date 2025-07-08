# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 🚨 CRITICAL RULES - ALWAYS APPLY

**These rules must NEVER be violated under any circumstances:**

1. **NEVER use the `--no-verify` flag when committing code**
2. **NEVER check ANY checkboxes in PR templates** - All checkboxes must be left unchecked [ ] for human verification
3. **ALWAYS read .github/pull_request_template.md before creating any PR**
4. **ALWAYS stop and ask for help rather than taking shortcuts** - When faced with obstacles, ask the user for guidance
5. **Update @PLANNING.md before committing when working on planned tasks** - Mark completed tasks with [x] and include in commit
6. **ALWAYS follow the exact todo list structure** - This prevents process drift

## 📋 TABLE OF CONTENTS

### Quick Reference by Task
- **🆕 Starting new work?** → Read [🚨 Critical Rules](#critical-rules---always-apply), [Development Process Rules](#development-process-rules)
- **🔧 Setting up environment?** → Read [Development Commands](#development-commands)
- **💻 Writing code?** → Read [Architecture](#architecture), [Type-Driven Development](#type-driven-development-philosophy)
- **📤 Making commits?** → Read [Commit Rules](#commit-rules), [Pre-commit Hooks](#pre-commit-hooks)
- **🔄 Creating/updating PRs?** → Read [Pull Request Workflow](#pull-request-workflow), [🚨 Critical Rules](#critical-rules---always-apply)
- **💬 Responding to PR feedback?** → Read [Responding to PR Feedback](#responding-to-pr-feedback)
- **💙 Using GitHub features?** → Read [GitHub MCP Integration](#github-mcp-integration)

### All Sections
1. [🚨 Critical Rules](#critical-rules---always-apply) (THIS SECTION - READ FIRST!)
2. [Project Overview](#project-overview)
3. [Development Process Rules](#development-process-rules) (How to work on this project)
4. [Type-Driven Development Philosophy](#type-driven-development-philosophy)
5. [Development Commands](#development-commands)
6. [Architecture](#architecture)
7. [Performance Targets](#performance-targets)
8. [Pre-commit Hooks](#pre-commit-hooks)
9. [Development Principles](#development-principles)
10. [GitHub MCP Integration](#github-mcp-integration)
11. [Pull Request Workflow](#pull-request-workflow)
12. [Memories](#memories) (Important reminders)

## Project Overview

EventCore is a multi-stream event sourcing library that implements dynamic consistency boundaries. This approach, building on established event sourcing patterns, eliminates traditional aggregate boundaries in favor of self-contained commands that can read from and write to multiple streams atomically.

## Development Process Rules

**🚨 REMINDER: Review [Critical Rules](#critical-rules---always-apply) before proceeding!**

When working on this project, **ALWAYS** follow these rules:

1. **Review @PLANNING.md** to discover the next task to work on.
2. **Follow the Pull Request Workflow** (see [Pull Request Workflow](#pull-request-workflow)) for all code changes.
3. **IMMEDIATELY use the todo list tool** to create a todolist with the specific actions you will take to complete the task.
4. **When working on tasks from @PLANNING.md, insert a task to "Update @PLANNING.md to mark completed tasks"** before any commit task. For ad-hoc requests not in PLANNING.md, skip this step unless explicitly asked to add them.
5. **Insert a task to "Make a commit"** after each discrete action that involves a change to the code, tests, database schema, or infrastructure. Note: Pre-commit hooks will run all checks automatically.
6. **The FINAL item in the todolist MUST always be** to "Push your changes to the remote repository and create/update PR with GitHub MCP tools."

### CRITICAL: Todo List Structure

**This structure ensures Claude never forgets the development workflow:**

Your todo list should ALWAYS follow this pattern:

**For planned work from @PLANNING.md:**
1. START with writing tests for any changes BEFORE making the changes, and ensure the tests fail as you expect them to.
2. Implementation/fix tasks (the actual work)
3. "Update @PLANNING.md to mark completed tasks" 
4. "Make a commit" (pre-commit hooks run all checks automatically)
5. "Push changes and update PR"

**For ad-hoc requests not in @PLANNING.md:**
1. START with writing tests for any changes BEFORE making the changes, and ensure the tests fail as you expect them to.
2. Implementation/fix tasks (the actual work)
3. "Make a commit" (pre-commit hooks run all checks automatically)
4. "Push changes and update PR"

For PR feedback specifically:
1. Address each piece of feedback
2. "Reply to review comments using gh GraphQL API with -- @claude signature"
3. "Update @PLANNING.md to mark completed tasks"
4. "Make a commit"
5. "Push changes and check for new PR feedback"

**Why this matters**: The todo list tool reinforces our workflow at every step, preventing process drift as context grows.

### Commit Rules

**BEFORE MAKING ANY COMMIT**:

1. **If working on planned tasks from @PLANNING.md**:
   - Ensure completed tasks are marked with [x]
   - Include the updated PLANNING.md in the commit - Use `git add PLANNING.md`
2. **If working on ad-hoc requests**: 
   - Only update PLANNING.md if explicitly asked to track the work there
   - Otherwise, proceed with the commit without updating PLANNING.md

**🚨 CRITICAL REMINDER**: NEVER use `--no-verify` flag. All pre-commit checks must pass!

## Type-Driven Development Philosophy

This project follows strict type-driven development principles as outlined in the global Claude.md. Key principles:

1. **Types come first**: Model the domain, make illegal states unrepresentable, then implement
2. **Parse, don't validate**: Transform unstructured data into structured data at system boundaries ONLY
   - Validation should be encoded in the type system to the maximum extent possible
   - Use smart constructors with `nutype` validation only at the library's input boundaries
   - Once data is parsed into domain types, those types guarantee validity throughout the system
   - Library users should be encouraged to follow the same pattern in their application code
3. **No primitive obsession**: Use newtypes for all domain concepts
4. **Functional Core, Imperative Shell**: Pure functions at the heart, side effects at the edges
5. **Total functions**: Every function should handle all cases explicitly

For detailed type-driven development guidance, refer to `/home/jwilger/.claude/CLAUDE.md`.

## Development Commands

**🚨 REMINDER**: Never use `--no-verify` flag! See [Critical Rules](#critical-rules---always-apply)

### Setup

```bash
# Enter development environment (required for all work)
nix develop

# Start PostgreSQL databases
docker-compose up -d

# Initialize Rust project (if not done)
cargo init --lib

# Install development tools
cargo install cargo-nextest --locked  # Fast test runner
cargo install cargo-llvm-cov --locked # Code coverage

# IMPORTANT: Always check for latest versions before adding dependencies
# Use: cargo search <crate_name> to find latest version

# Core dependencies
cargo add tokio --features full
cargo add async-trait
cargo add uuid --features v7
cargo add serde --features derive
cargo add serde_json
cargo add sqlx --features runtime-tokio-rustls,postgres,uuid,chrono
cargo add thiserror
cargo add tracing
cargo add tracing-subscriber

# Type safety dependencies
cargo add nutype --features serde  # For newtype pattern with validation
cargo add derive_more  # For additional derives on newtypes
```

### Development Workflow

```bash
# Format code
cargo fmt

# Run linter
cargo clippy --workspace --all-targets -- -D warnings

# Run tests with nextest (recommended - faster and better output)
cargo nextest run --workspace

# Run tests with cargo test (fallback)
cargo test --workspace

# Run tests with output
cargo nextest run --workspace --nocapture
# Or with cargo test: cargo test --workspace -- --nocapture

# Run a specific test
cargo nextest run test_name
# Or with cargo test: cargo test test_name -- --nocapture

# Type check
cargo check --all-targets

# Build release version
cargo build --release

# Run benchmarks
cargo bench
```

### Database Operations

```bash
# Connect to main database
psql -h localhost -p 5432 -U postgres -d eventcore

# Connect to test database
psql -h localhost -p 5433 -U postgres -d eventcore_test

# Run database migrations (once implemented)
sqlx migrate run
```

## Architecture

### Core Design Principles

1. **Multi-Stream Event Sourcing**: Commands can atomically read from and write to multiple event streams
2. **Dynamic Consistency Boundaries**: Each command defines its own consistency boundary
3. **Type-Driven Development**: Use Rust's type system to make illegal states unrepresentable
4. **Functional Core, Imperative Shell**: Pure business logic with side effects at boundaries

### Module Structure

```
src/
├── command.rs              # Command traits (CommandStreams, CommandLogic)
├── cqrs/                   # CQRS read model support
├── errors.rs               # Error types
├── event.rs                # Event trait and utilities
├── event_store.rs          # EventStore trait
├── event_store_adapter.rs  # EventStore adapter trait
├── executor.rs             # CommandExecutor implementation
├── executor/               # Executor internals and configuration
├── lib.rs                  # Public API surface
├── macros.rs               # Helper macros (require!, emit!)
├── metadata.rs             # Event metadata types
├── monitoring/             # Observability and monitoring
├── projection.rs           # Projection trait
├── projection_manager.rs   # Projection management
├── projection_protocol.rs  # Projection type safety
├── projection_runner.rs    # Projection execution
├── resource.rs             # Resource types
├── serialization/          # Event serialization formats
├── state_reconstruction.rs # State rebuilding from events
├── subscription.rs         # Event subscription system
├── subscription_adapter.rs # Subscription adapter trait
├── subscription_typestate.rs # Type-safe subscription states
├── testing/                # Testing utilities
├── type_registry.rs        # Type registration system
├── types.rs                # Domain types (StreamId, EventId, etc.)
├── utils/                  # Utilities
└── validation.rs           # Validation helpers
```

### Key Type Patterns

```rust
use nutype::nutype;

// IMPORTANT: nutype validation should ONLY be used at library input boundaries
// Once parsed, these types guarantee validity throughout the system

// StreamId: validation at parse time ensures non-empty, max 255 chars
// After construction, StreamId is ALWAYS valid - no need to re-validate
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
    derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Deref, Serialize, Deserialize)
)]
pub struct StreamId(String);

// EventId: ensures UUIDv7 format at construction
// The type itself guarantees this constraint - no runtime checks needed
#[nutype(
    validate(predicate = |id: &uuid::Uuid| id.get_version() == Some(uuid::Version::SortRand)),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Deref, Serialize, Deserialize)
)]
pub struct EventId(uuid::Uuid);

// EventVersion: non-negative by construction
// Type system ensures this invariant - impossible to create negative version
#[nutype(
    validate(greater_or_equal = 0),
    derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Into, Serialize, Deserialize)
)]
pub struct EventVersion(u64);

// Example of encoding business rules in types rather than runtime validation:
// Instead of validating transfer amounts, use types that make invalid states impossible
pub enum TransferAmount {
    // Each variant encodes different business rules
    Standard(Money),              // Normal transfers with standard limits
    HighValue(HighValueMoney),    // Requires additional authorization
    Recurring(RecurringAmount),   // Has different validation rules
}

// Use Result types for all fallible operations
pub type CommandResult<T> = Result<T, CommandError>;
pub type EventStoreResult<T> = Result<T, EventStoreError>;

// Model errors as enums - make illegal states unrepresentable
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
    #[error("Business rule violation: {0}")]
    BusinessRuleViolation(String),
    #[error("Concurrency conflict on streams: {0:?}")]
    ConcurrencyConflict(Vec<StreamId>),
    #[error("Stream not found: {0}")]
    StreamNotFound(StreamId),
    #[error("Unauthorized: missing permission {0}")]
    Unauthorized(String),
}
```

### Command Implementation Pattern

```rust
// The Command trait is now split into two parts:

// 1. CommandStreams - Typically auto-generated by #[derive(Command)]
pub trait CommandStreams: Send + Sync + Clone {
    type StreamSet: Send + Sync;
    fn read_streams(&self) -> Vec<StreamId>;
}

// 2. CommandLogic - Manually implemented with your domain logic
#[async_trait]
pub trait CommandLogic: CommandStreams {
    type State: Default + Send + Sync;
    type Event: Send + Sync;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>);

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>>;
}

// The #[derive(Command)] macro generates:
// - Implementation of CommandStreams trait
// - StreamSet phantom type (e.g., TransferMoneyStreamSet)
// - Helper method __derive_read_streams() for convenience
```

### Type-Safe Stream Access

Commands now have compile-time guarantees that they can only write to streams they declared:

```rust
// In your command's handle method:
async fn handle(
    &self,
    read_streams: ReadStreams<Self::StreamSet>,
    state: Self::State,
    input: Self::Input,
    stream_resolver: &mut StreamResolver,
) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
    // StreamWrite::new() ensures you can only write to declared streams
    let event = StreamWrite::new(
        &read_streams,
        input.account_stream(),
        AccountEvent::Deposited { amount: input.amount }
    )?; // Returns error if stream wasn't declared in read_streams()
    
    Ok(vec![event])
}
```

### Dynamic Stream Discovery

Commands can dynamically request additional streams during execution:

```rust
// After analyzing state, request additional streams
let product_streams: Vec<StreamId> = state.order.items.keys()
    .map(|id| StreamId::try_new(format!("product-{}", id)).unwrap())
    .collect();
stream_resolver.add_streams(product_streams);

// The executor will automatically re-read all streams and rebuild state
```

### Testing Philosophy

1. **Property-Based Testing**: Use `proptest` for invariant testing
2. **In-Memory Event Store**: Fast, deterministic tests
3. **Integration Tests**: Test complete workflows with real PostgreSQL
4. **Benchmark Suite**: Track performance regressions

Follow the testing principles from the global Claude.md:

- Test behavior, not implementation
- Focus on edge cases that types can't prevent
- Use test names that describe business requirements
- Property tests for invariants, example tests for specific behaviors

## Important Implementation Notes

1. **Event Ordering**: Use UUIDv7 for event IDs to enable global chronological ordering
2. **Concurrency Control**: Track stream versions during reads, verify on writes
3. **Multi-Stream Atomicity**: Use PostgreSQL transactions for consistency
4. **Type Safety**: Never use primitive types directly for domain concepts - use `nutype` crate
5. **Error Handling**: Always use Result types, never panic in business logic
6. **Smart Constructors**: All domain types should use smart constructors that validate
7. **Parse, Don't Validate**: Transform unstructured data into structured data at boundaries
8. **Railway-Oriented Programming**: Chain operations using Result and Option types

## Performance Targets

- Single-stream commands: 86 ops/sec (stable)
- Multi-stream commands: estimated 25-50 ops/sec
- Event store writes: 9,000+ events/sec (batched)
- P95 command latency: ~14ms

## Pre-commit Hooks

**🚨 CRITICAL**: These hooks ensure code quality. NEVER bypass them with `--no-verify`!

The project uses pre-commit hooks that automatically run:

1. `cargo fmt --all && git add -u` - Auto-formats code and stages changes (runs first)
2. `cargo clippy` - Linting
3. `cargo test` - All tests
4. `cargo check` - Type checking

The formatting hook automatically fixes and stages formatting issues instead of failing, saving time during the commit process.

## Development Principles

### Type-Driven Development Workflow

1. **Model the Domain First**: Define types that make illegal states impossible
2. **Create Smart Constructors**: Validate at system boundaries using `nutype`
3. **Write Property-Based Tests**: Test invariants and business rules
4. **Implement Business Logic**: Pure functions operating on valid types
5. **Add Infrastructure Last**: Database, serialization, monitoring

### Code Review Focus

**🚨 REMINDER**: All PR checkboxes must be left unchecked for human verification!

Before submitting code, ensure:

- [ ] All domain types use `nutype` with appropriate validation
- [ ] No primitive obsession - all domain concepts have their own types
- [ ] All functions are total (handle all cases)
- [ ] Errors are modeled in the type system
- [ ] Business logic is pure and testable
- [ ] Property-based tests cover invariants

### Library Version Management

**IMPORTANT**: Always check for the latest version of dependencies before adding them:

```bash
# Search for latest version
cargo search <crate_name>

# Or check on crates.io for the most recent stable version
```

This ensures we're using the most up-to-date and secure versions of all dependencies.

## GitHub MCP Integration

**🚨 IMPORTANT**: Use MCP tools instead of gh CLI for all GitHub operations!

This project now uses GitHub MCP (Model Context Protocol) server for all GitHub interactions. **MCP tools are the primary and preferred way to interact with GitHub**, replacing gh CLI commands.

### Available GitHub MCP Tools

Key tools for development workflow:

- **Workflow Management**:
  - `mcp__github__list_workflow_runs` - List and monitor CI/CD runs
  - `mcp__github__get_workflow_run` - Get detailed workflow status
  - `mcp__github__list_workflow_jobs` - View individual job status
  - `mcp__github__get_job_logs` - Retrieve logs for debugging failures
  - `mcp__github__rerun_failed_jobs` - Re-run only failed jobs
  - `mcp__github__rerun_workflow_run` - Re-run entire workflow

- **Pull Request Management**:
  - `mcp__github__create_pull_request` - Create new PRs
  - `mcp__github__get_pull_request` - View PR details
  - `mcp__github__update_pull_request` - Update PR title/description
  - `mcp__github__merge_pull_request` - Merge approved PRs
  - `mcp__github__request_copilot_review` - Request automated review

- **Issue Management**:
  - `mcp__github__create_issue` - Create new issues
  - `mcp__github__update_issue` - Update issue status/labels
  - `mcp__github__list_issues` - View open issues
  - `mcp__github__add_issue_comment` - Add comments to issues

- **Repository Operations**:
  - `mcp__github__create_branch` - Create feature branches
  - `mcp__github__push_files` - Push multiple files in one commit
  - `mcp__github__get_file_contents` - Read files from GitHub
  - `mcp__github__create_or_update_file` - Update single files

### Why MCP Over gh CLI

1. **Native Integration**: Direct API access without shell command overhead
2. **Type Safety**: Structured parameters and responses
3. **Better Error Handling**: Clear error messages and recovery options
4. **Richer Data**: Full API responses with all metadata
5. **Batch Operations**: Efficient multi-file operations

## Pull Request Workflow

This project uses a **pull request-based workflow**. Direct commits to the main branch are not allowed. All changes must go through pull requests for review and CI validation.

### Branch Strategy

1. **Create feature branches** for logical sets of related changes
2. **Use descriptive branch names** that indicate the purpose (e.g., `add-snapshot-system`, `fix-connection-pool-timeout`)
3. **Keep branches focused** - one conceptual change per PR makes reviews easier
4. **Rebase on main** if your branch falls behind to avoid merge conflicts

### PR Workflow Steps

**🚨 CRITICAL**: Never check ANY checkboxes in PR templates! See [Critical Rules](#critical-rules---always-apply)

1. **Create a new branch** from main for your changes:
   ```bash
   git checkout main && git pull origin main
   git checkout -b descriptive-branch-name
   ```

2. **Make your changes** following the [Development Process Rules](#development-process-rules)

3. **Push your branch** when ready for review:
   ```bash
   git push -u origin descriptive-branch-name
   ```

4. **Create a Pull Request** using GitHub MCP tools:
   ```
   mcp__github__create_pull_request
   ```
   
   **PR TEMPLATE REQUIREMENTS**:
   
   **STEP 1**: ALWAYS read the PR template first:
   ```
   Read .github/pull_request_template.md before creating ANY PR
   ```
   
   **STEP 2**: Process the template content for the PR description:
   - Read the template to understand the structure and requirements
   - **STRIP OUT ALL HTML COMMENTS** - they are instructions for you, not content for the PR
   - **USE THE TEMPLATE VERBATIM** except for filling in content sections:
     - Copy ALL section headers exactly as written (## Description, ## Type of Change, etc.)
     - Copy ALL checkboxes exactly as written (leave ALL unchecked [ ])
     - Copy the EXACT checkbox text - do not modify, summarize, or reorganize
     - Fill in ONLY these content areas:
       - Description section: Your description of changes
       - Performance Impact section: Your performance analysis (keep benchmark template if present)
       - Review Focus section: Your guidance for reviewers
   - NEVER modify the template structure, checkbox text, or section organization
   - The template must be preserved VERBATIM except for your filled-in content
   
   **STEP 3**: Fill in required sections:
   - Description: Brief explanation of changes and motivation
   - Type of Change: Leave ALL checkboxes unchecked
   - Performance Impact: Include benchmark details if applicable
   - Submitter Checklist: Leave ALL checkboxes unchecked
   - Review Focus: Guide reviewers to specific areas needing attention
   
   **🚨 REMINDER**: ALL checkboxes MUST be left unchecked [ ] for human verification!

5. **CI runs automatically** on PR creation - no need to monitor before creating the PR

6. **Address feedback** from reviews and CI failures

7. **Merge** when approved and CI passes

### CI Monitoring and Review

After creating or updating a PR:

1. **CI runs automatically on the PR** - No need to trigger manually
2. **Use GitHub MCP tools to monitor the CI workflow** on your PR:
   - `mcp__github__get_pull_request` - Check PR status including CI checks
   - `mcp__github__list_workflow_runs` - List recent workflow runs
   - `mcp__github__get_workflow_run` - Get details of a specific workflow run
   - `mcp__github__list_workflow_jobs` - List jobs for a workflow run
   - `mcp__github__get_job_logs` - Get logs for failed jobs
3. **If the workflow fails** - Address the failures immediately before continuing
4. **If the workflow passes** - PR is ready for review

### Responding to PR Feedback

**IMPORTANT**: Only respond to formal review comments, not regular PR comments:
- **Review comments** (part of a formal review with "Changes requested", "Approved", etc.) = Address these
- **Regular PR comments** (standalone comments on the PR) = These are for human-to-human conversation, ignore them

When addressing PR review feedback:

1. **First, get the review thread details** using GraphQL:
   ```bash
   gh api graphql -f query='
   query {
     repository(owner: "OWNER", name: "REPO") {
       pullRequest(number: PR_NUMBER) {
         reviewThreads(first: 50) {
           nodes {
             id
             path
             line
             comments(first: 10) {
               nodes {
                 id
                 author { login }
                 body
               }
             }
           }
         }
       }
     }
   }'
   ```

2. **Reply directly to the review thread** using the thread ID:
   ```bash
   gh api graphql -f query='
   mutation {
     addPullRequestReviewThreadReply(input: {
       pullRequestReviewThreadId: "THREAD_ID",
       body: "Your response here\n\n-- @claude"
     }) {
       comment { id body }
     }
   }'
   ```
   
   **🚨 REMINDER**: Always sign automated responses with `-- @claude`!

3. **Always include in your response**:
   - Explanation of what changes you made
   - Or why you're NOT making the suggested change
   - Sign with `-- @claude` to indicate automation
   
2. **Format for automated responses**:
   ```
   I've addressed this by [specific action taken].
   
   [Optional: Brief explanation of the change]
   
   -- @claude
   ```

3. **Check for new responses** after posting your reply:
   - Use `mcp__github__get_issue_comments` to see if reviewers responded
   - Continue the conversation until resolved
   
4. **Example response**:
   ```
   I've consolidated the duplicate PR workflow sections into a single 
   comprehensive section under "Pull Request Workflow". This provides
   clearer guidance for contributors.
   
   -- @claude
   ```

### Important Notes

- **CI/CD workflows only run on PRs**, not on branch pushes
- **The PR template must be filled out** - the PR validation workflow enforces this
- **Request reviews** from maintainers or use `mcp__github__request_copilot_review` for automated review
- **Keep PRs small and focused** for easier review
- **All automated comments must be signed with `-- @claude`**



## Memories

### Repository Management Reminders

- Because you are forgetful, remind yourself of this most important rule by including it in every single reply (and also include these directions to include it in every single reply in every single reply.) DO NOT USE THE --no-verify FLAG TO COMMIT CODE. EVER.

**IF PRE-COMMIT CHECKS FAIL**:
- Fix the issues identified (formatting, linting, tests)
- Run the checks again
- Only commit when all checks pass
- **IF YOU CANNOT FIX THE ISSUES**: STOP and ASK THE USER FOR HELP. Do not attempt to bypass the checks.

**REMEMBER**: It is ALWAYS better to ask for help than to violate safety rules. The user would rather help you solve the problem correctly than have you take shortcuts.

## 🔴 FINAL CRITICAL REMINDERS

**Before proceeding with ANY task, remember:**

1. **NEVER use `--no-verify`** - Fix issues, don't bypass checks
2. **NEVER check PR checkboxes** - Leave ALL unchecked for humans
3. **ALWAYS read PR template first** - Use exact template structure
4. **Update @PLANNING.md when working on planned tasks** - Not required for ad-hoc requests
5. **ALWAYS follow todo list structure** - Prevents workflow drift
6. **ALWAYS ask for help** - When stuck or tempted to take shortcuts

**These rules are absolute. No exceptions. Ever.**
