# Technical Requirements Analysis: EventCore

**Document Version:** 1.0
**Date:** 2025-10-12
**Project:** EventCore
**Phase:** 1 - Technical Requirements Analysis
**Workflow:** Infrastructure

## Executive Summary

EventCore is a type-safe event sourcing library for Rust that implements multi-stream event sourcing with dynamic consistency boundaries. Unlike traditional event sourcing libraries that enforce rigid aggregate boundaries, EventCore enables commands to atomically read from and write to multiple event streams, allowing developers to define consistency boundaries that match their business requirements rather than technical constraints.

**Target Consumers:** Rust application developers building event-sourced systems who need strong consistency guarantees across related business entities without the complexity of sagas or process managers.

**Core Value Proposition:** EventCore eliminates the artificial constraints of traditional event sourcing by enabling atomic multi-stream operations while maintaining type safety, strong consistency guarantees, and developer ergonomics through code generation.

## Current Landscape

### Existing Solutions

**Traditional Event Sourcing Frameworks:**
- Axon Framework (Java), EventStore, NEventStore, Marten
- **Limitation:** Force rigid aggregate boundaries, requiring sagas/process managers for cross-aggregate operations
- **Consequence:** Increased complexity, eventual consistency where immediate consistency is needed

**Database Transaction Systems:**
- Traditional RDBMS with ACID transactions
- **Limitation:** Lose event sourcing benefits (audit trail, time travel, event replay)
- **Consequence:** Cannot reconstruct historical state, difficult to build event-driven architectures

**CQRS/ES Libraries in Rust:**
- cqrs-es, eventually
- **Limitation:** Follow traditional aggregate model without multi-stream atomicity
- **Consequence:** Same saga complexity as non-Rust solutions

### EventCore's Innovation

EventCore addresses the fundamental limitation of traditional event sourcing: **the inability to atomically modify multiple streams**. This enables:
- Money transfers between accounts (atomic debit/credit)
- Order fulfillment across inventory, orders, and shipping
- Distributed ledger operations with guaranteed balance
- Complex business workflows without saga orchestration

## Functional Requirements

### FR-1: Multi-Stream Command Execution

**FR-1.1 Stream Declaration**
- Library SHALL provide `#[derive(Command)]` macro for command definitions
- Macro SHALL generate boilerplate from `#[stream]` field attributes
- Commands SHALL declare stream dependencies at compile time via field annotations
- Generated code SHALL include phantom type for compile-time stream tracking
- WHY: Developers need declarative stream dependencies without manual boilerplate

**FR-1.2 Type-Safe Stream Access**
- Library SHALL prevent writing to undeclared streams at compile time
- API SHALL use phantom types to enforce stream access control
- `StreamWrite` operations SHALL only accept streams from declared set
- WHY: Type system should prevent runtime errors from invalid stream access

**FR-1.3 Dynamic Stream Discovery**
- Commands SHALL discover additional streams during execution
- API SHALL provide `StreamResolver` for runtime stream addition
- Executor SHALL re-execute commands when new streams are discovered
- Discovery SHALL maintain atomicity guarantees across all streams
- WHY: Business logic may determine stream requirements based on runtime state

**FR-1.4 Atomic Multi-Stream Writes**
- Library SHALL write events to multiple streams in single atomic operation
- All events SHALL be written or none, preventing partial state updates
- Atomicity SHALL be maintained across any number of streams
- WHY: Business operations spanning multiple entities require consistency

### FR-2: State Reconstruction and Event Handling

**FR-2.1 Event Folding**
- Commands SHALL implement `apply()` method for state reconstruction
- Library SHALL fold events in order to build current state
- State reconstruction SHALL be deterministic and repeatable
- WHY: Event sourcing requires rebuilding state from event history

**FR-2.2 Command Business Logic**
- Commands SHALL implement `handle()` method for validation and event generation
- Business logic SHALL receive reconstructed state from all declared streams
- Validation SHALL use `require!` macro for concise business rule checks
- WHY: Separation of state reconstruction from business logic enables clarity

**FR-2.3 Event Emission**
- API SHALL provide `emit!` macro for type-safe event creation
- Events SHALL be associated with specific streams from declared set
- Multiple events SHALL be generated per stream if needed
- WHY: Developers need ergonomic API for event generation with compile-time safety

### FR-3: Optimistic Concurrency Control

**FR-3.1 Version Tracking**
- Event store SHALL track version numbers for each stream
- Version SHALL increment atomically with each write
- Read operations SHALL capture stream versions
- WHY: Detect concurrent modifications without locks

