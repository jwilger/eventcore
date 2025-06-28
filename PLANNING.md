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

## Phase 1: CI/CD and Project Setup

### 1.1 GitHub Actions Setup

- [x] Create `.github/workflows/ci.yml`
  - [x] Run tests on all commits
  - [x] Check formatting with `cargo fmt`
  - [x] Run clippy with strict settings
  - [x] Check for security vulnerabilities
  - [x] Generate test coverage reports

### 1.2 Project Configuration

- [x] Create workspace `Cargo.toml` at root
  - [x] Define workspace members:
    - `eventcore` (core library)
    - `eventcore-postgres` (PostgreSQL adapter)
    - `eventcore-memory` (in-memory adapter for testing)
    - `eventcore-examples` (example implementations)
  - [x] Configure shared dependencies
  - [x] Configure linting rules
  - [x] Configure optimization profiles
- [x] Create `.gitignore` for Rust projects
- [x] Set up pre-commit hooks locally
- [x] Configure cargo audit to ignore false positives (`.cargo/audit.toml`)
- [x] Create README.md for workspace metadata requirements

### 1.3 Development Tooling

- [x] Configure `rust-toolchain.toml` (already exists)
- [x] Set up `cargo-nextest` for faster test runs
- [x] Configure `cargo-llvm-cov` for coverage
- [x] Add `justfile` for common commands

### 1.4 Workspace Structure Setup

- [x] Create crate directories:
  - [x] `eventcore/` - Core library with traits and types
  - [x] `eventcore-postgres/` - PostgreSQL adapter crate
  - [x] `eventcore-memory/` - In-memory adapter crate
  - [x] `eventcore-examples/` - Examples crate
- [x] Create individual `Cargo.toml` for each crate:
  - [x] `eventcore`: No database dependencies
  - [x] `eventcore-postgres`: Only PostgreSQL dependencies (sqlx)
  - [x] `eventcore-memory`: Minimal dependencies
  - [x] `eventcore-examples`: Depends on core and adapters

## Phase 2: Core Type System Foundation

### 2.1 Core Event Sourcing Types

- [x] Create `eventcore/src/types.rs`
  - [x] Define `StreamId` with validation (non-empty, max 255 chars)
  - [x] Define `EventId` ensuring UUIDv7 format
  - [x] Define `EventVersion` (non-negative integer)
  - [x] Define `Timestamp` wrapper around chrono::DateTime
  - [x] Write property tests for all type constructors
  - [x] Verify smart constructors reject invalid inputs

### 2.2 Error Modeling

- [x] Create `eventcore/src/errors.rs`
  - [x] Define `CommandError` enum with all variants
  - [x] Define `EventStoreError` enum
  - [x] Define `ProjectionError` enum
  - [x] Define `ValidationError` for smart constructor failures
  - [x] Implement `From` traits for error conversions
  - [x] Write tests ensuring all errors are properly constructed

### 2.3 Event Metadata Types

- [x] Create `eventcore/src/metadata.rs`
  - [x] Define `EventMetadata` struct
  - [x] Define `CausationId` and `CorrelationId` types
  - [x] Define `UserId` type for actor information
  - [x] Implement builders for metadata construction
  - [x] Write property tests for metadata serialization

## Phase 3: Command System Design

### 3.1 Command Trait Definition

- [x] Create `eventcore/src/command.rs`
  - [x] Define `Command` trait with associated types
  - [x] Add `read_streams` method signature (stub with `todo!()`)
  - [x] Add `apply` method signature for event folding
  - [x] Add `handle` method signature for business logic
  - [x] Create `CommandResult<T>` type alias
  - [x] Note: No validate method - Input types must be self-validating

### 3.2 Command Executor Design

- [ ] Create `eventcore/src/executor.rs`
  - [ ] Define `CommandExecutor` struct
  - [ ] Add `execute` method signature (stub with `todo!()`)
  - [ ] Add `execute_with_retry` method signature
  - [ ] Define retry configuration types
  - [ ] Write property tests for retry logic invariants

### 3.3 Command Infrastructure

- [ ] Create `eventcore/src/command_executor.rs`
  - [ ] Define retry configuration types
  - [ ] Define command context types
  - [ ] Create command builder utilities
  - [ ] Define command metadata types
  - [ ] Write tests for command infrastructure

