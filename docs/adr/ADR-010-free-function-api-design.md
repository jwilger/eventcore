# ADR-010: Free Function API Design Philosophy

## Status

accepted - 2025-10-15

## Context

EventCore is an infrastructure library providing event sourcing capabilities to Rust applications. As the public API takes shape during I-001 implementation, a fundamental question arises: **should the API expose functionality through free functions or through methods on structs?**

**Key Forces:**

1. **Minimal API Surface**: Libraries should expose only what's necessary - NFR-2.1 emphasizes minimal boilerplate, which extends to API design
2. **Compiler-Driven Evolution**: The Rust compiler is excellent at identifying what must be public vs what can remain private
3. **Composability**: Free functions are more composable than methods - they can be passed as function pointers, wrapped easily, and combined
4. **Developer Mental Model**: Simple APIs are easier to understand and use - fewer types to learn means faster onboarding
5. **Testability**: Free functions with explicit dependencies are easier to test than methods with implicit state
6. **API Stability**: Smaller public API surface means fewer breaking changes in future versions
7. **Type System Integration**: Rust's type system works well with free functions that take explicit type parameters
8. **Macro Design Alignment**: ADR-006 establishes that `#[derive(Command)]` generates infrastructure boilerplate - free functions complement this by keeping the runtime API minimal
9. **NFR-2.2 Composability**: Free functions enable function composition patterns that methods cannot support

**Discovered During I-001:**

During technical architecture review for I-001, the codebase contains a free `execute()` function, but ADR-008 and ARCHITECTURE.md reference a `CommandExecutor` struct. This inconsistency prompted clarification: should execution be a method (`executor.execute(cmd)`) or a free function (`execute(cmd, store)`)?

The user's decision: **prefer free functions, making types public only when the compiler requires it**.

**Why This Decision Now:**

I-001 establishes EventCore's first public API. The free function vs struct method decision affects every subsequent increment. Defining this philosophy now ensures API consistency and prevents API churn from switching approaches mid-development.

## Decision

EventCore's public API SHALL consist primarily of free functions, with types made public only when the compiler or testing requirements force the issue.

**1. API Design Principles**

Free functions are the default API style:

```rust
// Preferred: Free function with explicit dependencies
pub async fn execute<C, S>(
    command: C,
    store: &S,
) -> Result<ExecutionResponse, CommandError>
where
    C: CommandLogic, // CommandLogic now implies CommandStreams
    S: EventStore,
{
    // Implementation
}

// Not preferred: Method on struct
pub struct CommandExecutor<S> {
    store: S,
}

impl<S: EventStore> CommandExecutor<S> {
    pub async fn execute<C>(&self, command: C) -> Result<ExecutionResponse, CommandError>
    where
        C: CommandLogic,
    {
        // Implementation
    }
}
```

**2. Type Visibility Strategy**

Types remain private by default:

- **Start Private**: All types, structs, enums begin as private (`pub(crate)` or module-private)
- **Compiler-Forced Public**: Make public when compiler errors indicate a type appears in public API
- **Test-Forced Public**: Make public when integration tests (from consumer perspective) require access
- **Never Speculative**: Do not make types public "just in case" - wait for actual need

**3. When Structs Are Appropriate**

Structs are used when they provide clear value:

- **Configuration Objects**: Grouping related configuration (e.g., `RetryPolicy` with max_attempts, base_delay)
- **Result Types**: Returning multiple related values (e.g., `ExecutionResponse` with event count, versions)
- **Builder Patterns**: When construction requires validation or multiple steps
- **State Machines**: When typestate pattern enforces valid state transitions

Structs should still expose functionality through free functions when possible.

**4. Trait-Based Abstraction**

EventCore uses traits for polymorphism, not struct hierarchies:

- `EventStore` trait abstracts storage backends
- `CommandLogic` (which implies `CommandStreams`) defines command behavior
- Free functions accept trait bounds, enabling implementation flexibility
- Consumers provide implementations; EventCore provides free functions that operate on them

**5. Function Signature Style**

Free functions use explicit dependency injection:

```rust
// Good: Dependencies explicit in signature
pub async fn execute<C, S>(
    command: C,
    store: &S,
) -> Result<ExecutionResponse, CommandError>

// Avoid: Dependencies hidden in struct
pub async fn execute<C>(command: C) -> Result<ExecutionResponse, CommandError>
// Where does EventStore come from? Global? Magic?
```

**6. API Evolution Path**

As EventCore grows:

1. Start with free function accepting all dependencies
2. If callers need to share configuration, introduce config struct
3. If config struct becomes complex, add builder
4. Never introduce struct just for namespacing - use modules instead

**7. Documentation and Discoverability**

Free functions require excellent documentation:

- Module-level docs explain overall patterns
- Function docs include examples showing typical usage
- Re-exports at crate root for commonly-used functions
- IDE autocomplete works well with free functions and trait imports

## Rationale

