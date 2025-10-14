# ADR-004: Error Handling Hierarchy

## Status

accepted

## Context

EventCore requires a comprehensive error handling strategy that serves multiple audiences and use cases. As a library for business-critical event sourcing systems, errors must be structured to enable appropriate programmatic responses while providing clear context for debugging and observability.

**Key Forces:**

1. **Error Categories**: Different failure modes require different handling - validation failures, version conflicts, storage errors, and business rule violations each have distinct characteristics
2. **Retriable vs Permanent**: Automatic retry logic (from ADR-001) requires distinguishing transient failures (network timeouts, version conflicts) from permanent failures (validation errors, business rule violations)
3. **Observability**: Errors must carry sufficient context for debugging, logging, and distributed tracing without leaking sensitive information
4. **Consumer Ergonomics**: Library users need clear, actionable error messages that guide resolution
5. **Type Safety**: Rust's type system should enable programmatic error handling via pattern matching
6. **Async Integration**: Errors must flow cleanly through async/await boundaries and be compatible with async error handling patterns
7. **Cross-Cutting Concerns**: Correlation IDs, causation chains, and metadata must be preserved through error paths for distributed tracing

**Why This Decision Now:**

Error handling is foundational to the library API. The error hierarchy shapes how commands report failures, how the executor handles retry logic, and how consumers respond to failures. This decision must be made before implementing core traits (EventStore, CommandLogic) to ensure consistent error handling patterns throughout.

## Decision

EventCore will implement a hierarchical error structure with clear categorization and rich context:

**1. Top-Level Error Types**

Define separate error enums for different library subsystems:

- `EventStoreError`: Storage backend failures
- `CommandError`: Command execution failures
- `ValidationError`: Domain type validation failures
- `ConcurrencyError`: Version conflict and optimistic locking failures

**2. Error Classification Traits**

Provide traits for programmatic error classification:

- `Retriable`: Marker trait indicating transient failures suitable for automatic retry
- `Permanent`: Marker trait indicating failures that will not succeed on retry
- Error types implement appropriate classification traits

**3. Error Context Enrichment**

All error types include structured context:

- **Correlation ID**: Links related operations across command executions
- **Causation Chain**: Tracks causal relationships between commands and events
- **Operation Context**: Describes what operation failed (stream IDs, command type, etc.)
- **Diagnostic Information**: Details for debugging without sensitive data

**4. Business Rule Violations**

Business rule violations are a distinct error category:

- Part of CommandError hierarchy
- Always permanent failures (not retriable)
- Include rule name and explanation for actionable feedback
- Integrated with `require!` macro for ergonomic validation

**5. Storage Error Mapping**

EventStore implementations map backend errors to structured types:

- Network/timeout errors marked as Retriable
- Constraint violations marked as Permanent
- Backend-specific context preserved in error chain
- Version conflicts handled via dedicated ConcurrencyError type

**6. Error Propagation**

Errors flow through the system with context accumulation:

- Lower-level errors wrapped with higher-level context
- Source errors preserved via Rust's Error trait chaining
- Context added at each layer without losing original cause
- Async error propagation via standard Result and `?` operator

## Rationale

**Why Separate Error Enums per Subsystem:**

Each subsystem (storage, command execution, validation) has distinct error characteristics and handling requirements. Separate enums:

- Enable targeted pattern matching in error handlers
- Prevent unrelated error variants cluttering single enum
- Allow each subsystem to evolve independently
- Make error sources immediately clear from type

**Why Retriable/Permanent Classification:**

ADR-001's automatic retry logic requires distinguishing transient from permanent failures. Classification traits:

- Enable executor to make retry decisions programmatically
- Prevent wasted retry attempts on permanent failures (validation, business rules)
- Guide consumers on appropriate error handling strategies
- Make retry semantics explicit in the API

**Why Rich Error Context:**

Production debugging and observability require context, but error messages can't contain everything. Structured context fields:

- Enable observability tools to extract relevant information
- Support distributed tracing via correlation/causation IDs
- Provide debugging information without logging sensitive data
- Allow consumers to customize error presentation for their needs

**Why Dedicated Business Rule Error Category:**

Business rule violations are fundamentally different from technical failures:

- Always result from domain logic, not infrastructure
- Never retriable (command will fail again with same input)
- Require different presentation to end users vs technical errors
- Need to carry rule-specific context for clear messaging

**Why Storage Error Mapping:**

Different storage backends have different failure modes and error types. Mapping to structured EventCore errors:

- Abstracts backend-specific details from consumers
- Enables consistent error handling regardless of backend
- Preserves important backend information in error chain
- Allows retry logic to work across different storage implementations

**Why Error Context Accumulation:**

Errors originate deep in the call stack but need context from higher layers. Context accumulation:

- Provides full picture without losing low-level details
- Follows Rust error handling patterns (Error trait, source())
- Enables root cause analysis via error chains
- Supports different logging strategies at different layers

**Trade-offs Accepted:**

- **Multiple Error Types**: More error types add complexity but enable precise handling
- **Context Overhead**: Storing context in errors adds allocation cost, but essential for production debugging
- **Classification Ambiguity**: Some errors may be situationally retriable, requiring documentation and guidance
- **Backend Abstraction Leakage**: Storage errors may expose backend details despite abstraction