## Phase 4: Event Store Abstraction

### 4.1 Event Store Trait (Adapter Pattern)

- [ ] Create `eventcore/src/event_store.rs`
  - [ ] Define `EventStore` trait as the port interface
  - [ ] Add `read_streams` method signature
  - [ ] Add `write_events_multi` method signature
  - [ ] Add `stream_exists` method signature
  - [ ] Add `get_stream_version` method signature
  - [ ] Define `StreamData<E>` type
  - [ ] Make trait async and Send + Sync
  - [ ] Design for backend independence

### 4.2 Event Types

- [ ] Create `eventcore/src/event.rs`
  - [ ] Define `Event<E>` struct with generic payload
  - [ ] Define `StoredEvent` for persistence
  - [ ] Implement event ordering using EventId
  - [ ] Write property tests for event ordering invariants

### 4.3 Subscription System

- [ ] Create `eventcore/src/subscription.rs`
  - [ ] Define `Subscription` trait
  - [ ] Define `SubscriptionOptions` enum
  - [ ] Add subscription position tracking types
  - [ ] Stub subscription implementation

### 4.4 Event Store Adapter Interface

- [ ] Create `eventcore/src/event_store_adapter.rs`
  - [ ] Define connection configuration traits
  - [ ] Define backend-specific error mapping
  - [ ] Create adapter lifecycle management
  - [ ] Design feature flags for optional backends

## Phase 5: Test Infrastructure

### 5.1 In-Memory Event Store Adapter

- [ ] Create `eventcore-memory/src/lib.rs`
  - [ ] Implement `EventStore` trait for testing
  - [ ] Add thread-safe storage using Arc<RwLock<\_>>
  - [ ] Implement version tracking per stream
  - [ ] No persistence (for testing only)
  - [ ] Write comprehensive unit tests

### 5.2 Test Utilities

- [ ] Create `eventcore/src/testing/mod.rs`
  - [ ] Command test harness builder
  - [ ] Event builder utilities
  - [ ] Assertion helpers for domain types
  - [ ] Property test generators using `proptest`

### 5.3 Property Test Suite

- [ ] Create `tests/properties/mod.rs`
  - [ ] "Events are immutable" property
  - [ ] "Stream versions monotonically increase" property
  - [ ] "Event ordering is deterministic" property
  - [ ] "Commands are idempotent" property
  - [ ] "Concurrent commands maintain consistency" property

## Phase 6: Command Implementation

### 6.1 Command Input Type Design

- [ ] Design command input types with built-in validation
  - [ ] Use smart constructors that parse raw input into valid types
  - [ ] Ensure all command inputs are self-validating through their types
  - [ ] Write property tests that valid inputs can be constructed
  - [ ] Write tests that invalid raw data is rejected at construction

### 6.2 State Reconstruction

- [ ] Implement event folding logic
  - [ ] Create `apply` implementations for each command
  - [ ] Ensure state mutations are immutable
  - [ ] Write property tests for state consistency

### 6.3 Command Execution Logic

- [ ] Implement generic command execution flow
  - [ ] Stream reading and merging logic
  - [ ] State reconstruction from events
  - [ ] Version tracking for concurrency control
  - [ ] Error propagation and handling
  - [ ] Write comprehensive unit tests

### 6.4 Command Executor Implementation

- [ ] Implement `CommandExecutor::execute`
  - [ ] Read streams specified by command
  - [ ] Reconstruct state by folding events
  - [ ] Execute command business logic
  - [ ] Handle optimistic concurrency control
  - [ ] Write integration tests with in-memory store

## Phase 7: Projection System

### 7.1 Projection Trait

- [ ] Create `eventcore/src/projection.rs`
  - [ ] Define `Projection` trait
  - [ ] Add state management methods
  - [ ] Add checkpointing support
  - [ ] Define `ProjectionResult<T>` type alias

### 7.2 Projection Manager

- [ ] Create `eventcore/src/projection_manager.rs`
  - [ ] Define `ProjectionManager` struct
  - [ ] Add `start`, `pause`, `resume` methods
  - [ ] Add rebuild functionality
  - [ ] Implement health monitoring

### 7.3 Projection Infrastructure

- [ ] Create `eventcore/src/projection_runner.rs`
  - [ ] Implement event subscription handling
  - [ ] Add checkpoint management
  - [ ] Implement error recovery
  - [ ] Write tests for projection reliability

