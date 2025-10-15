# EventCore Technical Increment Plan

**Document Version:** 3.0 (Progressive Disclosure Restructure)
**Date:** 2025-10-14
**Project:** EventCore
**Phase:** 6 - Technical Increment Planning (Progressive Disclosure)
**Workflow:** Infrastructure Library Development

**Version 3.0 Changes:**
- SPLIT I-008 (Event Subscriptions) → I-008 (Basic Subscriptions) + I-009 (Checkpointing)
- MOVED Snapshot Support from I-009 → I-012 (after Performance Benchmarking)
- SPLIT Macros from I-012 → I-013 (require!) + I-014 (emit!)
- RESTRUCTURED Documentation from I-013 → I-015 (Audit, not big bang)
- RESTRUCTURED Error Messages from I-014 → I-016 (Audit, not polish at end)
- **Total:** 16 increments (was 14) with better progressive disclosure

## Overview

This document outlines the technical increment plan for developing EventCore, a type-safe event sourcing library implementing multi-stream atomicity with dynamic consistency boundaries.

**CRITICAL RESTRUCTURE:** This plan has been completely restructured from horizontal layers (all types, then all storage, then all commands) to **proper vertical slices** where each increment is an end-to-end feature testable from the library consumer's perspective.

**Core Value Proposition:** EventCore eliminates the artificial constraints of traditional event sourcing by enabling atomic multi-stream operations while maintaining type safety, strong consistency guarantees, and developer ergonomics.

**Development Philosophy:**
- **Type-Driven Development:** Types enforce domain constraints at compile time FROM INCREMENT 1
- **Correctness Over Performance:** Multi-stream atomicity is non-negotiable
- **Infrastructure Neutrality:** Library, not framework - no business domain assumptions
- **Developer Ergonomics:** Minimize boilerplate while maximizing type safety
- **Vertical Slices:** Each increment provides end-to-end value developers can integrate immediately

## Vertical Slice Principles

**What Makes This Different:**

1. **End-to-End from Library Consumer POV**
   - Each increment = something a developer can actually use in their application
   - Must be testable as integration test from consumer perspective
   - Must provide real, working functionality (even if limited)

2. **Domain Types and Error Handling Are NOT Features**
   - Validated domain types (nutype) included from increment 1
   - Proper error handling (thiserror, Result types) included from increment 1
   - These are foundational discipline, not optional "features to add later"
   - We don't write with String primitives and "add types later"

3. **Simplest Possible First Slice**
   - I-001: Developer can create single-stream command with validated types, execute it with proper error handling, events written to in-memory storage
   - Includes: StreamId, EventId validated types; Event/EventMetadata; structured errors; InMemoryEventStore; single-stream executor
   - Excludes: retry logic, multi-stream, PostgreSQL, macros, dynamic discovery (all come later)

4. **Each Subsequent Slice Adds ONE Capability**
   - I-002: Add automatic retry with sensible defaults (no configuration)
   - I-003: Add retry configuration and advanced observability
   - I-004: Add multi-stream atomic commands (THE core value prop)
   - I-005: Add PostgreSQL production backend
   - I-006: Add macro ergonomics
   - I-007: Add dynamic stream discovery
   - etc.

## Increment Organization

Increments are organized as **end-to-end vertical slices** that library consumers can integrate and use:

- **I-001: Single-Stream Command (Complete)** - Developer can execute single-stream command with validated types and proper error handling (NO retry - manual ConcurrencyError handling)
- **I-002: Automatic Retry with Defaults** - Automatic retry on conflicts using hardcoded sensible defaults (no configuration needed)
- **I-003: Configurable Retry Policies** - Custom retry configuration and advanced observability for production tuning
- **I-004: Multi-Stream Atomicity** - Core value prop: atomic operations across multiple streams
- **I-005: PostgreSQL Production Backend** - Production storage with ACID transactions
- **I-006: Command Derive Macro** - Eliminate boilerplate with #[derive(Command)]
- **I-007: Dynamic Stream Discovery** - State-dependent stream requirements
- **I-008: Basic Event Subscriptions** - Subscribe to event streams and process events (poll or callback based)
- **I-009: Checkpointing for Subscriptions** - Add checkpoint/resume capability for projections
- **I-010: Chaos Testing Infrastructure** - Failure injection for robust testing
- **I-011: Performance Benchmarking** - Establish baselines and track regressions
- **I-012: Snapshot Support** - Optimize long-lived stream reconstruction based on performance data
- **I-013: require! Macro** - Ergonomic business rule validation
- **I-014: emit! Macro** - Type-safe event emission with phantom types
- **I-015: Documentation Completeness Audit** - Ensure documentation quality and consistency across increments
- **I-016: Error Message Consistency Audit** - Ensure error message quality and consistency across increments

**Critical Principle:** Each increment is a complete vertical slice testable from library consumer perspective. Domain types and error handling are included from I-001 (not separate increments).

Each increment:
- Provides end-to-end functionality developers can integrate immediately
- Testable as integration test from consumer perspective
- Includes all infrastructure needed (types, errors, storage, execution)
- Builds incrementally on previous work

---

## I-001: Single-Stream Command End-to-End

### Purpose

Enable library consumer (application developer) to create and execute a complete single-stream command with validated domain types, proper error handling, and in-memory event storage.

**What This Provides:** A working, testable command execution system that developers can integrate into their applications immediately. Even though limited to single streams, it's a real, functional event sourcing system.

### What's Included

This increment includes ALL infrastructure needed for end-to-end single-stream command execution:

**Domain Types (with validation):**
- StreamId, EventId, CorrelationId, CausationId (all using `nutype`)
- Event and EventMetadata structures
- Parse-don't-validate pattern throughout

**Error Handling:**
- Structured error hierarchy (EventStoreError, CommandError, ValidationError, ConcurrencyError)
- Error classification (Retriable vs Permanent)
- Full error context with `thiserror`

**Storage:**
- InMemoryEventStore implementing EventStore trait
- Version-based optimistic concurrency control
- Atomic single-stream append operations

**Command System:**
- CommandStreams and CommandLogic traits (manual implementation, no macro yet)
- Command execution context for event emission
- State reconstruction via apply()
- Business logic via handle()

**Executor:**
- CommandExecutor orchestrating read → apply → handle → write
- NO retry logic - returns ConcurrencyError on version conflicts
- Correlation/causation tracking

**Integration Test Example:**
- Complete BankAccount command (deposit/withdraw)
- Demonstrates type safety, error handling, version conflict detection

### Acceptance Criteria

```gherkin
Feature: Developer executes complete single-stream command end-to-end

Scenario: Developer implements and executes bank account command
  Given developer creates BankAccount command with StreamId using nutype
  When developer implements CommandLogic with apply() and handle()
  And developer creates InMemoryEventStore
  And developer creates CommandExecutor with store
  And developer executes Deposit(account_id, amount: 100)
  Then command succeeds
  And AccountDeposited event is stored with correct metadata
  And developer can query account balance from reconstructed state
  And balance equals 100

Scenario: Developer handles business rule violations with proper errors
  Given account has balance of 50
  When developer executes Withdraw command with amount 100
  Then CommandError::BusinessRuleViolation is returned
  And error message explains "insufficient funds: balance 50, withdrawal 100"
  And error includes context (account_id, current balance, attempted withdrawal)

Scenario: Developer handles version conflict manually
  Given developer executes two concurrent Deposit commands on same account
  When both commands read account at version 0
  And first command writes event, advancing to version 1
  And second command attempts write expecting version 1
  Then ConcurrencyError is returned to developer
  And developer must handle retry manually (or wait for I-002)
  And no automatic retry occurs
  And developer can inspect error details (expected vs actual version)
```

### Integration Requirements

**API Entry Point:** Library consumer imports core types, store, command traits, and executor from the eventcore crate.

**Integration Testing Approach:**
- Test complete end-to-end command execution from consumer perspective
- Verify type safety with validated domain types (StreamId, EventId, etc.)
- Test event storage and retrieval
- Verify state reconstruction from event history
- Test business rule enforcement through proper error types