**FR-3.2 Conflict Detection**
- Write operations SHALL verify expected versions match current versions
- Mismatch SHALL result in version conflict error
- Conflicts SHALL be detected before any events are written
- WHY: Prevent lost updates and maintain consistency

**FR-3.3 Automatic Retry**
- Executor SHALL automatically retry commands on version conflicts
- Retry SHALL re-read streams and rebuild state
- Retry logic SHALL use configurable exponential backoff
- Excessive retries SHALL eventually fail with descriptive error
- WHY: Handle concurrent modifications gracefully without manual retry logic

### FR-4: Event Store Abstraction

**FR-4.1 Pluggable Storage**
- Library SHALL define `EventStore` trait for storage adapters
- Trait SHALL support reading single and multiple streams
- Trait SHALL support atomic writes across multiple streams
- Trait SHALL support event subscriptions for projections
- WHY: Developers need choice of storage backend (PostgreSQL, in-memory, custom)

**FR-4.2 Event Ordering**
- Events SHALL have globally unique identifiers using UUIDv7
- UUIDv7 SHALL provide time-based ordering
- Stream ordering SHALL be maintained via version numbers
- WHY: Global event ordering enables projection building and debugging

**FR-4.3 Event Metadata**
- Events SHALL include metadata for auditing (user, correlation, causation)
- Metadata SHALL support custom fields via key-value map
- Metadata SHALL be preserved in storage
- WHY: Audit trails and distributed tracing require event context

### FR-5: Type-Driven Domain Modeling