## Phase 8: PostgreSQL Adapter Crate

### 8.1 PostgreSQL Crate Setup

- [ ] Create `eventcore-postgres/Cargo.toml`
  - [ ] Depend on `eventcore` crate
  - [ ] Add `sqlx` with PostgreSQL features
  - [ ] Add `tokio` for async runtime
  - [ ] Configure as separate publishable crate

### 8.2 PostgreSQL Adapter Structure

- [ ] Create `eventcore-postgres/src/lib.rs`
  - [ ] PostgreSQL-specific configuration types
  - [ ] Connection pool management
  - [ ] Error mapping from sqlx to EventStoreError
  - [ ] Public API exports

### 8.3 Database Schema

- [ ] Create `eventcore-postgres/migrations/`
  - [ ] Design event_streams table migration
  - [ ] Design events table migration
  - [ ] Add necessary indexes
  - [ ] Create partitioning strategy

### 8.4 PostgreSQL Event Store Implementation

- [ ] Create `eventcore-postgres/src/event_store.rs`
  - [ ] Implement `EventStore` trait for PostgreSQL
  - [ ] Use `sqlx` for database operations
  - [ ] Implement optimistic concurrency with transactions
  - [ ] Map PostgreSQL errors to EventStoreError

### 8.5 PostgreSQL Adapter Tests

- [ ] Create integration tests with real PostgreSQL
  - [ ] Test concurrent command execution
  - [ ] Test multi-stream atomicity
  - [ ] Verify transaction isolation
  - [ ] Performance benchmarks

## Phase 9: Serialization & Persistence

### 9.1 Event Serialization

- [ ] Create `eventcore/src/serialization/mod.rs`
  - [ ] Define `EventSerializer` trait
  - [ ] Implement JSON serialization
  - [ ] Support schema evolution
  - [ ] Write round-trip property tests

### 9.2 Type Registry

- [ ] Create `eventcore/src/type_registry.rs`
  - [ ] Map event type names to Rust types
  - [ ] Support dynamic deserialization
  - [ ] Handle unknown event types gracefully

## Phase 10: Monitoring & Observability

### 10.1 Metrics Collection

- [ ] Create `eventcore/src/monitoring/metrics.rs`
  - [ ] Define metrics types (Counter, Gauge, Timer)
  - [ ] Add command execution metrics
  - [ ] Add event store operation metrics
  - [ ] Add projection lag metrics

### 10.2 Health Checks

- [ ] Create `eventcore/src/monitoring/health.rs`
  - [ ] Event store connectivity check
  - [ ] Projection status checks
  - [ ] Memory usage monitoring
  - [ ] Define health check API

### 10.3 Structured Logging

- [ ] Integrate `tracing` throughout codebase
  - [ ] Add spans for command execution
  - [ ] Log all errors with context
  - [ ] Include correlation IDs

## Phase 11: Performance & Benchmarks

### 11.1 Benchmark Suite

- [ ] Create `benches/` directory
  - [ ] Command execution benchmarks
  - [ ] Event store read/write benchmarks
  - [ ] Projection processing benchmarks
  - [ ] Memory allocation profiling

### 11.2 Performance Optimization

- [ ] Profile and optimize hot paths
  - [ ] Minimize allocations in event processing
  - [ ] Optimize database queries
  - [ ] Add caching where appropriate
  - [ ] Verify against performance targets

## Phase 12: Public API & Documentation

### 12.1 Library Public API

- [ ] Create `eventcore/src/lib.rs` with clean exports
  - [ ] Export core traits and types
  - [ ] Export command creation helpers
  - [ ] Export test utilities
  - [ ] Hide implementation details
- [ ] Document crate usage patterns:
  - [ ] How to depend on core + adapter crates
  - [ ] Example Cargo.toml configurations
  - [ ] Adapter selection and initialization

### 12.2 Documentation

- [ ] Write comprehensive rustdoc comments
  - [ ] Document all public types and traits
  - [ ] Add usage examples in doc comments
  - [ ] Create module-level documentation
  - [ ] Generate and review HTML docs

### 12.3 Examples Crate

- [ ] Create `eventcore-examples/Cargo.toml`
  - [ ] Depend on `eventcore` core crate
  - [ ] Depend on `eventcore-postgres` for examples
  - [ ] Depend on `eventcore-memory` for tests