**Manual Verification Steps:**
1. Create new Rust project: `cargo new example-app`
2. Add `eventcore = "0.1"` to Cargo.toml
3. Copy bank account example from documentation
4. Run `cargo test` - all tests pass
5. Implement custom command (e.g., TodoList)
6. Execute command, verify events stored and state reconstructed
7. Time to first working command: < 30 minutes for developer new to EventCore

**Documentation Requirements:**
- Getting Started guide with bank account example
- API docs for all public types with examples
- Integration test template developers can copy
- Error handling guide explaining Retriable vs Permanent

### Dependencies

None. This is the first increment and includes everything needed for basic functionality.

### What's Excluded (Comes Later)

- **Automatic retry:** Developer handles ConcurrencyError manually (I-002 adds automatic retry)
- **Retry configuration:** No retry logic yet (I-002 adds automatic retry, I-003 adds configuration)
- **Multi-stream commands:** Only single stream in this increment (I-004 adds multi-stream)
- **PostgreSQL backend:** Only in-memory storage (I-005 adds PostgreSQL)
- **Command derive macro:** Manual trait implementation (I-006 adds macro)
- **Dynamic stream discovery:** Static stream declarations only (I-007 adds discovery)
- **Event subscriptions:** Not yet implemented (I-008 adds basic subscriptions, I-009 adds checkpointing)
- **Snapshots:** Not yet implemented (I-012 adds after performance benchmarking)

### Technical Notes

**Type-Driven Development from Day 1:**
- All domain types use `nutype` for validation (no String primitives)
- Error types use `thiserror` for structured errors
- Result types throughout (no unwrap/expect in library code)
- Phantom types for compile-time safety (preparation for macro in I-005)

**Why In-Memory First:**
- Enables fast TDD for all subsequent increments
- Zero external dependencies for library consumers
- Perfect for testing and development
- Real EventStore implementation (not a mock)

**Performance Expectations:**
- In-memory operations: microseconds per command
- Adequate for testing, development, and small-scale production
- Version conflict detection: 100% accurate
- No retry - developer handles ConcurrencyError explicitly

### References

- **Requirements:** FR-1.1, FR-1.2, FR-2.1, FR-2.2, FR-2.3, FR-3.1, FR-3.2, FR-3.3, FR-4.1, FR-5.1, FR-5.2, FR-5.3
- **Architecture:** ARCHITECTURE.md sections on Command System, Executor, Type System, Error Handling
- **ADRs:** ADR-002 (EventStore trait), ADR-003 (Type system), ADR-004 (Error handling), ADR-007 (Optimistic concurrency), ADR-008 (Executor and retry)

---

## I-002: Automatic Retry with Sensible Defaults

### Purpose

Add automatic retry on version conflicts so developers don't have to handle ConcurrencyError manually. Use hardcoded sensible defaults that work for most cases.

**What This Adds:** Executor automatically retries commands when version conflicts occur, using exponential backoff with jitter. No configuration required.

**Key Design Decision:** This increment adds automatic retry with hardcoded defaults ONLY. Configuration comes later (I-003). This gives developers immediate value without requiring them to learn retry policies.

### What's Included

**Automatic Retry Logic:**
- Hardcoded exponential backoff: 10ms, 20ms, 40ms, 80ms, 160ms
- Hardcoded max attempts: 5 retries
- Jitter (random variation) to prevent thundering herd
- Re-reads stream state on each retry attempt

**Basic Observability:**
- Log retry attempts with attempt number and stream ID
- Log final outcome (success after N attempts or exhausted retries)

**Behavior Change from I-001:**
- ConcurrencyError now triggers automatic retry instead of being returned
- Developer no longer needs to handle retry logic manually
- Commands "just work" under typical contention

### What's Excluded (Comes Later)

- **Configurable retry policies:** Hardcoded defaults only (I-003 adds configuration)
- **RetryPolicy builder:** Not yet (I-003 adds this)
- **Custom backoff strategies:** Exponential only (I-003 adds linear/fixed)
- **Per-command retry tuning:** One policy for all commands (I-003 adds customization)
- **Advanced observability:** Basic logging only (I-003 adds metrics hooks, tracing integration)

### Acceptance Criteria

```gherkin
Feature: Developer benefits from automatic retry without configuration

Scenario: Developer executes command under contention
  Given developer creates executor (no retry configuration needed)
  When version conflict occurs during command execution
  Then executor automatically retries up to 5 times
  And uses exponential backoff (10ms, 20ms, 40ms, 80ms, 160ms)
  And eventually succeeds if conflict resolves
  And developer never sees ConcurrencyError for transient conflicts

Scenario: Developer observes retry attempts in logs
  Given developer enables logging
  When version conflict triggers retry
  Then log shows "Retry attempt 1/5 for stream account-123 after 10ms"
  And subsequent retries log with increasing delays
  And final log shows success: "Command succeeded after 3 retry attempts"

Scenario: Developer experiences automatic success under typical contention
  Given two concurrent commands modify same stream
  When both commands read stream at version 5
  And first command writes successfully, advancing to version 6
  And second command detects conflict
  Then second command automatically retries
  And reads stream at version 6
  And writes successfully at version 7
  And developer code doesn't handle retry manually

Scenario: Retries are exhausted under extreme contention
  Given command faces continuous conflicts
  When all 5 retry attempts fail
  Then ConcurrencyError is returned to developer
  And error message explains "Exhausted 5 retry attempts"
  And developer can handle this edge case explicitly

Scenario: Jitter prevents thundering herd
  Given 10 concurrent commands conflict on same stream
  When all commands retry simultaneously
  Then jitter adds random delay (±20% of backoff)
  And commands retry at slightly different times
  And reduces probability of repeated conflicts
```

### Integration Requirements

**Behavioral Evolution from I-001:**
- I-001: ConcurrencyError returned to developer immediately
- I-002: ConcurrencyError triggers automatic retry (up to 5 attempts)
- This is a breaking behavior change but vastly improves developer experience
- Developer can still see exhausted retry errors if contention is extreme

**Implementation Approach:**
- Add retry loop to CommandExecutor around execute() method
- Catch ConcurrencyError and retry with exponential backoff
- Re-read stream state on each retry (fresh data)
- Max 5 attempts then return ConcurrencyError with retry context

**Testing Requirements:**
- Integration tests with injected conflicts verifying retry behavior
- Verify backoff timing (approximately 10ms, 20ms, 40ms...)
- Verify jitter adds randomness (not exact delays)
- Verify exhausted retries return proper error

**Documentation Requirements:**
- Explain behavior change from I-001 (automatic vs manual retry)
- Document hardcoded defaults (5 attempts, exponential backoff)
- Note that I-003 will add configuration for advanced tuning
- Guide for when retry exhaustion occurs (increase capacity or partition streams)

### Dependencies

I-001 (single-stream command with ConcurrencyError detection)

### References

- **Requirements:** FR-3.3 (Automatic Retry), NFR-1.2 (Latency)
- **Architecture:** ARCHITECTURE.md Executor section
- **ADRs:** ADR-008 (Command Executor and Retry Logic)

---

## I-003: Configurable Retry Policies

### Purpose

Enable library consumers to customize retry behavior for their specific workloads and observe retry patterns through metrics and tracing.

**What This Adds:** RetryPolicy configuration with custom max attempts, backoff strategies, and advanced observability. Developers can tune retry for specific contention patterns while defaults from I-002 work for most cases.

### What's Included

**RetryPolicy Configuration:**
- RetryPolicy builder pattern for custom configuration
- Configurable max attempts (override default 5)
- Configurable backoff strategies: exponential (default), linear, fixed
- Configurable backoff base delay and multiplier
- Per-executor policy (different policies for different executors)

**Advanced Observability:**
- Integration with `tracing` crate for distributed tracing
- Metrics hooks compatible with Prometheus (retry rates, success after N attempts)
- Structured log format with correlation ID
- Retry attempt context in error messages

