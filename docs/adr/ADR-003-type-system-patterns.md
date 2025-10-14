# ADR-003: Type System Patterns for Domain Safety

## Status

accepted

## Context

EventCore is a type-safe event sourcing library for Rust where correctness is paramount. The library serves as infrastructure for business-critical systems where invalid states, runtime panics, or type confusion can lead to data corruption or incorrect business outcomes. The type system must enforce domain constraints, prevent illegal states, and guide developers toward correct usage.

**Key Forces:**

1. **Correctness Over Convenience**: Library code must be reliable and handle all error cases explicitly
2. **Developer Ergonomics**: Types should be self-documenting and prevent misuse without excessive ceremony
3. **Compile-Time Safety**: Catch errors during development, not in production
4. **Domain Clarity**: Business concepts should have explicit types, not primitives
5. **API Boundaries**: Invalid data must be rejected at construction time, not discovered during processing
6. **Total Functions**: Library code must handle all cases explicitly without panicking
7. **Rust Ecosystem Integration**: Patterns should align with community standards (serde, async-trait, etc.)

**Why This Decision Now:**

Type system patterns are foundational architectural decisions that affect every API design, error handling strategy, and domain modeling choice. These patterns must be established before implementing core traits (EventStore, Command, etc.) to ensure consistency across the library.

## Decision

EventCore will implement a rigorous type-driven development approach based on the following patterns:

**1. Parse, Don't Validate - Domain Types Over Primitives**

All domain concepts will be represented by validated newtypes using the `nutype` crate, not primitive types. Types will validate invariants at construction time and guarantee validity thereafter.

- StreamId instead of String
- EventId (UUIDv7) instead of Uuid
- Validated domain concepts (Email, AccountNumber, etc. in consumer code)
- Construction returns Result types with descriptive errors

**2. Total Functions - Explicit Error Handling**

All public APIs will handle all cases explicitly via Result types:

- No unwrap() or expect() in library code
- Error types distinguish retriable vs permanent failures
- Business rule violations are explicit domain errors
- Panics reserved only for programming errors (assertions in debug builds)

**3. Making Illegal States Unrepresentable**

Type system will prevent invalid states at compile time:

- Phantom types for compile-time stream access control
- State machines via typestate pattern where applicable
- Enum variants to model mutually exclusive states
- Builder patterns for complex construction with validation

**4. Structured Error Types**

Error handling will use thiserror for structured, actionable errors:

- Error types categorize failures (validation, version conflict, storage, etc.)
- Errors include context for debugging and recovery decisions
- Distinction between retriable and permanent failures
- Error types implement standard traits (Error, Display, Debug)

**5. Smart Constructors with nutype**

Domain types will use nutype for validation at boundaries:

- Sanitization (trim whitespace, normalize)
- Validation rules (length, format, business constraints)
- Derived traits for ergonomics (Debug, Clone, Serialize, etc.)
- AsRef/Deref for accessing inner values safely

**6. Composition Over Complexity**

Types will compose via small, focused traits:

- Single-responsibility traits (CommandStreams, CommandLogic)
- Generic bounds for flexibility
- Associated types for type-level relationships
- Trait objects for dynamic dispatch only when needed

## Rationale

**Why Parse, Don't Validate with nutype:**

Traditional validation approaches check values repeatedly throughout the codebase. The "parse, don't validate" pattern validates once at construction, then the type system guarantees validity. Using nutype:

- Eliminates redundant validation checks
- Makes validity guarantees explicit in function signatures
- Catches invalid data at API boundaries before it propagates
- Provides compile-time documentation of constraints
- Reduces runtime overhead (single validation at construction)

**Why Total Functions:**

Library code is used by many applications and must be reliable. Panics in library code are unacceptable because:

- Consumers cannot recover from panics gracefully
- Panics in async contexts can corrupt executor state
- Production systems need explicit error handling, not crashes
- Result types make error paths visible in signatures

**Why Illegal States Unrepresentable:**

Preventing invalid states at compile time is far superior to runtime validation:

- Eliminates entire classes of bugs before code runs
- Type errors provide immediate feedback during development
- Runtime checks are expensive and can be missed
- Compiler-verified correctness enables confident refactoring

**Why Structured Error Types:**

Flat error types or string-based errors lose information and make error handling difficult:

- Structured errors enable programmatic handling (retry logic, fallback strategies)
- Context aids debugging and observability
- Type safety prevents errors being ignored or mishandled
- Aligns with Rust ecosystem standards (thiserror, anyhow)

**Why nutype for Domain Types:**

Hand-written newtypes are verbose and error-prone. nutype provides:

