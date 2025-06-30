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

## Phase 1-14: Core Implementation ✅ COMPLETED

All initial implementation phases have been completed successfully, including:
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

## Phase 15: Post-Review Improvements

Based on the comprehensive expert review, the following improvements have been identified:

### 15.1 Simplify API Surface and Reduce Complexity

**Addresses feedback from Rich Hickey, Ward Cunningham, and Without Boats regarding complexity management**

- [x] **Consolidate Command Creation Approaches**
  - [x] Evaluate usage patterns of manual vs procedural macro vs declarative macro approaches
  - [x] Choose one primary approach and deprecate others for 1.0 release
  - [x] Update documentation to focus on the chosen approach
  - [x] Provide clear migration paths for existing code

- [x] **Remove Over-Engineering**
  - [x] Remove unused `version_cache` field from PostgreSQL adapter or implement it
  - [x] Evaluate and remove other premature optimizations identified in review
  - [x] Simplify configuration options where possible

- [x] **Magic Number Configuration**
  - [x] Replace hardcoded `MAX_ITERATIONS: usize = 10` with configurable parameter
  - [x] Add validation and sensible defaults for iteration limits
  - [x] Document when high iteration counts might be legitimate

### 15.2 Modernize Rust Dependencies and Idioms

**Addresses feedback from Niko Matsakis, Yoshua Wuyts, and Without Boats**

- [x] **Async Trait Migration**
  - [x] Investigate migration from `async-trait` to native async traits (Rust 1.75+)
  - [x] Benchmark performance improvements from removing async-trait overhead
  - [x] Update MSRV if necessary for native async traits
  - **Decision**: Migration not recommended due to trait object compatibility issues and minimal performance benefit. See ASYNC_TRAIT_MIGRATION.md for detailed analysis.

- [x] **Dependency Optimization**
  - [x] Add granular Tokio feature flags to reduce dependency weight
  - [x] Evaluate and remove unnecessary dependency features
  - [x] Update workspace Cargo.toml with minimal feature requirements

- [x] **Enhanced Type Safety Analysis** (CORRECTED - Compile-time validation impossible)
  - [x] Explore const generics for compile-time stream set validation (INFEASIBLE - Stream IDs are runtime data)
  - [x] Investigate type-level lists for stream access control (INFEASIBLE - Requires compile-time knowledge)
  - [x] **FINDING**: 100% of stream IDs are runtime data (account IDs, product IDs, etc.) making compile-time validation impossible
  - [x] **ALTERNATIVE**: Focus on runtime optimization opportunities identified in analysis
  - [x] **CLEANUP**: Remove flawed analysis documents and organize remaining docs in docs/ directory

### 15.2A Runtime Performance Optimizations

**Based on corrected analysis of what's actually feasible given dynamic stream IDs**

- [x] **Stream Access Validation Optimization** (HIGH IMPACT)
  - [x] Replace O(n) vector search with O(1) hash set lookup in `StreamWrite::new`
  - [x] Add pre-computed hash set to `ReadStreams` for fast validation
  - [x] Benchmark performance improvement (expected: eliminate O(n) cost per stream write)

- [x] **Stream ID Constructor Optimization**
  - [x] Add `StreamId::from_static()` for compile-time known literals (e.g., "transfers")
  - [x] Implement const fn validation for string literals where possible
  - [x] Optimize hot path construction for dynamic IDs with runtime caching

- [x] **Input Type Safety Improvements** (COMPLETED)
  - [x] Implement branded types for command inputs (SourceAccount vs TargetAccount)
  - [x] Add compile-time guarantees for business rules where types permit
  - [x] Create smart constructors that eliminate redundant validation

### 15.3 Error Handling and Diagnostics Improvements

**Addresses feedback from Without Boats, Edwin Brady, and Michael Snoyman**

- [x] **Unified Error Types**
  - [x] Create unified `ValidationError` type to reduce error mapping boilerplate
  - [x] Improve error conversion traits and reduce manual mapping
  - [x] Separate business rule violations from technical errors clearly

- [x] **Better Lock Handling**
  - [x] Replace `expect("RwLock poisoned")` with graceful error handling
  - [x] Implement recovery strategies for lock poisoning scenarios
  - [x] Add comprehensive tests for panic recovery

- [x] **Timeout Controls**
  - [x] Add timeout configuration to all EventStore operations
  - [x] Implement timeout handling in command execution
  - [x] Add circuit breaker patterns for resilience

### 15.4 Complete Missing Features

**Addresses feedback from Yoshua Wuyts, Gabriele Keller, and Kent Beck**

- [x] **Subscription System Implementation** (COMPLETED)
  - [x] Complete the stubbed subscription system with real implementations
  - [x] Add position tracking and checkpointing  
  - [x] Implement subscription replay and catch-up logic
  - [x] Write comprehensive tests for subscription reliability
  - [x] Add edge case testing for failure scenarios and concurrent operations
  - [x] Create integration tests demonstrating subscription usage with projections

- [x] **Schema Evolution Strategy**
  - [x] Design and implement event versioning strategy
  - [x] Add support for event schema migration
  - [x] Create tools for handling backward compatibility
  - [x] Document schema evolution best practices

- [x] **Enhanced Observability**
  - [x] Expand metrics collection with more detailed metrics
  - [x] Improve tracing with better span hierarchy
  - [x] Add performance monitoring and alerting guidelines
  - [x] Implement structured logging best practices

### 15.5 Testing and Quality Improvements

**Addresses feedback from Kent Beck, Without Boats, and Gabriele Keller**

