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

## Next Phase: Post-Review Improvements

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

## Development Process Rules

When working on this project, **ALWAYS** follow these rules:

1. **BROKEN CI BUILDS ARE HIGHEST PRIORITY** - If CI is failing, stop all other work and fix it immediately.
2. **Review @PLANNING.md** to discover the next task to work on.
3. **IMMEDIATELY use the todo list tool** to create a todolist with the specific actions you will take to complete the task.
4. **ALWAYS include "Update @PLANNING.md to mark completed tasks" in your todolist** - This task should come BEFORE the commit task to ensure completed work is tracked.
5. **Insert a task to "Run all tests and make a commit if they all pass"** after each discrete action that involves a change to the code, tests, database schema, or infrastructure.
6. **The FINAL item in the todolist MUST always be** to "Push your changes to the remote repository, monitor CI workflow with gh cli, and if it passes, review @PLANNING.md to discover the next task and review our process rules."

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