These trade-offs are acceptable because:

- Type safety and precise error handling justify multiple error types
- Context overhead is negligible compared to database operations
- Clear documentation and examples address classification ambiguity
- Backend details in error chains aid debugging without breaking abstraction

## Consequences

**Positive:**

- **Automatic Retry**: Executor can reliably distinguish retriable vs permanent failures for ADR-001's retry logic
- **Type-Safe Handling**: Pattern matching enables exhaustive, compiler-checked error handling
- **Observability**: Structured context enables rich logging, metrics, and distributed tracing
- **Clear Errors**: Actionable error messages guide consumers toward resolution
- **Debugging Support**: Full error chains preserve root causes for production debugging
- **Backend Flexibility**: Storage backends map errors consistently regardless of underlying technology
- **Business Logic Clarity**: Business rule violations are explicit and separate from technical failures

**Negative:**

- **API Complexity**: Multiple error types increase learning curve
- **Context Management**: Developers must remember to add context at appropriate layers
- **Pattern Matching**: Consumers must handle all error variants or use wildcard matches
- **Documentation Burden**: Each error variant needs clear documentation and examples
- **Breaking Changes**: Adding error variants is technically a breaking change

**Enabled Future Decisions:**

- Retry policy configuration can use Retriable/Permanent classification
- Observability integrations can extract correlation IDs and context
- Monitoring dashboards can track error rates by category and retriability
- User-facing error messages can be customized based on error type
- Chaos testing can inject specific error categories to verify handling
- Circuit breakers can use error classification for failure detection

**Constrained Future Decisions:**

- All public APIs must return Result types with appropriate error types
- New error categories must implement classification traits (Retriable/Permanent)
- Error context must include correlation/causation for tracing
- Storage backends must map errors to EventCore types consistently
- Business rule violations must always be CommandError::BusinessRuleViolation
- Error types must implement std::error::Error trait for ecosystem compatibility

## Alternatives Considered

### Alternative 1: Single Unified Error Enum

Define one error enum for the entire library with variants for all failure modes.

**Rejected Because:**

- Creates enormous enum with dozens of variants from different subsystems
- Makes pattern matching cumbersome (unrelated variants must be handled)
- Prevents subsystems from evolving error types independently
- Loses type-level distinction between error sources
- Error variant names require prefixes to avoid collisions (StorageTimeout, CommandTimeout, etc.)
- Doesn't scale as library grows

### Alternative 2: String-Based Error Messages

Use simple error types (anyhow, Box<dyn Error>) with string messages.

**Rejected Because:**

- No type-safe programmatic error handling
- Cannot reliably distinguish retriable vs permanent failures
- Difficult to extract structured context for observability
- Pattern matching impossible - consumers resort to string parsing
- Error classification requires fragile string matching
- Loses compile-time guarantees about error handling
- Not suitable for library API (better for applications)

### Alternative 3: No Retriable/Permanent Classification

Provide error types without classification, leaving retry decisions to consumers.

**Rejected Because:**

- Defeats purpose of automatic retry logic in ADR-001
- Forces every consumer to reimplement retry classification
- Inconsistent retry behavior across applications
- Documentation burden shifts to consumers
- Library has best knowledge of which errors are retriable
- Misses opportunity to encode critical distinction in type system

### Alternative 4: Detailed Error Variants for Every Failure Mode

Define specific error variant for every possible failure (PostgresConnectionTimeout, PostgresConstraintViolation, ValidationLengthError, etc.).

**Rejected Because:**

- Creates explosion of error variants
- Too fine-grained for most consumer error handling needs
- Makes pattern matching impractical (50+ variants)
- Backend-specific variants break abstraction
- Difficult to add new variants without breaking changes
- Consumers typically care about category (retriable, validation, storage), not specific failure

### Alternative 5: Error Codes with Payload

Use integer error codes with optional untyped payload (similar to errno).

**Rejected Because:**

- Not idiomatic Rust - language has first-class enum error types
- Loses type safety and compile-time checks
- Payload type ambiguity requires runtime checks
- Error code documentation easily becomes stale
- Pattern matching on integers is fragile
- Doesn't leverage Rust's Error trait ecosystem
- Reminiscent of C error handling patterns

### Alternative 6: No Error Context (Minimal Errors)

Provide simple error variants without correlation IDs or context.

**Rejected Because:**

- Insufficient for production debugging and observability
- Distributed tracing requires correlation/causation chains
- Root cause analysis difficult without operation context
- Logging and monitoring tools need structured context
- Error messages become vague without contextual information
- Contradicts NFR-2.3 requirement for clear, actionable errors

## References

- ADR-001: Multi-Stream Atomicity Implementation Strategy (automatic retry logic)
- ADR-002: Event Store Trait Design (storage error handling)
- ADR-003: Type System Patterns for Domain Safety (Result types, thiserror)
- REQUIREMENTS_ANALYSIS.md: FR-3.2 Conflict Detection
- REQUIREMENTS_ANALYSIS.md: FR-3.3 Automatic Retry
- REQUIREMENTS_ANALYSIS.md: FR-5.3 Error Handling
- REQUIREMENTS_ANALYSIS.md: NFR-2.3 Clear Error Messages
- REQUIREMENTS_ANALYSIS.md: NFR-4.2 Total Functions