**Testing Utilities:**
- RetryPolicy with max_attempts=0 for disabling retry in tests
- Chaos testing support to inject conflicts systematically
- Benchmarks showing retry impact on latency

### What's Excluded (Comes Later)

- **Per-command policies:** One policy per executor (not per command type)
- **Dynamic policy adjustment:** Static policy at executor creation

### Acceptance Criteria

```gherkin
Feature: Developer customizes retry behavior for deployment environment

Scenario: Developer uses default retry policy
  Given developer creates executor without explicit RetryPolicy
  When version conflict occurs
  Then executor uses defaults from I-002 (5 attempts, exponential backoff)
  And developer benefits from sensible defaults without configuration

Scenario: Developer configures custom max attempts
  Given developer creates RetryPolicy::builder().max_attempts(10).build()
  When developer creates executor with custom policy
  Then executor retries up to 10 times on conflicts
  And backoff strategy remains exponential (default)

Scenario: Developer configures linear backoff
  Given developer creates RetryPolicy::builder().backoff_strategy(Linear).build()
  When version conflict triggers retry
  Then executor uses linear backoff (10ms, 20ms, 30ms, 40ms...)
  And developer can tune for predictable timing

Scenario: Developer disables retry for testing
  Given developer creates RetryPolicy::builder().max_attempts(0).build()
  When version conflict occurs
  Then executor fails immediately without retry
  And test can verify conflict detection logic

Scenario: Developer observes retry metrics
  Given developer integrates with Prometheus metrics
  When commands execute under contention
  Then metrics track retry_attempts_total, retry_success_after_n_attempts
  And developer can monitor production contention patterns

Scenario: Developer uses distributed tracing
  Given developer enables tracing integration
  When version conflict triggers retry
  Then each retry attempt creates span with context
  And correlation ID links retries to original command
  And developer can diagnose contention across services
```

### Integration Requirements

**API Enhancement Approach:**
- RetryPolicy builder with fluent interface
- Builder methods: max_attempts(), backoff_strategy(), base_delay(), multiplier()
- Executor accepts optional RetryPolicy (defaults to I-002 hardcoded policy)
- Policy is immutable after creation

**Migration Path from I-002:**
- I-002 behavior unchanged if no RetryPolicy provided
- Explicit policy overrides defaults
- Backward compatible - no breaking changes

**Observability Integration:**
- `tracing` spans around retry attempts with metadata
- Metrics hooks called on retry attempt and final outcome
- Error messages include retry context (attempted N times)

**Testing Requirements:**
- Integration tests with different retry policies
- Chaos tests verifying retry behavior under injected conflicts
- Benchmarks comparing exponential vs linear vs fixed backoff
- Verify max_attempts=0 disables retry completely

### Dependencies

I-002 (automatic retry with defaults)

### References

- **Requirements:** FR-3.3 (Automatic Retry), NFR-1.2 (Latency), NFR-2.2 (Composability)
- **Architecture:** ARCHITECTURE.md Executor section
- **ADRs:** ADR-008 (Command Executor and Retry Logic)

---

## I-004: Multi-Stream Atomic Commands

### Purpose

Enable library consumers to create commands that atomically read from and write to multiple event streams - THE core value proposition of EventCore.

**What This Adds:** Commands can declare multiple streams, read from all declared streams, and write to multiple streams atomically. All-or-nothing semantics across any number of streams.

### What's Included

**Multi-Stream Command Support:**
- Commands declare multiple stream IDs
- Executor reads all declared streams before handle()
- State reconstruction from events across multiple streams
- Atomic write to all streams with version checking for each

**Enhanced InMemoryEventStore:**
- Atomic append across multiple streams
- Version checking for all streams in single operation
- All-or-nothing guarantee (no partial writes)

**Concurrency Testing:**
- Property-based tests with concurrent multi-stream commands
- Verification that NO partial state is ever observable
- Stress tests with high contention on multiple streams

**Example Implementation:**
- TransferMoney command demonstrating atomic debit/credit
- Shows value over saga pattern (simpler, consistent)

### Acceptance Criteria

```gherkin
Feature: Developer creates atomic multi-stream commands

Scenario: Developer implements money transfer command
  Given developer creates TransferMoney command with two StreamId fields
  When developer implements CommandLogic with apply() and handle()
  Then apply() receives events from both account streams
  And handle() can emit events to both streams
  And executor writes both events atomically

Scenario: Developer executes successful transfer
  Given account A has balance 100
  And account B has balance 50
  When developer executes TransferMoney(from: A, to: B, amount: 30)
  Then account A balance becomes 70
  And account B balance becomes 80
  And both updates are atomic (all-or-nothing)

Scenario: Developer observes atomic rollback on conflict
  Given transfer reads accounts A and B at versions 5 and 3
  And concurrent command modifies account B to version 4
  When transfer attempts write
  Then ConcurrencyError is returned for account B
  And neither account is modified (atomic rollback)
  And executor retries with fresh read of both accounts

Scenario: Developer verifies no partial state under concurrency
  Given two transfers execute concurrently on overlapping accounts
  When both commands race on shared streams
  Then one succeeds with atomic write
  And other detects conflict and retries
  And at no point can observer see partial transfer (debit without credit)

Scenario: Developer understands value over sagas
  Given developer reviews TransferMoney example documentation
  When developer compares to saga pattern alternative
  Then documentation explains atomicity eliminates compensating transactions
  And error handling is simpler (business logic only, no orchestration)
```

### Integration Requirements

**API Enhancement Approach:**
- Commands declare multiple StreamId fields
- CommandLogic trait supports state types representing multiple streams
- Context allows emitting events to any declared stream
- Atomic write across all streams with version checking per stream

**Concurrency Testing Requirements:**
- Property tests with 10+ concurrent multi-stream commands
- Verify invariant: sum(all account balances) never changes during transfers
- Stress tests with intentional conflicts (same streams accessed concurrently)
- NO partial state observable at any point

**Performance Expectations:**
- Multi-stream commands slower than single-stream (coordination overhead)
- Target: <100ms P95 for 2-stream commands under low contention
- Correctness maintained even under high contention

### Dependencies

I-001 (single-stream foundation), I-002 (automatic retry with defaults)

### References

- **Requirements:** FR-1.4 (Atomic Multi-Stream Writes)
- **Architecture:** ARCHITECTURE.md Multi-Stream Workflow
- **ADRs:** ADR-001 (Multi-Stream Atomicity), ADR-007 (Optimistic Concurrency)

---

## I-005: PostgreSQL Production Backend

### Purpose

Enable library consumers to use production-ready PostgreSQL storage with ACID transactions for multi-stream atomicity.

**What This Adds:** PostgreSQL implementation of EventStore trait in separate `eventcore-postgres` crate, enabling production deployments.

### What's Included

**PostgreSQL Adapter (Separate Crate):**
- `eventcore-postgres` crate implementing EventStore trait
- Connection pooling (sqlx)
- ACID transaction support for multi-stream atomicity
- Event serialization/deserialization (JSON default)
- Schema migrations (sql scripts)

**Event Schema Design:**
- Events table with UUID primary keys
- Stream ID and version columns with unique constraint
- Event type and data stored as JSONB
- Metadata stored as JSONB
- Timestamp tracking
- Indexes optimized for stream-based queries

**Integration Tests:**
- Real PostgreSQL tests (via Docker Compose)
- Multi-stream atomicity verified with ACID transactions
- Concurrent command tests with actual database contention
- Migration tests (schema evolution scenarios)

### Acceptance Criteria