- [ ] Create `eventcore-examples/src/` structure:
  - [ ] Banking transfer example (`banking/`)
    - [ ] Define `Money` type with validation
    - [ ] Define `AccountId` and `TransferId` types
    - [ ] Implement `TransferMoney` command
    - [ ] Implement `OpenAccount` command
    - [ ] Create account balance projection
  - [ ] E-commerce order example (`ecommerce/`)
    - [ ] Define order-specific types
    - [ ] Implement order workflow commands
    - [ ] Create inventory projection
  - [ ] Long-running saga example (`sagas/`)
  - [ ] Performance testing example (`benchmarks/`)

## Phase 13: Additional Event Store Adapters (Future)

### 13.1 EventStoreDB Adapter Crate

- [ ] Create `eventcore-eventstoredb/` crate
  - [ ] Separate `Cargo.toml` with EventStoreDB client
  - [ ] Implement EventStore trait for EventStoreDB
  - [ ] Map EventStoreDB-specific features
  - [ ] Integration tests
  - [ ] Publish as separate crate

### 13.2 Other Potential Adapters

- [ ] Document adapter implementation guide
  - [ ] Required trait implementations
  - [ ] Testing requirements
  - [ ] Performance benchmarks
  - [ ] Example adapter skeleton

## Phase 14: Integration & Polish

### 14.1 Release Preparation

- [ ] Create README.md for each crate:
  - [ ] `eventcore` - Core library documentation
  - [ ] `eventcore-postgres` - PostgreSQL adapter docs
  - [ ] `eventcore-memory` - Testing adapter docs
  - [ ] `eventcore-examples` - Example usage docs
- [ ] Add comprehensive CHANGELOG.md
- [ ] Define semantic versioning strategy
- [ ] Create migration guides
- [ ] Publishing strategy:
  - [ ] Publish `eventcore` core crate first
  - [ ] Publish adapter crates with version alignment
  - [ ] Use workspace versioning for consistency

### 14.2 Final Testing

- [ ] Comprehensive integration test suite
  - [ ] Stress testing with concurrent operations
  - [ ] Memory leak detection
  - [ ] Security audit
  - [ ] Performance validation against PRD targets

## Success Criteria

Each phase is complete when:

1. All types compile with no `todo!()` remaining
2. All property tests pass
3. Unit test coverage > 95%
4. Integration tests verify the complete flow
5. Documentation is complete and accurate
6. Performance meets PRD requirements

## Notes on Type-Driven Development Process

For each component:

1. **Design types first** - Make illegal states unrepresentable
2. **Create stubs** - All functions return `todo!()`
3. **Write property tests** - Define invariants that must hold
4. **Write unit tests** - Test specific behaviors
5. **Implement** - Replace `todo!()` guided by failing tests
6. **Refactor** - Improve implementation while tests pass
7. **Document** - Add rustdoc with examples

This approach ensures we think through the design before coding and that all code is tested from the start.

## Development Process Rules

When working on this project, **ALWAYS** follow these rules:

1. **Review @PLANNING.md** to discover the next task to work on.
2. **IMMEDIATELY use the todo list tool** to create a todolist with the specific actions you will take to complete the task.
3. **ALWAYS include "Update @PLANNING.md to mark completed tasks" in your todolist** - This task should come BEFORE the commit task to ensure completed work is tracked.
4. **Insert a task to "Run all tests and make a commit if they all pass"** after each discrete action that involves a change to the code, tests, database schema, or infrastructure.
5. **The FINAL item in the todolist MUST always be** to "Push your changes to the remote repository, monitor CI workflow with gh cli, and if it passes, review @PLANNING.md to discover the next task and review our process rules."

### CRITICAL: Todo List Structure

Your todo list should ALWAYS follow this pattern:
1. Implementation tasks...
2. "Update @PLANNING.md to mark completed tasks"
3. "Run all tests and make a commit if they all pass"
4. "Push changes to remote repository, monitor CI workflow..."

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

**NEVER** make a commit with the `--no-verify` flag. All pre-commit checks must be passing before proceeding. If pre-commit checks fail:
- Fix the issues identified (formatting, linting, tests)
- Run the checks again
- Only commit when all checks pass