**Why Free Functions Over Methods:**

Free functions provide superior composability and testability:

- **Explicit Dependencies**: All inputs visible in signature, no hidden state
- **Function Composition**: Can be passed as function pointers, wrapped in closures, composed with combinators
- **Easier Testing**: Mock dependencies passed explicitly, no need to construct complex structs
- **Clearer Ownership**: Who owns what is explicit (parameters vs struct fields)
- **Better Type Inference**: Rust's type inference works better with free functions than methods with associated types
- **Aligns with Rust Ecosystem**: Many successful Rust libraries prefer free functions (tokio::spawn, serde_json::to_string, etc.)

Methods on structs add ceremony without benefit in infrastructure libraries.

**Why Compiler-Driven Public Types:**

The compiler knows what must be public better than developers:

- **No Speculation**: Developers are poor predictors of what will be needed
- **Minimal Exposure**: Only types actually used in public API become public
- **Forced Documentation**: When type becomes public, forces consideration of documentation
- **Prevents API Bloat**: Reduces surface area, making future changes easier
- **Clear Intent**: Public types are public because they must be, not might be

Alternative (make everything public) creates massive API surface and maintenance burden.

**Why This Aligns with NFR-2.2 (Composability):**

Free functions are inherently more composable:

- Can be partially applied via closures
- Work naturally with function combinators (map, and_then, etc.)
- Enable point-free programming styles
- Easier to wrap for cross-cutting concerns (logging, metrics, etc.)
- Methods require more ceremony to achieve same composability

**Why This Complements ADR-006 (Command Macro):**

ADR-006 establishes that `#[derive(Command)]` generates infrastructure boilerplate. Free functions complement this:

- Macro generates traits that free functions accept as bounds
- Developer writes zero infrastructure code
- Free functions provide runtime behavior using generated traits
- Clean separation: compile-time generation + runtime functions
- No need for executor struct - `execute()` function operates on command traits directly

**Why This Simplifies Learning Curve:**

Fewer types means faster developer onboarding:

- Learn `execute()` function signature, start using EventCore immediately
- No need to understand `CommandExecutor` lifecycle, configuration, construction
- Trade-off: Must pass `EventStore` each time vs storing in executor
- Benefit: Explicit dependencies make data flow obvious
- Result: 30-minute onboarding target (I-001 success metric) more achievable

**Trade-offs Accepted:**

- **Parameter Repetition**: Callers pass same dependencies (e.g., `store`) to multiple functions
- **No Shared State**: Cannot cache expensive setup across calls without application-level management
- **Discovery Challenge**: Developers must find functions via docs/examples, not method autocomplete
- **Verbosity**: Function calls may be more verbose than method calls
- **Less Object-Oriented**: Developers from OOP backgrounds may find style unfamiliar

These trade-offs are acceptable because:

- Parameter repetition is minimal in typical usage (one `execute()` call per request)
- Shared state can be achieved through explicit config structs when needed
- Good documentation mitigates discovery challenge
- Verbosity is offset by explicitness and clarity
- Rust ecosystem trends toward functional style; EventCore aligns with community norms

## Consequences

**Positive:**

- **Minimal API Surface**: Only essential types are public, reducing learning curve
- **Maximum Flexibility**: Callers choose how to manage dependencies (pass each time, wrap in closure, build helper)
- **Testability**: Explicit dependencies enable easy mocking and property testing
- **Composability**: Free functions work naturally with function combinators and higher-order functions
- **Clear Ownership**: No confusion about who owns what - parameters make it explicit
- **Type Inference**: Rust compiler infers types better with free functions
- **Ecosystem Alignment**: Matches patterns from successful Rust libraries (tokio, serde, etc.)
- **Future Flexibility**: Can introduce structs later without breaking existing code (free functions continue working)

**Negative:**

- **Parameter Repetition**: Common dependencies passed repeatedly (mitigated by closures/wrappers)
- **Discovery Friction**: Developers must learn which functions exist (mitigated by documentation)
- **Unfamiliar Pattern**: OOP developers may expect executor struct (mitigated by examples)
- **No Shared Setup**: Cannot amortize expensive initialization across calls (mitigated by application-level caching)
- **Verbose Call Sites**: More parameters than method call (accepted for explicitness)

**Enabled Future Decisions:**

- Configuration structs can be introduced when actual need arises
- Builder patterns can wrap free functions for complex setup scenarios
- Facade functions can provide simplified API for common cases
- Extension crates can build higher-level abstractions on free function foundation
- Async executor pools can wrap `execute()` for resource management
- Testing utilities can provide mock factories for common scenarios

**Constrained Future Decisions:**

- Primary API must remain free functions to maintain consistency
- New functionality should default to free functions unless clear struct need
- Types stay private until compiler or tests force public
- Cannot introduce breaking changes to free function signatures (parameter order, types)
- Must provide excellent documentation since method autocomplete doesn't help

## Alternatives Considered

... (rest unchanged)
