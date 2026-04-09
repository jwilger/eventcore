# Acceptance Test Boundaries

Acceptance tests must exercise the system from the perspective of a downstream
library consumer. The consumer is a Rust developer calling the public API.

## Library Features

Integration tests MUST call the public API: `eventcore::execute()`,
`run_projection()`, public trait methods (`CommandLogic`, `EventStore`,
`Projector`), and macro-generated code (`#[derive(Command)]`, `require!`,
`emit!`).

Tests must not reach into internal modules, bypass `execute()` to call
`apply`/`handle` directly, or construct internal types that are not part of
the public API.

## Backend Implementations

Contract tests in `eventcore-testing` verify that `EventStore` and
`EventReader` implementations satisfy the required behavioral contracts.
Each backend crate (postgres, sqlite, memory) runs these contract tests
against its implementation.

## The Litmus Test

> "Could a downstream developer perform this operation using only the
> published API?"

If a test calls internal functions, constructs types from private modules,
or bypasses `execute()`, it is not testing the user's experience of the
library.

## Unit Tests

Unit tests within a crate may test internal functions directly. They are
permitted and sometimes valuable, but they:

- Do NOT count as acceptance tests for public API behavior
- Do NOT replace the requirement for an integration test exercising the
  public API
- Should be used when drill-down discipline requires narrower scope

## Why

The gap between "the internal logic works" and "the consumer can use the
API" is where bugs hide. Missing re-exports, broken macro expansions,
type inference failures, and trait bound issues are invisible to unit tests.
Integration tests catch all of these because they exercise the same path
a real consumer takes.