- Declarative validation rules
- Automatic derive of common traits
- Consistent error types
- Reduces boilerplate while maintaining type safety

**Trade-offs Accepted:**

- **Verbosity**: More explicit types means more code, but it's self-documenting and safer
- **Learning Curve**: Developers must understand Result types, nutype patterns, and type-driven design
- **Compilation Time**: More types and derives increase compile time, but catch more errors
- **API Evolution**: Changing type constraints is a breaking change, requiring careful API design

These trade-offs are acceptable because:

- Correctness and reliability are non-negotiable for event sourcing infrastructure
- Type safety prevents entire categories of production bugs
- Verbosity is offset by clarity and self-documentation
- One-time learning investment benefits all users of the library

## Consequences

**Positive:**

- **Compile-Time Safety**: Large classes of bugs are impossible (invalid IDs, null/empty values where not allowed)
- **Self-Documenting Code**: Function signatures express constraints and requirements
- **Fearless Refactoring**: Type system catches breakage during refactoring
- **Explicit Error Handling**: Consumers must handle errors consciously, not accidentally ignore them
- **Domain Clarity**: Business concepts have explicit types, improving code readability
- **Ecosystem Alignment**: Patterns match Rust community best practices

**Negative:**

- **Initial Verbosity**: More types to define and manage
- **Learning Curve**: Developers unfamiliar with type-driven development need education
- **Compilation Overhead**: More types and trait bounds increase compile time
- **Breaking Changes**: Type constraint changes require major version bumps
- **Migration Complexity**: Changing validation rules affects all consumers

**Enabled Future Decisions:**

- Command trait design can use phantom types for stream access control
- Error types can be evolved to support observability and debugging
- Domain modeling examples can demonstrate type-driven patterns
- Testing can leverage type safety to reduce test burden
- Macro generation can produce type-safe boilerplate

**Constrained Future Decisions:**

- All public APIs must use Result types, not panics
- All domain concepts must be wrapped in validated types
- Type system must prevent invalid states at compile time
- Error types must be structured and actionable
- Breaking type changes require semantic versioning

## Alternatives Considered

### Alternative 1: Primitive Types with Runtime Validation

Use String, Uuid, u64, etc. directly with runtime validation checks scattered throughout code.

**Rejected Because:**

- Validation is repeated in multiple places, creating maintenance burden
- No compile-time guarantees that validation occurred
- Runtime checks add overhead to every operation
- Function signatures don't express constraints
- Easy to forget validation at API boundaries
- Invalid states can propagate through system before detection

### Alternative 2: NewType Pattern Without Validation

Define newtypes (struct StreamId(String)) without validation, relying on consumer discipline.

**Rejected Because:**

- No enforcement of invariants at construction time
- Moves responsibility to consumers, not library
- Violates "parse, don't validate" principle
- Documentation alone is insufficient for correctness
- Still allows invalid states to be constructed

### Alternative 3: Aggressive Use of unwrap/expect with Documentation

Use Option/Result internally but unwrap with expect messages documenting assumptions.

**Rejected Because:**

- Panics in library code are unacceptable
- Consumers cannot recover from panics gracefully
- "Documented panics" are still production failures
- Violates Rust philosophy of explicit error handling
- Makes library unsuitable for fault-tolerant systems

### Alternative 4: Separate Validation Layer

Provide unvalidated types plus separate validation functions consumers must call.

**Rejected Because:**

- Requires consumer discipline to remember validation
- No compile-time enforcement that validation occurred
- Creates two parallel type hierarchies (validated/unvalidated)
- Function signatures don't express validation requirements
- Duplicates effort compared to validated newtypes

### Alternative 5: Macro-Based Validation Instead of nutype

Create custom derive macros for validation instead of using nutype.

**Rejected Because:**

- Reinvents the wheel - nutype is battle-tested
- Macro maintenance burden falls on EventCore maintainers
- nutype provides extensive validation rules out of the box
- Custom macros unlikely to match nutype's ergonomics
- Not a good use of development time for infrastructure library

## References

- REQUIREMENTS_ANALYSIS.md: FR-5 Type-Driven Domain Modeling
- REQUIREMENTS_ANALYSIS.md: FR-5.1 Validated Domain Types
- REQUIREMENTS_ANALYSIS.md: FR-5.3 Error Handling
- REQUIREMENTS_ANALYSIS.md: NFR-4.1 No Primitive Obsession
- REQUIREMENTS_ANALYSIS.md: NFR-4.2 Total Functions
- CLAUDE.md: Type-Driven Development philosophy
- Parse, Don't Validate: https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/
- nutype crate: https://github.com/greyblake/nutype