```gherkin
Feature: Developer deploys EventCore with PostgreSQL backend

Scenario: Developer connects to PostgreSQL
  Given developer has PostgreSQL connection string
  When developer creates PostgresEventStore::new(connection_string)
  Then connection pool is established
  And connection is verified with ping

Scenario: Developer runs schema migrations
  Given developer has fresh PostgreSQL database
  When developer runs eventcore-postgres migrations
  Then events table is created with correct schema
  And indexes are created for query performance

Scenario: Developer stores events in PostgreSQL
  Given developer executes command with PostgresEventStore
  When command writes events to multiple streams
  Then events are stored in PostgreSQL events table
  And stream versions increment atomically
  And event data is serialized as JSON

Scenario: Developer verifies ACID atomicity
  Given developer executes multi-stream command
  When PostgreSQL transaction commits
  Then all events across all streams are visible
  And if transaction rolls back, no events are visible
  And partial writes are impossible

Scenario: Developer handles concurrent commands in production
  Given multiple application instances execute commands
  When commands conflict on stream versions
  Then PostgreSQL detects conflicts via unique constraint
  And ConcurrencyError is returned to executor
  And automatic retry resolves conflict

Scenario: Developer migrates schema safely
  Given production PostgreSQL with existing events
  When developer adds new metadata column
  Then migration is backward compatible
  And existing events remain queryable
  And documentation explains migration strategy
```

### Integration Requirements

**Separate Crate Approach:**
- Separate `eventcore-postgres` crate implementing EventStore trait
- Application adds both eventcore and eventcore-postgres dependencies
- Store creation with PostgreSQL connection string
- Executor accepts PostgresEventStore same as InMemoryEventStore

**Docker Compose for Testing:**
- Docker Compose configuration for local PostgreSQL instance
- Environment variables for database connection
- Port mapping for development access

**Integration Testing Approach:**
- All tests run against real PostgreSQL (not mocks)
- CI runs PostgreSQL in container
- Tests verify ACID properties under concurrency
- Schema migration tests included

**Documentation Requirements:**
- Connection configuration guide
- Schema migration strategy
- Backup/restore procedures
- Performance tuning recommendations (connection pool size, indexes)

### Dependencies

I-001 (EventStore trait), I-004 (multi-stream atomicity to test with production backend)

### What's Excluded

- Schema versioning in events (noted for future consideration)
- Event subscriptions (I-008 adds basic subscriptions)
- Advanced query capabilities (projections handle this)

### References

- **Requirements:** FR-4.1 (Pluggable Storage), IR-1.1 (PostgreSQL Adapter)
- **Architecture:** ARCHITECTURE.md Event Store Abstraction
- **ADRs:** ADR-001 (Multi-Stream Atomicity with ACID), ADR-002 (EventStore Trait)

---

## I-006: Command Derive Macro

### Purpose

Eliminate infrastructure boilerplate by auto-generating CommandStreams trait implementation from `#[stream]` field attributes.

**What This Adds:** `#[derive(Command)]` procedural macro that developers use to generate all the infrastructure code, leaving only domain logic to implement.

### What's Included

**Procedural Macro (Separate Crate):**
- `eventcore-macros` crate with proc-macro
- `#[derive(Command)]` generates CommandStreams trait
- `#[stream]` field attribute marks stream fields
- Phantom types for compile-time stream access control
- Generated code is inspectable (cargo expand)

**Macro Features:**
- Supports single and multiple `#[stream]` fields
- Generates extract_streams() implementation
- Creates phantom types for emit! macro integration
- Helpful compile errors when #[stream] missing

**Documentation:**
- Before/after examples showing boilerplate elimination
- Macro expansion verification in tests
- Common mistakes and error messages guide

### Acceptance Criteria

```gherkin
Feature: Developer eliminates boilerplate with derive macro

Scenario: Developer uses macro for single-stream command
  Given developer defines command struct with #[stream] field
  When developer adds #[derive(Command)]
  Then CommandStreams trait is implemented automatically
  And developer only writes apply() and handle() methods
  And generated code matches hand-written implementation

Scenario: Developer uses macro for multi-stream command
  Given developer defines TransferMoney with two #[stream] fields
  When developer derives Command
  Then both stream IDs are extracted automatically
  And emit! macro type-checks against declared streams
  And developer writes only business logic

Scenario: Developer gets clear compile error for missing attribute
  Given developer forgets #[stream] attribute on stream field
  When developer attempts to compile
  Then compiler produces clear error
  And error message suggests adding #[stream] attribute

Scenario: Developer verifies macro expansion
  Given developer uses cargo expand on derived command
  When macro expands to Rust code
  Then generated code is readable and understandable
  And no unexpected or magic code is generated
  And expansion matches hand-written CommandStreams trait impl

Scenario: Developer migrates from manual to macro
  Given developer has working command with manual trait impl
  When developer adds #[derive(Command)] and removes manual impl
  Then command behaves identically
  And all tests pass without changes
  And code is significantly shorter (fewer lines)
```

### Integration Requirements

**Macro Crate Setup:**
- Separate `eventcore-macros` crate with proc-macro type
- Dependencies on syn, quote, and proc-macro2 for AST manipulation
- Re-exported through main eventcore crate for convenience

**Developer Experience Improvement:**
- Before: Manual CommandStreams trait implementation (~30 lines infrastructure)
- After: #[derive(Command)] with #[stream] attributes (~5 lines)
- Developer focuses only on business logic (apply/handle methods)
- Significant reduction in boilerplate code

**Testing Approach:**
- Macro expansion tests using trybuild for compile-fail scenarios
- Integration tests comparing macro vs manual implementation
- cargo-expand verification in CI to detect unexpected changes
- UI tests for error messages

### Dependencies

I-001 (CommandStreams trait to generate), I-004 (multi-stream to test macro with)

### References

- **Requirements:** FR-5.2 (Command Traits), NFR-2.1 (Minimal Boilerplate)
- **Architecture:** ARCHITECTURE.md Command System
- **ADRs:** ADR-006 (Command Macro Design)

---

## I-007: Dynamic Stream Discovery

### Purpose

Enable commands to discover additional streams at runtime based on state, supporting workflows where stream requirements depend on runtime data.

**What This Adds:** StreamResolver trait allowing commands to examine initial state and declare additional streams to read, with full atomicity maintained.

### What's Included

**StreamResolver Trait:**
- Optional trait for commands with state-dependent streams
- resolve_additional_streams(state) method
- Executor support for multi-pass discovery
- Incremental re-reading of static streams (optimization)

**Discovery Integration:**
- Static streams declared with #[stream] (or manual)
- Dynamic streams discovered via resolver
- All streams (static + dynamic) participate in atomicity
- Deduplication prevents re-reading same stream twice

**Example Implementation:**
- ProcessPayment command discovering payment method streams
- Shows when to use static vs dynamic vs hybrid

### Acceptance Criteria

```gherkin
Feature: Developer discovers streams based on runtime state

Scenario: Developer implements stream resolver
  Given developer creates ProcessPayment command with #[stream] order
  When developer implements StreamResolver trait
  Then resolve_additional_streams() examines order state
  And returns payment method stream IDs based on state

Scenario: Developer executes command with discovery
  Given ProcessPayment command reads order stream
  And order.payment_method is CreditCard("card-123")
  When resolver discovers credit card stream
  Then executor reads credit card stream
  And apply() receives events from both order and credit card streams
  And handle() can emit to both streams atomically

Scenario: Developer benefits from incremental reading
  Given command requires multiple discovery passes
  When second pass discovers new streams
  Then static streams are read incrementally (only new events)
  And newly discovered streams are read from version 0
  And performance is optimized (minimal redundant I/O)

Scenario: Developer chooses appropriate strategy
  Given developer reviews decision framework documentation
  When developer evaluates stream requirements
  Then documentation explains when to use static (known at compile time)
  And when to use dynamic (runtime-dependent)
  And when to use hybrid (mix of static and dynamic)
  And examples show each pattern with trade-offs

Scenario: Developer verifies atomicity with discovery
  Given command discovers streams dynamically
  When executor writes events
  Then ALL streams (static + discovered) participate in version checking
  And atomicity is guaranteed across all streams
  And concurrent modification of any stream triggers retry
```

### Integration Requirements

**API Addition Approach:**
- StreamResolver trait with resolve_additional_streams method
- Method examines state and returns additional stream IDs
- Executor calls resolver after reading static streams
- Incremental re-reading optimizes performance

