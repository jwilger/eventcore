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

## Next Phase: To Be Determined

All documented implementation phases have been completed. The project is ready for:
- Production usage
- Community feedback
- Feature requests
- Performance optimization based on real-world usage patterns

### Recent Maintenance (2025-07-05)
- Reviewed and updated all documentation for consistency
- Fixed outdated Command trait references (now CommandLogic)
- Updated broken documentation links in README.md
- Corrected license information to reflect MIT-only licensing
- Ensured all examples use current API patterns

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