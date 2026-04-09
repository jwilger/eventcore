# Public API Coverage

Every public API surface must have at least one integration test proving a
downstream consumer can use it through the documented entry points.

## What Coverage Means

A public API is "covered" when an integration test exercises it through the
crate's public interface — calling `execute()`, constructing commands,
running projections, or using macro-generated code — without reaching into
internal modules.

## What Is Required

For every public API path:

1. **A usage test** that calls the API the way a downstream consumer would.
2. **A behavioral assertion** that verifies the API produces the correct
   observable outcome (events appended, projection state updated, errors
   returned).

Tests that only exercise internal functions prove correctness but NOT
usability. Both are needed.

## What This Catches

- Missing re-exports from `lib.rs`
- Broken macro expansions that compile internally but fail for consumers
- Trait bound issues that only surface when using the public API
- Type inference failures at the consumer boundary
- Documentation examples that don't actually work

## Applies To

All public items, including:

- Free functions (`execute`, `run_projection`)
- Public traits and their required methods
- Macro-generated code (`#[derive(Command)]`, `require!`, `emit!`)
- Feature-gated re-exports (postgres, sqlite backends)
- Error types and their variants

## Why

A function that works internally but cannot be called through the public
API is dead code from the consumer's perspective. Coverage tests ensure the
library is usable end-to-end, not just correct in isolation.