**Decision Framework Documentation:**
- **Static:** Streams known at compile time (e.g., TransferMoney always needs two accounts)
- **Dynamic:** Streams determined by state (e.g., payment processing depends on payment method)
- **Hybrid:** Mix of static and dynamic (e.g., order stream static, payment stream dynamic)
- When in doubt, start static (simpler) and migrate to dynamic if needed

**Migration Guide:**
- Refactoring path from static to dynamic declarations
- Backward compatibility considerations
- Performance implications of discovery

### Dependencies

I-004 (multi-stream atomicity), I-006 (macro to integrate discovery with)

### References

- **Requirements:** FR-1.3 (Dynamic Stream Discovery)
- **Architecture:** ARCHITECTURE.md Dynamic Discovery Workflow
- **ADRs:** ADR-009 (Stream Resolver Design)

---

## I-008: Basic Event Subscriptions

### Purpose

Enable developers to subscribe to event streams and process events in order. This increment provides the core subscription mechanism WITHOUT checkpointing (I-009 adds that).

**What This Adds:** EventSubscription trait allowing consumers to subscribe to streams and process events as they occur, using either poll-based or callback-based approach.

### What's Included

**EventSubscription Trait:**
- subscribe(stream_ids) method returning event iterator/stream
- Delivers events in stream order
- Works with both PostgreSQL and in-memory backends
- Poll-based or callback-based consumption (developer choice)

**Subscription Features:**
- Subscribe to one or more streams (pattern matching like "account-*")
- Process events to build read models
- Events delivered in order within each stream
- Simple iteration over events

**Example Implementation:**
- AccountBalance projection showing simple read model built from subscription
- Live event processing example (not restart-safe yet)

### What's Excluded (Comes in I-009)

- **Checkpoint storage:** No persistence of subscription position
- **Resume from checkpoint:** Subscription starts from beginning or current on restart
- **Projection rebuilding:** No reliable restart mechanism (I-009 adds this)

### Acceptance Criteria

```gherkin
Feature: Developer subscribes to event streams

Scenario: Developer subscribes to stream pattern
  Given developer creates subscription to "account-*" streams
  When events are appended to matching streams
  Then subscription delivers events in order
  And developer processes events to update read model

Scenario: Developer processes events from multiple streams
  Given developer creates AccountBalance projection
  When developer subscribes to all account streams
  Then events from all matching streams are delivered
  And developer can update balance for each account

Scenario: Developer iterates over events
  Given subscription to account streams
  When developer calls next() on event stream
  Then events are returned in order
  And developer can process synchronously or asynchronously

Scenario: Subscription starts fresh without checkpoint
  Given developer restarts application
  When developer creates subscription again
  Then subscription starts from beginning (or current position)
  And no checkpoint restoration occurs (comes in I-009)
  And developer understands limitation documented
```

### Integration Requirements

**API Addition Approach:**
- EventSubscription trait with subscribe() method
- Subscribe returns async stream of events (Stream<Item = Event>)
- Developer iterates and processes events
- Simple poll-based or callback-based consumption

**Usage Pattern:**
- Create subscription with stream pattern
- Iterate over events as they arrive
- Process each event to update read model
- No checkpoint management yet (manual tracking only)

**Documentation Requirements:**
- Explain subscription lifecycle (start, process, end)
- Document ordering guarantees (per-stream ordering)
- Note checkpoint limitation (I-009 will add)
- Show example projection building from events

### Dependencies

I-005 (PostgreSQL backend to implement subscriptions on)

### References

- **Requirements:** FR-6.1 (Event Subscription)
- **Architecture:** ARCHITECTURE.md Projection Layer

---

## I-009: Checkpointing for Subscriptions

### Purpose

Add checkpoint storage and resume capability to subscriptions, enabling reliable projection rebuilding and restart safety.

**What This Adds:** Checkpoint persistence allowing subscriptions to resume from last processed position after restart, making projections production-ready.

### What's Included

**Checkpoint Storage:**
- save_checkpoint(subscription_id, position) method
- load_checkpoint(subscription_id) returns last saved position
- Checkpoint stored alongside events (same database)
- Automatic checkpoint advancement as events process

**Resume from Checkpoint:**
- On restart, subscription loads last checkpoint
- Events delivered starting from checkpoint position + 1
- Projection rebuilds only new events since last checkpoint
- Idempotent event processing supported

**Projection Rebuilding:**
- Reset checkpoint to version 0 for complete rebuild
- Replay all historical events
- Useful for projection schema changes or bug fixes

**Example Enhancement:**
- AccountBalance projection now checkpoint-aware
- Demonstrates restart safety and incremental processing

### Acceptance Criteria

```gherkin
Feature: Developer uses checkpoints for reliable projections

Scenario: Developer saves checkpoint during processing
  Given subscription has processed events up to version 1000
  When developer calls save_checkpoint()
  Then checkpoint is persisted at version 1000
  And checkpoint storage confirms save

Scenario: Developer resumes from checkpoint after restart
  Given subscription checkpoint exists at version 1000
  When application restarts and creates subscription
  Then subscription loads checkpoint
  And events are delivered starting from version 1001
  And projection continues from last known state

Scenario: Developer rebuilds projection from scratch
  Given subscription has existing checkpoint at version 5000
  When developer resets checkpoint to version 0
  Then subscription replays ALL historical events
  And projection is rebuilt completely
  And new read model is correct

Scenario: Developer manages multiple independent projections
  Given developer creates AccountBalance and AuditLog projections
  When both subscribe to account streams
  Then each projection has independent checkpoint
  And projections can evolve at different rates
  And one projection can rebuild without affecting others

Scenario: Developer handles checkpoint failure gracefully
  Given checkpoint storage is temporarily unavailable
  When save_checkpoint() is called
  Then error is returned (not panic)
  And developer can retry or fall back to manual tracking
```

### Integration Requirements

**API Enhancement Approach:**
- Add checkpoint methods to EventSubscription trait
- save_checkpoint(subscription_id, EventPosition)
- load_checkpoint(subscription_id) -> Option<EventPosition>
- Subscription constructor accepts checkpoint for resume
- Checkpoints stored in same backend (PostgreSQL table, in-memory map)

**Checkpoint Schema (PostgreSQL):**
- subscription_checkpoints table
- Columns: subscription_id, position (version + stream), updated_at
- Unique constraint on subscription_id
- Upsert semantics for save_checkpoint

**Migration from I-008:**
- I-008 subscriptions still work without checkpointing
- Adding checkpoint is opt-in (backward compatible)
- Developer can start with simple subscription, add checkpointing later

**Documentation Requirements:**
- When to checkpoint (frequency trade-offs)
- How to handle checkpoint failures
- Idempotent event processing patterns
- Projection rebuild procedures

### Dependencies

I-008 (basic event subscriptions)

### References

- **Requirements:** FR-6.2 (Projection Rebuilding), NFR-3.1 (Reliability)
- **Architecture:** ARCHITECTURE.md Projection Layer
- **ADRs:** Consider ADR for checkpoint strategy

---

## I-010: Chaos Testing Infrastructure

### Purpose

Enable robust testing by injecting failures (read errors, write errors, version conflicts) into in-memory store.

**What This Adds:** Chaos mode for InMemoryEventStore allowing developers to test error handling paths systematically.

### What's Included

**Chaos Configuration:**
- Configurable failure injection rates
- Read failures, write failures, version conflict injection
- Deterministic chaos for reproducible tests

### Acceptance Criteria

```gherkin
Feature: Developer tests robustness with chaos injection

Scenario: Developer injects read failures
  Given developer enables chaos mode with 50% read failure rate
  When executor attempts to read stream
  Then 50% of reads fail with EventStoreError
  And developer verifies error handling works correctly

Scenario: Developer injects version conflicts
  Given developer enables conflict injection
  When executor attempts write
  Then ConcurrencyError is returned
  And retry logic is exercised
  And test verifies eventual success after retries
```