**FR-5.1 Validated Domain Types**
- Library SHALL integrate with `nutype` for domain type validation
- Types SHALL validate at construction (parse, don't validate pattern)
- Invalid values SHALL be rejected at API boundaries
- WHY: Type safety prevents invalid states from propagating through system

**FR-5.2 Command Traits**
- Commands SHALL implement `CommandStreams` trait (macro-generated)
- Commands SHALL implement `CommandLogic` trait (developer-provided)
- Trait design SHALL separate infrastructure from domain concerns
- WHY: Clear separation enables testability and maintainability

**FR-5.3 Error Handling**
- Library SHALL define structured error types for different failure modes
- Errors SHALL distinguish between retriable and permanent failures
- Business rule violations SHALL be explicit and actionable
- WHY: Developers need clear error handling strategies

### FR-6: Projection and Read Model Support

**FR-6.1 Event Subscription**
- Event store SHALL provide subscription mechanism for real-time events
- Subscriptions SHALL support checkpointing for resume
- Subscriptions SHALL deliver events in order
- WHY: Projections need real-time updates for read models

**FR-6.2 Projection Rebuilding**
- Projections SHALL rebuild from arbitrary event positions
- Rebuild SHALL be independent of current projection state
- WHY: Support projection corrections and new projection creation

**FR-6.3 Multiple Projections**
- Multiple projections SHALL consume same event streams
- Projections SHALL evolve independently
- WHY: Different read models for different query patterns

## Non-Functional Requirements

### NFR-1: Performance Characteristics

**NFR-1.1 Throughput**
- Single-stream commands SHALL execute efficiently (current: ~86 ops/sec PostgreSQL)
- Multi-stream commands SHALL maintain atomicity guarantees (current: ~25-50 ops/sec)
- Batch event writes SHALL achieve high throughput (current: 9,000+ events/sec)
- WHY: Production systems need adequate throughput for real-world workloads

**NFR-1.2 Latency**
- Command execution SHALL maintain low latency (current: P95 ~14ms)
- Retry overhead SHALL be minimal via exponential backoff
- State reconstruction SHALL be efficient through optimized event loading
- WHY: User-facing operations require responsive systems

**NFR-1.3 Performance Optimization Strategy**
- Performance SHALL prioritize correctness over raw throughput
- Multi-stream atomicity SHALL not compromise for speed
- Library SHALL provide optimization hooks (caching, batching)
- WHY: Correctness is non-negotiable; speed improvements come through optimization

### NFR-2: API Ergonomics and Developer Experience

**NFR-2.1 Minimal Boilerplate**
- `#[derive(Command)]` macro SHALL eliminate infrastructure code
- Developers SHALL write only domain-specific logic
- Common patterns SHALL be expressible in few lines
- WHY: Reduce time to productivity and maintenance burden

**NFR-2.2 Compile-Time Safety**
- Invalid stream access SHALL be caught at compile time
- Type system SHALL make illegal states unrepresentable
- Rust compiler SHALL provide clear error messages for common mistakes
- WHY: Catch errors before runtime

**NFR-2.3 Clear Error Messages**
- Runtime errors SHALL include actionable context
- Validation failures SHALL explain what rule was violated
- Version conflicts SHALL identify which streams conflicted
- WHY: Debugging efficiency depends on error clarity

**NFR-2.4 Documentation and Examples**
- Public API SHALL have comprehensive doc comments with examples
- Common patterns SHALL be documented in manual
- WHY: Developer adoption depends on documentation quality

### NFR-3: Compatibility and Integration

**NFR-3.1 Rust Edition**
- Library SHALL support Rust edition 2024
- Library SHALL use stable Rust features only (no nightly)
- WHY: Stability and broad compatibility

**NFR-3.2 Async Runtime**
- Library SHALL be async runtime agnostic (Tokio, async-std)
- Async trait usage SHALL be via `async-trait` crate
- WHY: Developers need flexibility in runtime choice

**NFR-3.3 Web Framework Integration**
- Commands SHALL integrate cleanly with web frameworks (Axum, Actix, etc.)
- Library SHALL not force specific web framework
- WHY: EventCore is a library component, not a framework

**NFR-3.4 Serialization**
- Events SHALL serialize via `serde` with JSON default
- Custom serialization formats SHALL be supported
- WHY: Flexibility in storage format and compatibility

### NFR-4: Type Safety and Correctness

**NFR-4.1 No Primitive Obsession**
- Domain concepts SHALL use validated newtypes, not primitives
- `StreamId` SHALL wrap String with validation
- Money, Email, etc. SHALL have dedicated types
- WHY: Type safety prevents entire classes of errors

**NFR-4.2 Total Functions**
- Public API SHALL handle all cases explicitly
- No unwrap/expect in library code
- Error paths SHALL be explicit via Result types
- WHY: Library code must be reliable

**NFR-4.3 Memory Safety**
- Library SHALL leverage Rust ownership for memory safety
- No unsafe code unless absolutely necessary and documented
- WHY: Memory safety is fundamental to Rust's value proposition

### NFR-5: Testing and Verification

**NFR-5.1 Test Infrastructure**
- Library SHALL provide in-memory event store for testing
- In-memory store SHALL support chaos testing (failure injection)
- Testing utilities SHALL support property-based testing
- WHY: Developers need to test their commands easily

**NFR-5.2 Property-Based Testing**
- Library internals SHALL use property-based tests (proptest)
- Invariants SHALL be verified across random inputs
- WHY: Catch edge cases not covered by example-based tests

**NFR-5.3 Integration Testing**
- Library SHALL provide integration test examples
- Tests SHALL cover multi-stream scenarios
- Tests SHALL verify concurrent execution
- WHY: Multi-stream atomicity requires integration verification

## Integration Requirements

### IR-1: Storage Adapter Integration

**IR-1.1 PostgreSQL Adapter**
- Adapter SHALL implement `EventStore` trait
- Adapter SHALL use ACID transactions for atomicity
- Adapter SHALL provide connection pooling configuration
- Adapter SHALL be distributed as separate `eventcore-postgres` crate
- WHY: PostgreSQL is primary production storage backend

**IR-1.2 In-Memory Adapter**
- Adapter SHALL provide fast in-memory storage for testing
- Adapter SHALL support optional chaos injection
- Adapter SHALL be distributed as separate `eventcore-memory` crate
- WHY: Fast tests without external dependencies

**IR-1.3 Adapter Extension**
- Third parties SHALL implement custom adapters via trait
- Adapter trait SHALL have clear documentation and examples
- WHY: Enable community-contributed storage backends

### IR-2: Macro Integration

**IR-2.1 Procedural Macro Crate**
- `#[derive(Command)]` SHALL be in separate `eventcore-macros` crate
- Macro SHALL generate well-documented code for debugging
- Macro errors SHALL provide helpful messages
- WHY: Separate compilation and standard Rust macro distribution

**IR-2.2 Macro Dependencies**
- Macro SHALL work with minimal dependencies
- Generated code SHALL reference core library types
- WHY: Minimize macro compilation overhead

### IR-3: Runtime Dependencies

**IR-3.1 Core Dependencies**
- Library SHALL depend on: async-trait, serde, uuid, chrono, thiserror
- Dependencies SHALL be stable and well-maintained
- Optional features SHALL gate non-essential dependencies
- WHY: Minimize dependency burden while providing essential functionality

**IR-3.2 No Conflicting Dependencies**
- Library SHALL not force specific versions of common dependencies
- Version requirements SHALL be as permissive as compatibility allows
- WHY: Reduce dependency conflicts in consumer applications

## Success Criteria

### Developer Experience Metrics
- New developers SHALL create first command in under 30 minutes
- Command implementation SHALL require fewer than 50 lines of code typically
- Type errors SHALL provide clear guidance on fixes
- Documentation examples SHALL be copy-paste ready

### Correctness Metrics
- Multi-stream atomicity SHALL be verified via concurrent tests
- Version conflicts SHALL be detected 100% of the time
- No data corruption possible under any failure scenario
- Retry logic SHALL eventually succeed or fail clearly

### Performance Metrics
- Single-stream throughput SHALL be adequate for CRUD applications (50+ ops/sec)
- Multi-stream operations SHALL maintain correctness even at scale
- Memory usage SHALL remain bounded under load
- No memory leaks under sustained operation

### Adoption Metrics
- Library API SHALL be intuitive to Rust developers familiar with async
- Examples SHALL cover common use cases (banking, e-commerce, workflow)
- Community contributions SHALL be feasible via clear extension points
- Error messages SHALL enable self-service problem resolution

## Dependencies and Constraints

### Technical Dependencies
- **Rust Language:** Edition 2024, stable compiler
- **Async Runtime:** Tokio or async-std (consumer choice)
- **Database:** PostgreSQL 15+ for production (via adapter)
- **Serialization:** serde ecosystem for event serialization

### Design Constraints
- **Type Safety First:** Cannot compromise type safety for convenience
- **Atomicity Guarantee:** Multi-stream atomicity is non-negotiable feature
- **No Magic:** Generated code must be inspectable and understandable
- **Parse Don't Validate:** Domain types must enforce invariants at construction

### Ecosystem Constraints
- Library must coexist with web frameworks without tight coupling
- Must integrate with standard Rust observability (tracing, metrics)
- Must support standard Rust testing tools (cargo test, nextest, proptest)
- Must follow Rust API guidelines for consistency

## Risk Assessment

### Technical Risks

**R-1: Performance Gap**
- **Risk:** Current performance (86 ops/sec) may be insufficient for high-throughput applications
- **Mitigation:** Document performance characteristics clearly; provide optimization guide; performance is adequate for many real-world use cases; future optimizations possible without API changes
- **Impact:** Medium - limits use cases but doesn't prevent adoption for typical CRUD applications

**R-2: Multi-Stream Complexity**
- **Risk:** Atomic multi-stream operations are complex to implement correctly
- **Mitigation:** Comprehensive testing; clear documentation; examples demonstrate correct usage
- **Impact:** High if incorrect - would undermine core value proposition

**R-3: Database Lock Contention**
- **Risk:** Multi-stream writes may cause database lock contention under high load
- **Mitigation:** Document lock ordering; provide stream partitioning guidance; optimize transaction duration
- **Impact:** Medium - affects high-concurrency scenarios

**R-4: Macro Complexity**
- **Risk:** Procedural macros are complex to maintain and debug
- **Mitigation:** Generate readable code; comprehensive macro tests; clear error messages
- **Impact:** Low - macro is isolated and well-tested

### Adoption Risks

**R-5: Learning Curve**
- **Risk:** Event sourcing concepts may be unfamiliar to developers
- **Mitigation:** Comprehensive documentation; working examples; gradual learning path
- **Impact:** Medium - affects initial adoption rate

**R-6: Breaking Changes**
- **Risk:** Library evolution may require breaking API changes
- **Mitigation:** Semantic versioning; migration guides; deprecation warnings
- **Impact:** Low - standard Rust practices mitigate this

**R-7: Storage Backend Limitations**
- **Risk:** Specific storage backends may not support required features
- **Mitigation:** Clear adapter trait documentation; reference implementations; community contributions
- **Impact:** Low - PostgreSQL reference implementation covers primary use case

## Next Steps

**Immediate (Phase 1 Complete):**
1. ✅ Requirements analysis documented
2. Validate requirements with stakeholder
3. Proceed to Phase 3: Architectural Decision Records (Phase 2 Event Modeling skipped for infrastructure projects)

**Phase 3 Preparation:**
- Document key architectural decisions
- Define adapter trait design
- Specify macro generation strategy
- Establish type system patterns
- Design error handling hierarchy

**Phase 4-7 Roadmap:**
- Phase 4: Architecture synthesis
- Phase 6: Technical increment planning (replacing story planning)
- Phase 7: Increment-by-increment implementation
- Phase 8: Acceptance validation

This requirements document establishes WHAT EventCore must provide and WHY, leaving all HOW decisions to subsequent phases.
