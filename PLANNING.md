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

- [ ] **Dependency Optimization**
  - [ ] Add granular Tokio feature flags to reduce dependency weight
  - [ ] Evaluate and remove unnecessary dependency features
  - [ ] Update workspace Cargo.toml with minimal feature requirements

- [ ] **Enhanced Type Safety**
  - [ ] Explore const generics for compile-time stream set validation
  - [ ] Investigate type-level lists for stream access control
  - [ ] Evaluate replacing runtime validation with compile-time checks where possible

### 15.3 Error Handling and Diagnostics Improvements

**Addresses feedback from Without Boats, Edwin Brady, and Michael Snoyman**

- [ ] **Unified Error Types**
  - [ ] Create unified `ValidationError` type to reduce error mapping boilerplate
  - [ ] Improve error conversion traits and reduce manual mapping
  - [ ] Separate business rule violations from technical errors clearly

- [ ] **Better Lock Handling**
  - [ ] Replace `expect("RwLock poisoned")` with graceful error handling
  - [ ] Implement recovery strategies for lock poisoning scenarios
  - [ ] Add comprehensive tests for panic recovery

- [ ] **Timeout Controls**
  - [ ] Add timeout configuration to all EventStore operations
  - [ ] Implement timeout handling in command execution
  - [ ] Add circuit breaker patterns for resilience

### 15.4 Complete Missing Features

**Addresses feedback from Yoshua Wuyts, Gabriele Keller, and Kent Beck**

- [ ] **Subscription System Implementation**
  - [ ] Complete the stubbed subscription system with real implementations
  - [ ] Add position tracking and checkpointing
  - [ ] Implement subscription replay and catch-up logic
  - [ ] Write comprehensive tests for subscription reliability

- [ ] **Schema Evolution Strategy**
  - [ ] Design and implement event versioning strategy
  - [ ] Add support for event schema migration
  - [ ] Create tools for handling backward compatibility
  - [ ] Document schema evolution best practices

- [ ] **Enhanced Observability**
  - [ ] Expand metrics collection with more detailed metrics
  - [ ] Improve tracing with better span hierarchy
  - [ ] Add performance monitoring and alerting guidelines
  - [ ] Implement structured logging best practices

### 15.5 Testing and Quality Improvements

**Addresses feedback from Kent Beck, Without Boats, and Gabriele Keller**

- [ ] **Property Test Enhancements**
  - [ ] Improve shrinking strategies for better minimal counterexamples
  - [ ] Add configurable iteration counts for CI vs comprehensive testing
  - [ ] Optimize property test performance for faster CI runs
  - [ ] Add more sophisticated concurrency testing scenarios

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

- [ ] **Connection Management**
  - [ ] Add comprehensive connection pooling configuration
  - [ ] Implement connection health checks and recovery
  - [ ] Add connection timeout and retry strategies
  - [ ] Create connection pool monitoring

- [ ] **Resilience Patterns**
  - [ ] Implement circuit breaker patterns
  - [ ] Add backpressure handling mechanisms
  - [ ] Create graceful degradation strategies
  - [ ] Add bulkhead isolation patterns

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