### Dependencies

I-001 (in-memory store to enhance)

### References

- **Requirements:** NFR-5.1 (Test Infrastructure)

---

## I-011: Performance Benchmarking Suite

### Purpose

Establish performance baselines and track regressions using Criterion.rs benchmarks.

**What This Adds:** Comprehensive benchmark suite measuring throughput, latency, and memory usage for key operations.

### What's Included

**Benchmark Suite:**
- Single-stream command execution
- Multi-stream command execution
- Event append throughput
- State reconstruction performance
- PostgreSQL vs in-memory comparison

### Acceptance Criteria

```gherkin
Feature: Developer tracks performance characteristics

Scenario: Developer runs benchmark suite
  Given benchmark suite with representative commands
  When developer runs cargo bench
  Then benchmarks report ops/sec, latency percentiles (P50, P95, P99)
  And results are stored for regression tracking

Scenario: Developer detects performance regression
  Given baseline benchmarks from previous version
  When code change affects performance
  Then benchmark fails if regression exceeds threshold (e.g., 10%)
  And developer is alerted to investigate
```

### Dependencies

I-001 (single-stream), I-004 (multi-stream), I-005 (PostgreSQL)

### References

- **Requirements:** NFR-1.1, NFR-1.2 (Throughput, Latency)

---

## I-012: Snapshot Support for Performance

### Purpose

Optimize state reconstruction for long-lived streams by periodically saving snapshots and starting reconstruction from snapshot instead of version 0.

**What This Adds:** SnapshotStore trait and snapshot integration with command executor to dramatically reduce reconstruction time for streams with thousands of events.

**Key Design Decision:** This increment comes AFTER I-011 (Performance Benchmarking) because we need performance data to determine if snapshots are necessary and what snapshot frequency makes sense.

### What's Included

**SnapshotStore Trait:**
- save_snapshot(stream_id, version, state) method
- load_snapshot(stream_id) returns (version, state)
- Snapshots stored alongside events
- Automatic snapshot creation at configurable intervals

**Executor Integration:**
- Check for snapshot before reading events
- If snapshot exists, start from snapshot version
- Apply only events after snapshot
- Configurable snapshot frequency based on benchmarking data

**Snapshot Strategy Informed by Benchmarks:**
- Use benchmark data to determine optimal snapshot frequency
- Balance storage cost vs reconstruction time
- Document when snapshots are necessary (stream length threshold)

### Acceptance Criteria

```gherkin
Feature: Developer optimizes long-lived streams with snapshots

Scenario: Developer uses benchmark data to decide on snapshots
  Given developer reviews I-011 benchmark results
  When stream reconstruction time exceeds threshold (e.g., 100ms for 10k events)
  Then developer enables snapshot support
  And chooses snapshot frequency based on performance data

Scenario: Developer creates snapshot of stream
  Given account stream has 10,000 events
  When snapshot is saved at version 10,000
  Then snapshot stores complete account state
  And snapshot size is documented

Scenario: Developer loads state from snapshot
  Given snapshot exists at version 10,000
  When command reads account stream
  Then executor loads snapshot as starting state
  And applies only events 10,001+ (incremental)
  And state reconstruction is dramatically faster

Scenario: Developer configures snapshot frequency
  Given developer sets snapshot interval to 1000 events (from benchmark guidance)
  When events are appended
  Then snapshots are created automatically at 1000, 2000, 3000...
  And reconstruction remains fast even for very old streams

Scenario: Developer measures snapshot impact
  Given developer runs benchmarks with and without snapshots
  When comparing reconstruction time for 50k event stream
  Then snapshot-enabled reconstruction is significantly faster
  And benchmark documents improvement (e.g., 500ms → 50ms)
```

### Integration Requirements

**API Addition Approach:**
- SnapshotStore trait with save_snapshot and load_snapshot methods
- Save method accepts stream ID, version, and serializable state
- Load method returns optional (version, state) tuple
- Generic over state type with Serialize/Deserialize bounds

**Benchmark-Driven Configuration:**
- Documentation includes benchmark data justifying snapshot thresholds
- Recommended snapshot frequencies for different stream sizes
- Storage vs performance trade-off analysis
- When to enable snapshots (not always necessary)

**Documentation Requirements:**
- Snapshot strategy guide based on benchmark results
- Storage implications (snapshot size estimation)
- Snapshot cleanup strategies (old snapshot removal)
- Migration path from non-snapshotted to snapshotted streams

### Dependencies

I-001 (executor to integrate with), I-011 (benchmarks to inform snapshot strategy)

### References

- **Requirements:** NFR-1.2 (Latency optimization)
- **Architecture:** ARCHITECTURE.md Future Considerations

---

## I-013: require! Macro

### Purpose

Provide ergonomic macro for business rule validation with early return, making validation code concise and readable.

**What This Adds:** `require!` macro that checks conditions and returns CommandError::BusinessRuleViolation on failure.

**Key Design Decision:** Split from original I-012 because require! is simpler (just generates early return with error) while emit! is more complex (works with phantom types from derive macro).

### What's Included

**require! Macro:**
- Simple condition checking with early return
- Returns CommandError::BusinessRuleViolation on failure
- Descriptive error messages from validation expressions
- Format string support for dynamic error messages

**Macro Features:**
- Concise syntax: `require!(condition, "error message")`
- Format args: `require!(balance >= amount, "Insufficient funds: have {}, need {}", balance, amount)`
- Expands to readable if/return pattern
- No magic - just code generation

### Acceptance Criteria

```gherkin
Feature: Developer validates business rules with require! macro

Scenario: Developer validates simple condition
  Given developer checks account balance against withdrawal amount
  When developer uses require!(balance >= amount, "Insufficient funds")
  Then condition is checked at runtime
  And failure returns CommandError::BusinessRuleViolation
  And error message is "Insufficient funds"

Scenario: Developer uses format args in error message
  Given developer validates with context
  When developer uses require!(balance >= amount, "Insufficient: have {}, need {}", balance, amount)
  Then error message includes actual values
  And error is actionable for debugging

Scenario: Developer verifies macro expansion
  Given developer uses cargo expand on require! usage
  When macro expands to Rust code
  Then generated code is simple if/return pattern
  And no unexpected magic occurs

Scenario: Developer migrates from manual validation
  Given developer has manual if/return validation
  When developer replaces with require! macro
  Then code is more concise (1 line vs 3-5 lines)
  And behavior is identical
  And error handling unchanged
```

### Integration Requirements

**Macro Implementation:**
- Declarative macro (not proc-macro) - simpler implementation
- Expands to: `if !condition { return Err(CommandError::BusinessRuleViolation(message.to_string())); }`
- Supports format args via format! macro
- Works in any function returning Result<_, CommandError>

**Usage Pattern:**
```rust
fn handle(&self, ctx: &mut Context) -> Result<(), CommandError> {
    require!(self.amount > 0, "Amount must be positive");
    require!(balance >= self.amount, "Insufficient funds: have {}, need {}", balance, self.amount);
    // ... business logic
}
```

### Dependencies

I-001 (CommandError to return)

### References

- **Requirements:** NFR-2.1 (Minimal Boilerplate), NFR-2.3 (Clear Error Messages)
- **ADRs:** ADR-004 (Error Handling Hierarchy)

---

## I-014: emit! Macro

### Purpose

Provide type-safe event emission macro with compile-time verification that events are emitted to declared streams.

**What This Adds:** `emit!` macro that works with phantom types from derive macro to provide compile-time safety for event emission.

**Key Design Decision:** Comes after I-013 (require!) because emit! is more complex - it must work with phantom types generated by #[derive(Command)] macro, while require! is standalone.

### What's Included

**emit! Macro:**
- Compile-time verification that stream is declared in command
- Works with phantom types from #[derive(Command)]
- Concise syntax for event emission
- IDE autocomplete support for stream names

**Type Safety Features:**
- Phantom types ensure events only emitted to declared streams
- Compile error if emitting to undeclared stream
- Type-safe stream references (no string-based stream names at emission)

