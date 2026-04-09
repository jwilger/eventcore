# Design Principles

These principles govern architectural decisions across the eventcore library.

## Correctness Over Throughput

Multi-stream atomicity, optimistic concurrency detection, and event immutability
are non-negotiable. Performance optimizations must preserve these guarantees —
they happen within atomic transaction boundaries, not by relaxing them.

## Infrastructure Neutrality

The library owns infrastructure concerns (stream management, retries, metadata,
storage abstraction) and never assumes a particular business domain. Applications
own their domain events, metadata schemas, and business rules. EventCore does
not impose assumptions about "users" or "actors."

## Free-Function APIs

Public entry points are free functions with explicit dependencies:

```rust
execute(store, command, policy)
run_projection(projector, &backend)
```

Prefer this pattern over builder structs or intermediate types. Structs exist
only when grouping configuration or results adds clarity (e.g., `RetryPolicy`,
`ExecutionResponse`).

## Developer Ergonomics

The `#[derive(Command)]` macro generates all infrastructure boilerplate.
Developers write only domain code (state reconstruction and business logic).
Automatic retries, contract-test tooling, and in-memory storage support fast
onboarding — a working command should require minimal ceremony.
