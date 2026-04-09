# Blueprints

Technical documentation for this project's architecture and systems.

## When to Consult Blueprints

Before modifying system architecture, use Glob and Read on the `blueprints/`
directory to understand:

- Current design decisions and rationale
- Integration points and dependencies
- Established patterns to follow

## Key Triggers

Consult blueprints when working on:

- Event store backend implementations
- Command execution and retry logic
- Projection system and coordination
- Macro codegen (`#[derive(Command)]`, `require!`, `emit!`)
- Trait design (`CommandLogic`, `EventStore`, `Projector`)
- Testing infrastructure and contract tests

## After Modifications

Update blueprints using the Write tool on `blueprints/{name}.md` when you:

- Add new systems or major features
- Change architectural patterns
- Discover undocumented conventions

## Available Blueprints

<!-- AUTO-GENERATED INDEX - DO NOT EDIT BELOW THIS LINE -->

| Blueprint                                                            | Summary                                                                                                         |
| -------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| [command-execution](../../blueprints/command-execution.md)           | Command pattern with pure apply/handle, automatic retry, and dynamic stream discovery.                          |
| [event-sourcing](../../blueprints/event-sourcing.md)                 | Core event sourcing model with multi-stream atomic writes, optimistic concurrency, and immutable event storage. |
| [macro-codegen](../../blueprints/macro-codegen.md)                   | Procedural macros generating CommandStreams implementations and business rule validation helpers.               |
| [projection-system](../../blueprints/projection-system.md)           | Poll-based projection runner with checkpoint resumption, leader election, and configurable retry.               |
| [store-backends](../../blueprints/store-backends.md)                 | Pluggable EventStore implementations for PostgreSQL, SQLite, and in-memory testing.                             |
| [testing-infrastructure](../../blueprints/testing-infrastructure.md) | Contract tests, chaos harness, and deterministic testing tools for verifying EventStore backends.               |
| [type-system](../../blueprints/type-system.md)                       | Semantic domain types with nutype validation enforcing parse-don't-validate at construction boundaries.         |