**Macro Features:**
- Concise syntax: `emit!(ctx, self.account_id, AccountDebited { amount })`
- Integrates with derive macro's generated phantom types
- Descriptive compile errors when stream not declared

### Acceptance Criteria

```gherkin
Feature: Developer emits events with type safety

Scenario: Developer emits event to declared stream
  Given command declares stream "account" with #[stream]
  When developer uses emit!(ctx, self.account_id, AccountDebited { amount })
  Then event is emitted to correct stream
  And code is concise and readable
  And type checker verifies stream is declared

Scenario: Developer gets compile error for undeclared stream
  Given command declares only "account" stream
  When developer attempts emit!(ctx, self.other_id, SomeEvent {})
  Then code fails to compile
  And error message explains "other_id is not a declared stream"
  And error suggests adding #[stream] attribute

Scenario: Developer verifies macro expansion
  Given developer uses cargo expand on emit! usage
  When macro expands to Rust code
  Then generated code calls context.emit() with correct parameters
  And phantom types enforce compile-time checking

Scenario: Developer benefits from IDE autocomplete
  Given developer types emit!(ctx, self.
  When IDE autocomplete activates
  Then only declared stream fields are suggested
  And developer cannot accidentally emit to wrong stream
```

### Integration Requirements

**Macro Implementation:**
- Procedural macro (needs to inspect types) or declarative with clever typing
- Works with phantom types from #[derive(Command)]
- Expands to: `ctx.emit(stream_id, event)`
- Type safety via phantom type markers on context

**Integration with derive macro:**
- Derive macro generates phantom types for each #[stream] field
- emit! macro uses phantom types for compile-time verification
- Stream names from derive macro available to emit! macro

**Usage Pattern:**
```rust
#[derive(Command)]
struct TransferMoney {
    #[stream] from_account: StreamId,
    #[stream] to_account: StreamId,
    amount: u64,
}

impl CommandLogic for TransferMoney {
    fn handle(&self, ctx: &mut Context) -> Result<(), CommandError> {
        emit!(ctx, self.from_account, AccountDebited { amount: self.amount });
        emit!(ctx, self.to_account, AccountCredited { amount: self.amount });
        Ok(())
    }
}
```

### Dependencies

I-006 (derive macro that generates phantom types)

### References

- **Requirements:** FR-2.3 (Event Emission), NFR-2.1 (Minimal Boilerplate), NFR-2.2 (Type Safety)
- **ADRs:** ADR-006 (Command Macro Design)

---

## I-015: Documentation Completeness Audit

### Purpose

Audit and ensure completeness, consistency, and quality of documentation written incrementally throughout I-001 to I-014.

**What This Audit Covers:** This is NOT "write all documentation at the end" - each increment I-001 through I-014 includes its own documentation. This increment ensures that documentation is complete, consistent across increments, and ready for library release.

**Philosophy Change from Original Plan:**
- OLD: "Write comprehensive documentation at end"
- NEW: "Documentation is incremental from I-001, audit completeness at end"

### What's Included

**Completeness Audit:**
- Verify each increment has Getting Started section
- Check API docs completeness (all public items documented)
- Ensure examples directory has working code for each major feature
- Verify troubleshooting guide covers all error types

**Consistency Audit:**
- Terminology consistency across all docs (e.g., "stream" vs "event stream")
- Code style consistency in examples
- Cross-references between docs are accurate
- Version compatibility information is current

**Quality Audit:**
- Onboarding time: Can new developer implement first command in < 30 min?
- Error scenarios: Are common issues documented with solutions?
- Conceptual clarity: Are event sourcing concepts explained for newcomers?
- Migration guides: Are upgrade paths documented?

**Documentation Gaps to Fill:**
- Missing API docs (if any)
- Undocumented features or edge cases
- Missing examples for complex scenarios
- Incomplete troubleshooting entries

### Acceptance Criteria

```gherkin
Feature: Documentation is complete and consistent

Scenario: Audit reveals documentation completeness
  Given auditor reviews all increments I-001 to I-014
  When checking for documentation coverage
  Then each increment has Getting Started section
  And all public APIs have doc comments with examples
  And examples/ directory has working code for each feature
  And troubleshooting guide covers all error types

Scenario: Audit ensures terminology consistency
  Given auditor reviews all documentation
  When checking terminology usage
  Then "stream" is used consistently (not mixing with "event stream")
  And code examples follow consistent style
  And cross-references are accurate and up-to-date

Scenario: New developer validates onboarding quality
  Given developer has no EventCore experience
  When developer follows Getting Started guide
  Then developer implements first command in under 30 minutes
  And finds answers to common questions in docs
  And successfully deploys to production using deployment guide

Scenario: Audit identifies and fills gaps
  Given auditor reviews documentation against requirements
  When gaps are identified (missing examples, unclear explanations)
  Then gaps are documented and prioritized
  And critical gaps are filled before release
  And minor gaps are tracked for future improvement
```

### Integration Requirements

**Audit Process:**
1. Review each increment's documentation for completeness
2. Check cross-references and links
3. Verify examples compile and run
4. Test Getting Started guide with fresh developer
5. Review API docs with `cargo doc --open`

**Documentation Checklist:**
- [ ] Getting Started guide (complete and tested)
- [ ] API reference (all public items documented)
- [ ] Conceptual introduction (event sourcing fundamentals)
- [ ] Troubleshooting guide (all error types covered)
- [ ] Anti-patterns guide (common mistakes + fixes)
- [ ] Deployment guide (PostgreSQL setup, connection pooling, monitoring)
- [ ] Examples directory (bank account, e-commerce, multi-tenant)
- [ ] Upgrade/migration guides (version compatibility)

**Quality Metrics:**
- Time to first command: < 30 minutes (tested with real developer)
- API doc coverage: 100% of public items
- Example coverage: All major features have working examples
- Error coverage: All error types have troubleshooting entries

### Dependencies

I-001 through I-014 (all features documented incrementally)

### References

- **Requirements:** NFR-2.4 (Documentation and Examples)
- **Process:** ~/.claude/processes/DOCUMENTATION_PHILOSOPHY.md

---

## I-016: Error Message Consistency Audit

### Purpose

Audit and ensure consistency, clarity, and actionability of error messages written incrementally throughout I-001 to I-014.

**What This Audit Covers:** This is NOT "add error messages at the end" - error quality is foundational from I-001. This increment ensures error messages are consistent in format, provide appropriate context, and are actionable across all increments.

**Philosophy Change from Original Plan:**
- OLD: "Polish error messages at end"
- NEW: "Error quality is foundational from I-001, audit consistency at end"

### What's Included

**Consistency Audit:**
- Error message format consistency (e.g., always include context)
- Error type usage consistency (BusinessRuleViolation vs Permanent errors)
- Context inclusion patterns (stream IDs, versions, command types)
- Action suggestion patterns ("Automatic retry will..." vs "Increase...")

**Clarity Audit:**
- Are error messages understandable to library consumers?
- Do messages explain WHAT failed and WHY?
- Are technical terms explained or linked to docs?
- Are error codes/types meaningful?

**Actionability Audit:**
- Does each error suggest next steps?
- Are links to documentation included where helpful?
- Do validation errors show actual vs expected values?
- Are retry/recovery paths documented?

**Error Quality Standards:**
- Context: Always include relevant IDs (stream, command, correlation)
- Explanation: What failed and why
- Action: What developer should do next
- Links: Reference to documentation (where applicable)

### Acceptance Criteria

