# ADR-006: Command Macro Design

## Status

accepted

## Context

EventCore's primary developer-facing API is the `#[derive(Command)]` macro. This macro eliminates infrastructure boilerplate while maintaining compile-time type safety, allowing developers to focus on business logic. The macro must bridge two competing forces: ergonomics (minimal code) and safety (compile-time guarantees).

**Key Forces:**

1. **Developer Ergonomics**: Commands require stream declarations, state types, phantom types, and trait implementations - significant boilerplate for every command
2. **Compile-Time Safety**: Stream access must be validated at compile time to prevent runtime errors from undeclared stream writes
3. **Separation of Concerns**: Infrastructure code (stream management) should be separate from domain logic (business rules)
4. **Multi-Stream Support**: From ADR-001, commands read/write multiple streams atomically - macro must support this naturally
5. **Type System Integration**: Macro must align with ADR-003's type-driven patterns (validated types, no primitive obsession)
6. **Debuggability**: Generated code must be readable and understandable when debugging macro errors
7. **Library Maintenance**: Macro lives in separate crate (eventcore-macros) but must stay synchronized with core traits
8. **Dynamic Stream Discovery**: From FR-1.3, commands may discover additional streams at runtime via StreamResolver

**Why This Decision Now:**

The macro is the primary API developers interact with when using EventCore. This design decision shapes developer experience, learning curve, and the boundary between infrastructure and business logic. It must be defined before implementing command traits (CommandStreams, CommandLogic) to ensure consistency between macro-generated code and hand-written implementations.

## Decision

EventCore will provide a `#[derive(Command)]` procedural macro that generates infrastructure boilerplate from declarative field annotations, with the following design:

**1. Stream Declaration via Field Attributes**

Developers declare stream dependencies using `#[stream]` attributes on struct fields:

```rust
#[derive(Command)]
struct TransferMoney {
    #[stream]
    from_account: StreamId,
    #[stream]
    to_account: StreamId,
    amount: Money,
}
```

Each `#[stream]` field identifies one stream the command will read from and potentially write to.

**2. Generated StreamSet Phantom Type**

Macro generates a unique phantom type representing the set of declared streams:

- Type embeds stream structure at compile time
- Enables type-level tracking of which streams are accessible
- Used by `StreamWrite<StreamSet, Event>` to enforce compile-time access control
- Prevents writing to undeclared streams (caught by compiler, not at runtime)

**3. Trait Separation: Generated vs Developer-Provided**

Macro generates `CommandStreams` trait implementation (infrastructure):

- Extracts stream IDs from command fields
- Manages phantom type for compile-time tracking
- Handles interaction with EventStore trait
- Exposes `fn stream_declarations(&self) -> StreamDeclarations` for executor use
- Pure boilerplate - no business logic

Developers implement `CommandLogic` trait manually (business logic):

- `apply(state, event)` - reconstructs state from events
- `handle(state, context)` - validates business rules and produces events
- Contains only domain-specific logic
- No infrastructure concerns

**4. Type-Safe Event Emission**

Generated code integrates with `emit!` macro for type-safe event production:

```rust
// emit! macro ensures Event type matches and stream is declared
emit!(context, from_account, AccountDebited { amount });
emit!(context, to_account, AccountCredited { amount });
```

Compile-time error if attempting to emit to undeclared stream.

**5. Dynamic Stream Discovery Integration**

For commands that discover streams at runtime:

```rust
#[derive(Command)]
struct ProcessPayment {
    #[stream]
    order: StreamId,
    // Additional streams discovered via StreamResolver
}
```

Macro-generated code supports `StreamResolver` trait for adding streams dynamically while maintaining atomicity from ADR-001.

**6. Generated Code Characteristics**

Macro produces readable code:

- Clear function names and structure
- Includes generated code comments
- Follows Rust naming conventions
- Minimal use of advanced macros in generated output
- Debuggable via `cargo expand` or IDE macro expansion

**7. Distribution as Separate Crate**

`#[derive(Command)]` lives in `eventcore-macros` crate:

- Follows Rust ecosystem patterns (syn, quote, proc-macro2)
- Independent compilation from core library
- Generated code references core eventcore types
- Versioned in lockstep with core library

## Rationale

**Why Stream Declaration via Field Attributes:**

Stream dependencies are naturally represented as command fields - they're part of the command's identity. Using `#[stream]` attributes:

