# Blueprints

Technical documentation for this project's architecture and systems.

## When to Consult Blueprints

Before modifying system architecture, use Glob and Read on the `blueprints/` directory to understand:

- Current design decisions and rationale
- Integration points and dependencies
- Established patterns to follow

## Key Triggers

Consult blueprints when working on:

- GraphQL schema changes
- CLI command modifications
- MCP server integrations
- Plugin architecture changes
- Database schema updates
- Hook system modifications

## After Modifications

Update blueprints using the Write tool on `blueprints/{name}.md` when you:

- Add new systems or major features
- Change architectural patterns
- Discover undocumented conventions

## Available Blueprints

<!-- AUTO-GENERATED INDEX - DO NOT EDIT BELOW THIS LINE -->

| Blueprint              | Summary                                                                                                         |
| ---------------------- | --------------------------------------------------------------------------------------------------------------- |
| command-execution      | Command pattern with pure apply/handle, automatic retry, and dynamic stream discovery.                          |
| event-sourcing         | Core event sourcing model with multi-stream atomic writes, optimistic concurrency, and immutable event storage. |
| macro-codegen          | Procedural macros generating CommandStreams implementations and business rule validation helpers.               |
| projection-system      | Poll-based projection runner with checkpoint resumption, leader election, and configurable retry.               |
| store-backends         | Pluggable EventStore implementations for PostgreSQL, SQLite, and in-memory testing.                             |
| testing-infrastructure | Contract tests, chaos harness, and deterministic testing tools for verifying EventStore backends.               |
| type-system            | Semantic domain types with nutype validation enforcing parse-don't-validate at construction boundaries.         |