- [x] **Property Test Enhancements** ✅ COMPLETED
  - [x] Improve shrinking strategies for better minimal counterexamples
  - [x] Add configurable iteration counts for CI vs comprehensive testing
  - [x] Optimize property test performance for faster CI runs
  - [x] Add more sophisticated concurrency testing scenarios

- [ ] **Integration Test Expansion**
  - [ ] Add comprehensive concurrent scenario tests
  - [ ] Implement failure mode testing with controlled chaos
  - [ ] Add rollback behavior verification tests
  - [ ] Create performance regression test suite

### 15.6 Performance Validation and Optimization

**Addresses feedback from Yaron Minsky and Michael Snoyman**

- [ ] **Load Testing and Benchmarks**
  - [ ] Create realistic workload benchmarks
  - [ ] Profile stream discovery loops for bottlenecks
  - [ ] Benchmark against traditional event store solutions
  - [ ] Validate performance targets with real-world scenarios

- [ ] **Caching Strategy Implementation**
  - [ ] Implement or remove the version cache infrastructure
  - [ ] Add caching for frequently accessed streams
  - [ ] Design cache invalidation strategies
  - [ ] Measure cache effectiveness

### 15.7 Production Hardening

**Addresses feedback from Ward Cunningham and Michael Snoyman**

- [x] **Connection Management** ✅ COMPLETED
  - [x] Add comprehensive connection pooling configuration
  - [x] Implement connection health checks and recovery
  - [x] Add connection timeout and retry strategies
  - [x] Create connection pool monitoring

- [x] **Resilience Patterns** ✅ COMPLETED
  - [x] Implement circuit breaker patterns
  - [x] Add backpressure handling mechanisms
  - [x] Create graceful degradation strategies
  - [x] Add bulkhead isolation patterns

### 15.8 Documentation and Developer Experience

**Addresses feedback from Ward Cunningham and Philip Wadler**

- [ ] **Comprehensive Documentation**
  - [ ] Create "Why EventCore?" decision guide
  - [ ] Document when to use vs simpler solutions
  - [ ] Add performance characteristics documentation
  - [ ] Create troubleshooting and debugging guides

- [ ] **Developer Experience Polish**
  - [ ] Complete or remove the declarative `command!` macro
  - [ ] Improve error messages with more context
  - [ ] Add more detailed usage examples
  - [ ] Create migration guides from traditional event sourcing

### 15.9 CQRS Integration and Advanced Features

**Addresses feedback from Gabriele Keller and Bartosz Milewski**

- [ ] **CQRS Support**
  - [ ] Expand projection system for full CQRS support
  - [ ] Add read model synchronization strategies
  - [ ] Implement projection rebuild capabilities
  - [ ] Create projection monitoring and health checks

- [ ] **Advanced Example Applications**
  - [ ] Complete long-running saga example
  - [ ] Add performance testing example application
  - [ ] Create distributed system example
  - [ ] Add real-time collaboration example

### 15.10 Ecosystem Integration

**Addresses feedback about broader Rust ecosystem compatibility**

- [ ] **Framework Integration**
  - [ ] Create integration examples with popular Rust web frameworks
  - [ ] Add serialization format flexibility beyond JSON
  - [ ] Integrate with popular monitoring and observability tools
  - [ ] Create deployment and operations guides

## Priority and Sequencing

### High Priority (Address First)
1. **Simplify API Surface** (15.1) - Addresses core complexity concerns
2. **Complete Missing Features** (15.4) - Subscription system and schema evolution
3. **Production Hardening** (15.7) - Essential for real-world usage
4. **Error Handling Improvements** (15.3) - Critical for developer experience

### Medium Priority
5. **Modernize Dependencies** (15.2) - Performance and ecosystem alignment
6. **Testing Improvements** (15.5) - Quality and reliability enhancements
7. **Documentation** (15.8) - User adoption and onboarding

### Lower Priority
8. **Performance Optimization** (15.6) - Important but system already functional
9. **CQRS Integration** (15.9) - Advanced features for specific use cases
10. **Ecosystem Integration** (15.10) - Nice-to-have integrations

## Success Criteria for Phase 15

Each improvement area is complete when:

1. **All identified issues from expert review are addressed**
2. **Comprehensive tests validate the improvements**
3. **Documentation explains the changes and migration paths**
4. **Performance benchmarks show no regressions**
5. **Expert feedback concerns are demonstrably resolved**

## Notes from Expert Review

### Key Insights
- **Type-driven development approach is exemplary** - maintain this strength
- **Testing infrastructure is world-class** - continue this investment
- **Multi-stream innovation is valuable** - the core concept is sound
- **Complexity vs simplicity is the main tension** - focus on simplification
- **Production readiness needs attention** - prioritize hardening features

### Expert Grades Summary
- Philip Wadler: A- ("Impressive achievement in type-driven development")
- Kent Beck: B+ ("Testing gives confidence, examples prove usability")
- Rich Hickey: B ("Over-engineered but well-executed")
- Edwin Brady: A ("Beautiful from type system perspective")
- Michael Snoyman: A- ("Production-ready with minor reservations")
- Without Boats: B+ ("Solid Rust code with room for modernization")
- Bartosz Milewski: A- ("Theoretically sound and practically useful")

### Areas of Consensus
- **Strengths**: Type safety, testing, clean architecture, innovation
- **Concerns**: Complexity, learning curve, missing production features
- **Recommendation**: Focus on simplification while maintaining type safety benefits

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