- Makes stream dependencies explicit and visible in command definition
- Avoids separate stream declaration mechanism (trait, separate struct, etc.)
- Enables macro to extract stream structure from existing fields
- Aligns with developer mental model (fields represent command data, annotations add semantics)
- Reduces boilerplate (streams declared where they're used)

Alternative approaches (separate trait methods, builder patterns) require duplicating stream information in multiple locations.

**Why Phantom Types for Compile-Time Safety:**

Runtime stream access validation adds overhead and can only catch errors in tests or production. Phantom types leverage Rust's type system:

- Zero runtime cost (phantom types erased at compile time)
- Compiler enforces stream access rules automatically
- Impossible to write events to undeclared streams
- Type errors caught during development, not in production
- Aligns with ADR-003's "make illegal states unrepresentable" principle

This enables EventCore's promise: if it compiles, stream access is valid.

**Why Separate CommandStreams and CommandLogic Traits:**

Mixing infrastructure and business logic in a single trait creates problems:

- Business logic developers must understand infrastructure concerns
- Testing becomes complex (mocking both concerns)
- Generated code would mix with hand-written code
- Trait evolution affects both concerns simultaneously

Separation provides:

- Clear boundary between macro-generated (infrastructure) and developer-written (business logic)
- Developers only implement domain-specific methods
- Each trait evolves independently
- Testing focuses on appropriate concerns (state reconstruction, business rules)

**Why Macro Generation vs Hand-Written Boilerplate:**

Hand-writing CommandStreams implementations requires:

- 20-50 lines of boilerplate per command
- Understanding of phantom types and type-level programming
- Potential for errors (typos in stream extraction, incorrect phantom types)
- Maintenance burden when adding/removing streams

Macro generation:

- Eliminates boilerplate entirely
- Developers declare streams once via `#[stream]`
- Generated code is correct by construction
- Changes to stream set automatically update generated code
- Reduces cognitive load on library users

This is core to NFR-2.1 (minimal boilerplate) and NFR-2.2 (compile-time safety).

**Why Integration with emit! Macro:**

Event emission needs type-safe stream association. The `emit!` macro:

- Verifies event is emitted to declared stream (compile-time check)
- Provides ergonomic syntax for event production
- Works with generated phantom types for access control
- Enables compiler errors with clear messages ("stream not declared in command")
- Without macro integration, developers would need verbose API calls with runtime checks.

**Why Separate eventcore-macros Crate:**

Rust ecosystem best practices for procedural macros:

- Proc macros require proc-macro = true in Cargo.toml
- Separate compilation improves build times
- Macro dependencies (syn, quote) don't pollute core library
- Standard pattern across Rust ecosystem (serde_derive, tokio-macros, etc.)
- Enables independent versioning if needed

## Consequences

**Positive:**

- **Minimal Boilerplate**: Developers write only business logic (apply, handle), no infrastructure code
- **Compile-Time Safety**: Invalid stream access caught by compiler before code runs
- **Type-Driven Development**: Aligns with ADR-003 patterns (validated types, making illegal states unrepresentable)
- **Clear Separation**: Infrastructure (generated) vs business logic (hand-written) boundaries explicit
- **Ergonomic API**: Declarative stream declarations feel natural to Rust developers
- **Multi-Stream Natural**: Multiple `#[stream]` fields handle ADR-001 multi-stream atomicity intuitively
- **Debuggable**: Generated code readable via cargo expand; errors reference original source
- **Ecosystem Alignment**: Follows established Rust patterns (derive macros, proc-macro crates)

**Negative:**

- **Macro Expertise Required**: EventCore maintainers need proc macro expertise for changes
- **Two Crates to Maintain**: eventcore and eventcore-macros must stay synchronized
- **Compilation Overhead**: Macro expansion adds to build times
- **Error Message Quality**: Macro errors less polished than hand-written code errors
- **Learning Requirement**: Developers must understand macro attributes and generated trait separation
- **Debugging Indirection**: Generated code adds layer between source and compiled code

**Enabled Future Decisions:**

- Additional field attributes can extend macro capabilities (validation, default streams, etc.)
- `emit!` macro can be enhanced with additional compile-time checks
- Generated code can optimize for common patterns (single-stream commands)
- Macro can generate helpful Debug implementations for state types
- Observability hooks can be injected into generated code
- Testing utilities can leverage macro-generated structure

**Constrained Future Decisions:**

- CommandStreams trait signature must remain compatible with generated code
- Field attribute syntax must be stable (breaking changes affect all consumers)
- Phantom type structure cannot change without breaking macro version
- Generated code must reference only stable core library APIs
- eventcore-macros versioning must track core library versions
- emit! macro must coordinate with generated StreamSet types

## Alternatives Considered

### Alternative 1: No Macro - Hand-Written Trait Implementations

Require developers to implement CommandStreams trait manually for every command.

**Rejected Because:**

- Violates NFR-2.1 (minimal boilerplate) - requires 20-50 lines per command
- Error-prone - easy to make mistakes in stream extraction or phantom type setup
- High cognitive load - developers must understand infrastructure internals
- Poor developer experience - repetitive boilerplate discourages adoption
- Doesn't leverage Rust's code generation capabilities
- Other Rust libraries successfully use derive macros for boilerplate reduction

### Alternative 2: Builder Pattern for Stream Declaration

Use builder methods to declare streams instead of field attributes:

```rust
impl CommandStreams for TransferMoney {
    type StreamSet = TransferMoneyStreamSet;

    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::try_from_streams(vec![
            self.from_account.clone(),
            self.to_account.clone(),
        ]).expect("valid stream declarations")
    }
}
```

**Rejected Because:**

- Still requires hand-written trait implementation (boilerplate not eliminated)
- Phantom type generation impossible without macro
- Compile-time safety lost (can't enforce stream access at type level)
- Stream declarations separated from field definitions
- No advantage over derive macro approach
- Verbose compared to declarative attributes

### Alternative 3: Macro Generates Both CommandStreams and CommandLogic

Generate complete command implementation including business logic stubs.

**Rejected Because:**

- Business logic (apply, handle) is domain-specific and must be hand-written
- Generated stubs provide no value (developer replaces them anyway)
- Blurs boundary between infrastructure and domain logic
- Developers might accidentally leave stub implementations (runtime bugs)
- Increased generated code size with no benefit
- Violates separation of concerns principle

### Alternative 4: Single Unified Trait (No Separation)

Combine infrastructure and business logic in single Command trait.

**Rejected Because:**

- Mixes generated and hand-written code in same trait implementation
- Testing becomes complex (mock infrastructure and business logic together)
- Cannot generate partial trait implementations in Rust (must be complete)
- Trait evolution affects both concerns simultaneously
- Loses clear architectural boundary
- Makes it harder to understand which methods are infrastructure vs domain

### Alternative 5: Configuration File for Stream Declarations

Declare streams in separate TOML/YAML file instead of code attributes.

**Rejected Because:**

- Stream declarations separated from command definitions (maintainability issue)
- No compile-time verification of configuration file correctness
- Build process complexity (must parse external files during macro expansion)
- IDE support diminished (can't navigate from file to command)
- Non-standard approach in Rust ecosystem
- Violates principle of locality (related code should be together)

### Alternative 6: Runtime Stream Access Validation Only

Skip compile-time phantom types, validate at runtime instead.

**Rejected Because:**

- Violates NFR-2.2 (compile-time safety) and ADR-003 principles
- Adds runtime overhead to every event emission
- Errors only discovered during tests or production
- Cannot leverage Rust's type system for correctness
- Requires extensive runtime testing to catch access violations
- Misses opportunity for zero-cost abstractions

### Alternative 7: Macro Generates Stream Access Methods

Generate individual access methods per stream instead of phantom types:

```rust
// Generated
impl TransferMoney {
    fn from_account_stream(&self) -> StreamAccessor { ... }
    fn to_account_stream(&self) -> StreamAccessor { ... }
}
```

**Rejected Because:**

- Still allows runtime errors (can pass wrong accessor to emit)
- Verbose API (named methods per stream)
- Phantom type approach more idiomatic in Rust
- Accessor pattern adds runtime overhead
- Generated API surface larger and more complex
- Type-level tracking superior for compile-time safety
- Generated API surface larger and more complex

### Alternative 8: Attribute Macro Instead of Derive Macro

Use `#[command]` attribute macro instead of `#[derive(Command)]`.

**Rejected Because:**

- Attribute macros can modify input (harder to reason about)
- Derive macros more familiar pattern in Rust (serde, Debug, etc.)
- Attribute macro syntax less discoverable
- No significant advantage over derive approach
- Derive macro clearer intent (generating trait implementation)
- Ecosystem convention favors derive for trait implementation

### Alternative 9: Include Dynamic Stream Discovery in Field Attributes

Support attribute syntax for dynamic streams:

```rust
#[stream(dynamic = "resolve_payment_streams")]
payment: PaymentId
```

**Rejected Because:**

- Overcomplicates attribute syntax and macro implementation
- Dynamic discovery better handled through separate StreamResolver trait
- Mixing static and dynamic declarations in attributes creates confusion
- FR-1.3 dynamic discovery is runtime concern, not compile-time declaration
- Simpler to keep macro focused on static stream declarations
- Dynamic streams can be added via CommandLogic implementation when needed

## References

- ADR-001: Multi-Stream Atomicity Implementation Strategy (multi-stream command operations)
- ADR-003: Type System Patterns for Domain Safety (validated types, phantom types)
- ADR-004: Error Handling Hierarchy (CommandError for business rules)
- REQUIREMENTS_ANALYSIS.md: FR-1.1 Stream Declaration
- REQUIREMENTS_ANALYSIS.md: FR-1.2 Type-Safe Stream Access
- REQUIREMENTS_ANALYSIS.md: FR-1.3 Dynamic Stream Discovery
- REQUIREMENTS_ANALYSIS.md: FR-5.2 Command Traits
- REQUIREMENTS_ANALYSIS.md: NFR-2.1 Minimal Boilerplate
- REQUIREMENTS_ANALYSIS.md: NFR-2.2 Compile-Time Safety
- CLAUDE.md: Command macro usage patterns