```gherkin
Feature: Error messages are consistent and actionable

Scenario: Audit reveals error message consistency
  Given auditor reviews all error types across I-001 to I-014
  When checking error message format
  Then all errors include relevant context (stream IDs, versions)
  And all errors explain what failed and why
  And error format is consistent across all increments

Scenario: Developer receives actionable error messages
  Given developer encounters various error scenarios
  When error is returned
  Then error message explains what failed
  And error suggests next steps (retry, increase capacity, fix code)
  And error includes context for debugging (actual vs expected values)

Scenario: Version conflict error provides full context
  Given concurrent modification causes conflict
  When developer receives ConcurrencyError
  Then error includes stream IDs and current/expected versions
  And error explains "Automatic retry will reattempt with fresh state"
  And error links to concurrency documentation

Scenario: Business rule violation includes context
  Given account has balance 50
  When developer executes Withdraw with amount 100
  And business rule "sufficient funds" fails in handle()
  Then CommandError::BusinessRuleViolation is returned
  And error shows "Insufficient funds: balance 50, required 100"
  And error is actionable for debugging

Scenario: Audit identifies and fixes inconsistencies
  Given auditor reviews all error messages
  When inconsistencies are found (missing context, unclear wording)
  Then inconsistencies are documented and prioritized
  And critical issues are fixed before release
  And minor issues are tracked for future improvement
```

### Integration Requirements

**Audit Process:**
1. Review all error types (EventStoreError, CommandError, ValidationError, ConcurrencyError)
2. Check error message format consistency
3. Verify context inclusion (IDs, versions, values)
4. Test actionability (do messages help developers fix issues?)
5. Review error documentation completeness

**Error Message Checklist:**
- [ ] Context: All errors include relevant IDs and values
- [ ] Explanation: All errors explain what failed and why
- [ ] Action: All errors suggest next steps
- [ ] Links: Errors link to docs where helpful
- [ ] Format: Consistent message format across all types
- [ ] Testing: Error scenarios tested and validated

**Quality Metrics:**
- Context coverage: 100% of errors include relevant context
- Actionability: 100% of errors suggest next steps
- Documentation links: Key error types link to troubleshooting guide
- Format consistency: All errors follow standard format

### Dependencies

I-001 (error hierarchy foundational from start), I-015 (documentation to link to)

### References

- **Requirements:** NFR-2.3 (Clear Error Messages)
- **ADRs:** ADR-004 (Error Handling Hierarchy)
- **Process:** ~/.claude/processes/DOCUMENTATION_PHILOSOPHY.md

---

## Implementation Roadmap

### Phase 1: Core Functionality (Weeks 1-4)
- **I-001: Single-Stream Command End-to-End** (Week 1-2) - 🎯 COMPLETE WORKING SYSTEM (no retry)
- **I-002: Automatic Retry with Defaults** (Week 2) - Add automatic retry without configuration
- **I-003: Configurable Retry Policies** (Week 3) - Add retry configuration and observability
- **I-004: Multi-Stream Atomicity** (Week 4) - 🎯 CORE VALUE PROP DELIVERED

**Milestone:** Core value proposition (multi-stream atomicity) fully implemented with in-memory backend.

### Phase 2: Production Readiness (Weeks 5-7)
- **I-005: PostgreSQL Production Backend** (Week 5-6) - 🎯 PRODUCTION READY
- **I-006: Command Derive Macro** (Week 7) - Developer ergonomics
- **I-007: Dynamic Stream Discovery** (Week 7) - Advanced workflows

**Milestone:** Library ready for production use with excellent ergonomics.

### Phase 3: Advanced Features (Weeks 8-11)
- **I-008: Basic Event Subscriptions** (Week 8) - Subscribe and process events (no checkpointing)
- **I-009: Checkpointing for Subscriptions** (Week 9) - Add checkpoint/resume capability
- **I-010: Chaos Testing Infrastructure** (Week 9) - Failure injection for robust testing
- **I-011: Performance Benchmarking** (Week 10) - Establish baselines and track regressions
- **I-012: Snapshot Support** (Week 11) - Optimize based on benchmark data

**Milestone:** Advanced capabilities for complex scenarios with data-driven optimization.

### Phase 4: Developer Experience Polish (Weeks 12-14)
- **I-013: require! Macro** (Week 12) - Ergonomic business rule validation
- **I-014: emit! Macro** (Week 12) - Type-safe event emission with phantom types
- **I-015: Documentation Completeness Audit** (Week 13) - Ensure doc quality across increments
- **I-016: Error Message Consistency Audit** (Week 14) - Ensure error quality across increments

**Milestone:** Library ready for public release with excellent developer experience and consistent quality.

---

## Success Criteria

### Developer Experience
- ✅ New developer implements first command in under 30 minutes
- ✅ Typical command requires fewer than 50 lines of code (with macro)
- ✅ Type errors provide clear guidance on fixes
- ✅ Documentation examples are copy-paste ready

### Correctness
- ✅ Multi-stream atomicity verified via concurrent integration tests
- ✅ Version conflicts detected 100% of the time
- ✅ No data corruption possible under any failure scenario
- ✅ Retry logic eventually succeeds or fails clearly

### Performance
- ✅ Single-stream throughput adequate for business operations (50+ ops/sec)
- ✅ Multi-stream operations maintain correctness at scale
- ✅ Memory usage remains bounded under load
- ✅ No memory leaks under sustained operation

### Adoption
- ✅ API intuitive to Rust developers familiar with async
- ✅ Examples cover common use cases (banking, e-commerce)
- ✅ Community contributions feasible via clear extension points
- ✅ Error messages enable self-service problem resolution

---

## References

### Core Documentation
- **REQUIREMENTS_ANALYSIS.md:** Functional and non-functional requirements (FR-1 through FR-6, NFR-1 through NFR-5)
- **ARCHITECTURE.md:** System design and component interactions
- **CLAUDE.md:** Project philosophy and development patterns

### Architectural Decision Records
- **ADR-001:** Multi-Stream Atomicity Implementation Strategy
- **ADR-002:** Event Store Trait Design
- **ADR-003:** Type System Patterns for Domain Safety
- **ADR-004:** Error Handling Hierarchy
- **ADR-005:** Event Metadata Structure
- **ADR-006:** Command Macro Design
- **ADR-007:** Optimistic Concurrency Control Strategy
- **ADR-008:** Command Executor and Retry Logic
- **ADR-009:** Stream Resolver Design for Dynamic Discovery

### Process Documentation
- **~/.claude/processes/STORY_PLANNING.md:** Planning methodology (adapted for library development)
- **~/.claude/processes/DOCUMENTATION_PHILOSOPHY.md:** WHAT/WHY not HOW principles
- **~/.claude/processes/INTEGRATION_VALIDATION.md:** Testing and verification requirements

---

## Key Principles for Implementation

### Vertical Slice Discipline

1. **Each increment must be independently valuable** - Provides complete, usable functionality
2. **Integration tests are mandatory** - Test from library consumer perspective
3. **Type-driven development throughout** - Invalid states unrepresentable at compile time
4. **No horizontal layering** - Don't build "all types" then "all storage" then "all commands"
5. **Include infrastructure from day 1** - Types, errors, storage in I-001 (not separate increments)

### Testing Strategy

- **Integration tests:** Complete command execution from consumer perspective
- **Property tests:** Invariant verification across random inputs
- **Concurrent tests:** Multi-stream atomicity under concurrent load (NO partial state)
- **Chaos tests:** Failure injection to verify error handling
- **Performance tests:** Benchmarks establishing baselines
- **Real backends:** PostgreSQL tests via Docker (not mocks)

### Common Pitfalls to Avoid

- **Premature abstraction:** Start concrete (I-001), abstract later when patterns emerge
- **Horizontal layering:** Each increment must be end-to-end testable
- **Deferring types/errors:** Include from increment 1 (not "add later")
- **Skipping integration tests:** Must test from consumer perspective
- **Missing manual verification:** Document how developer would actually use this

---

**Document Status:** Version 3.0 - Progressive Disclosure Restructure Complete
**Key Improvements:**
- Subscription complexity split for learning curve (I-008 basic → I-009 checkpointing)
- Snapshot optimization data-driven (after I-011 benchmarks)
- Macro complexity progressive (I-013 simple require! → I-014 complex emit!)
- Documentation/error quality built-in from start, audited at end

**Next Steps:** Begin I-001 implementation with complete end-to-end single-stream command execution
