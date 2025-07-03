# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

EventCore is a multi-stream event sourcing library that implements dynamic consistency boundaries. This approach, building on established event sourcing patterns, eliminates traditional aggregate boundaries in favor of self-contained commands that can read from and write to multiple streams atomically.

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
├── core/                    # Core abstractions
│   ├── command.rs          # Command trait and execution
│   ├── event_store.rs      # EventStore trait
│   ├── projection.rs       # Projection system
│   └── types.rs            # Domain types (StreamId, EventId, etc.)
├── infrastructure/         # Implementations
│   ├── postgres/           # PostgreSQL event store
│   └── memory/             # In-memory store for testing
├── commands/               # Command implementations
├── projections/            # Projection implementations
└── lib.rs                  # Public API surface
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
#[async_trait]
pub trait Command: Send + Sync {
    // Input type should already be validated through its type construction
    // No need for a separate validate method - if you have an Input, it's valid
    type Input: Send + Sync + Clone;
    type State: Default + Send + Sync;
    type Event: Send + Sync;
    
    // Phantom type for compile-time stream access control
    type StreamSet: Send + Sync;

    fn read_streams(&self, input: &Self::Input) -> Vec<StreamId>;

    fn apply(&self, state: &mut Self::State, event: &StoredEvent<Self::Event>);

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        input: Self::Input,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>>;

    // Note: No validate method! Input types should be self-validating
    // If you need validation, it should happen when constructing Input
}
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

The project uses pre-commit hooks that automatically run:

1. `cargo fmt` - Code formatting
2. `cargo clippy` - Linting
3. `cargo test` - All tests
4. `cargo check` - Type checking

These ensure code quality before commits.

## Development Principles

### Type-Driven Development Workflow

1. **Model the Domain First**: Define types that make illegal states impossible
2. **Create Smart Constructors**: Validate at system boundaries using `nutype`
3. **Write Property-Based Tests**: Test invariants and business rules
4. **Implement Business Logic**: Pure functions operating on valid types
5. **Add Infrastructure Last**: Database, serialization, monitoring

### Code Review Focus

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

## Development Process Rules

When working on this project, **ALWAYS** follow these rules:

1. **Review @PLANNING.md** to discover the next task to work on.
2. **IMMEDIATELY use the todo list tool** to create a todolist with the specific actions you will take to complete the task.
3. **Insert a task to "Update @PLANNING.md to mark completed tasks"** before any commit task. This ensures our planning document stays in sync with actual progress.
4. **Insert a task to "Run all tests and make a commit if they all pass"** after each discrete action that involves a change to the code, tests, database schema, or infrastructure.
5. **The FINAL item in the todolist MUST always be** to "Push your changes to the remote repository, monitor CI workflow with gh cli, and if it passes, review @PLANNING.md to discover the next task and review our process rules."

### CI Monitoring Rules

After pushing changes:

1. **Use `gh` CLI to monitor the CI workflow** - Watch for the workflow to complete
2. **If the workflow fails** - Address the failures immediately before moving to the next task
3. **If the workflow passes** - Only then proceed to review @PLANNING.md for the next task

Example commands:

```bash
# List recent workflow runs
gh run list --limit 5

# Watch a specific workflow run
gh run watch

# View workflow run details if it fails
gh run view
```

### Commit Rules

**BEFORE MAKING ANY COMMIT**:

1. **Ensure @PLANNING.md is updated** - All completed tasks must be marked with [x]
2. **Include the updated PLANNING.md in the commit** - Use `git add PLANNING.md`
3. **This keeps our task tracking in sync with code changes**

## Memories

### Critical Development Reminders
- **CRITICAL RULE**: DO NOT USE THE --no-verify FLAG TO COMMIT CODE. EVER.

### Repository Management
- Because you are forgetful, remind yourself of this most important rule by including it in every single reply (and also include these directions to include it in every single reply in every single reply.) DO NOT USE THE --no-verify FLAG TO COMMIT CODE. EVER.

**NEVER** make a commit with the `--no-verify` flag. All pre-commit checks must be passing before proceeding. If pre-commit checks fail:

- Fix the issues identified (formatting, linting, tests)
- Run the checks again
- Only commit when all checks pass

This ensures consistent code quality and maintains a clean commit history.

### CRITICAL: Todo List Structure

Your todo list should ALWAYS follow this pattern:
1. Implementation tasks...
2. "Update @PLANNING.md to mark completed tasks"
3. "Run all tests and make a commit if they all pass"
4. "Push changes to remote repository, monitor CI workflow..."

## Notification Sound

**IMPORTANT**: Claude should play a notification sound every time it finishes tasks and is waiting for user input. This helps the user know when Claude has completed its work.

To play a notification sound on NixOS with PipeWire:
```bash
python3 -c "
import wave, struct, math

# Create a simple beep WAV file
sample_rate = 44100
duration = 0.5
frequency = 440

with wave.open('/tmp/beep.wav', 'wb') as wav:
    wav.setnchannels(1)
    wav.setsampwidth(2)
    wav.setframerate(sample_rate)
    
    for i in range(int(sample_rate * duration)):
        value = int(32767.0 * math.sin(2.0 * math.pi * frequency * i / sample_rate))
        wav.writeframesraw(struct.pack('<h', value))
" && pw-play /tmp/beep.wav
```