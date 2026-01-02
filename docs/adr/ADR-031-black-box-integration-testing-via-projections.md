# ADR-031: Black-Box Integration Testing via Projections

## Status

Accepted

## Date

2026-01-01

## Deciders

- John Wilger

## Context

Integration tests in `eventcore/tests/` need to verify that command execution stored events correctly. The current approach calls `store.read_stream()` directly, which requires the `EventStore` trait to be in scope.

**Problem: API Surface Conflict**

Per ADR-030 (Layered Crate Public API Design), `EventStore` is a backend-implementer type that belongs in `eventcore-types`, not in the application developer API exported by `eventcore`. Application developers should not need to import `EventStore` - they use `execute()` and `run_projection()`.

If integration tests require `EventStore` to verify behavior, we have two options:

1. **Re-export `EventStore` from `eventcore`** - Rejected because it violates ADR-030's strict separation between application developer and backend implementer APIs.

2. **Re-export `EventStore` from `eventcore-testing`** - This works for most cases, but creates complexity if `eventcore`'s own integration tests need the trait (circular dependency between `eventcore` tests and `eventcore-testing`).

3. **Rewrite tests to use projections** - The event sourcing way to query state.

**The Event Sourcing Insight**

In event sourcing, the intended mechanism for querying state is through projections (read models), not by peeking at raw events in the store. If application tests need to read events directly, that suggests either:

- The test is testing infrastructure, not application behavior
- The public API is incomplete for the application's needs

Application-level integration tests should verify outcomes using the same APIs that real applications use. If the test needs to verify "these events were stored," the application code would use a projection to achieve the same verification.

**Forces at Play:**

- Tests should use the same APIs that real applications use (dogfooding)
- The `EventStore` trait is a backend-implementer concern, not an application concern
- Projections are the event sourcing mechanism for reading/querying state
- Tests should document intended usage patterns
- ADR-015 established that test helpers belong in `eventcore-testing`

## Decision

We will verify command execution results via projections, not direct event store access.

Specifically:

1. **Create `EventCollector<E>` projector** in `eventcore-testing` that collects all events of type `E` for assertion purposes.

2. **Integration test pattern**:
   ```rust
   // Execute command
   execute(&mut store, &command).await?;

   // Collect events via projection
   let collector = EventCollector::<MyDomainEvent>::new();
   run_projection(collector, &backend).await?;

   // Assert on collected events
   assert_eq!(collector.events(), vec![expected_event]);
   ```

3. **This is true black-box testing** - tests use exactly the same APIs that real applications use:
   - `execute()` to run commands
   - `run_projection()` to read state

4. **No `EventStore` trait in application code** - The trait remains exclusively in `eventcore-types` for backend implementers.

## Alternatives Considered

### Alternative 1: Re-export EventStore from eventcore

**Description**: Add `EventStore` to the `eventcore` crate's public API.

**Pros**:
- Simple - tests can call `store.read_stream()` directly
- No additional infrastructure needed

**Cons**:
- Violates ADR-030's strict separation of concerns
- Exposes backend-implementer types to application developers
- Increases API surface unnecessarily
- Creates confusion about when to use projections vs direct reads

**Why rejected**: Fundamentally conflicts with the layered API design decision.

### Alternative 2: Re-export EventStore from eventcore-testing

**Description**: The testing crate could re-export `EventStore` for test-only use.

**Pros**:
- Keeps `EventStore` out of the main `eventcore` API
- Test code could still use direct event access

**Cons**:
- If `eventcore`'s own integration tests need this, circular dependency ensues
- Still encourages non-idiomatic testing patterns
- Tests don't demonstrate intended usage

**Why rejected**: Creates dependency complexity and encourages anti-patterns.

### Alternative 3: Allow tests to import from eventcore-types directly

**Description**: Tests that need backend types just import from `eventcore-types`.

**Pros**:
- No changes needed to crate structure
- Clear that test is using backend-level APIs

**Cons**:
- Tests don't mirror real application usage
- Doesn't provide reusable test utilities
- Still requires understanding of backend-implementer APIs

**Why rejected**: Misses the opportunity to demonstrate and test the projection-based approach.

## Consequences

### Positive

- **Dogfooding**: Tests prove the public API works as intended by using it
- **Clean API boundary**: `EventStore` trait stays exclusively in `eventcore-types`
- **Documentation through tests**: Integration tests demonstrate the intended usage pattern (commands + projections)
- **Reusable utilities**: `EventCollector` becomes a useful tool for any test that needs to inspect events
- **Aligns with event sourcing philosophy**: Read models/projections are THE mechanism for querying state

### Negative

- **Slightly more test boilerplate**: Need to set up projector and run projection instead of simple `read_stream()` call
- **Async complexity**: `run_projection()` is async, adding to test complexity
- **Learning curve**: Developers must understand projections to write tests

### Neutral

- Changes the mental model from "peek at events" to "project then assert"
- Forces test authors to think about what they're actually testing

## Related Decisions

- [ADR-030: Layered Crate Public API Design](ADR-030-layered-crate-public-api.md) - Establishes that `EventStore` is not part of application developer API
- [ADR-015: Testing Crate Scope](ADR-015-testing-crate-scope.md) - Establishes that test helpers belong in `eventcore-testing`
- [ADR-019: Projector Trait](ADR-019-projector-trait.md) - Defines the projector abstraction
- [ADR-029: Projection Runner API Simplification](ADR-029-projection-runner-api-simplification.md) - Establishes `run_projection()` as the preferred entry point

## References

- Event Sourcing pattern: https://martinfowler.com/eaaDev/EventSourcing.html
- CQRS pattern: https://martinfowler.com/bliki/CQRS.html
