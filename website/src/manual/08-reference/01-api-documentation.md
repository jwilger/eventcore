# API Documentation

The complete EventCore API documentation is generated from the source code using rustdoc.

<div style="text-align: center; margin: 2rem 0;">
  <a href="../../api/eventcore/index.html" class="primary-button" target="_blank">View API Documentation</a>
</div>

The API documentation includes:

- **Complete type and trait references** - All public types, traits, and functions
- **Usage examples** - Code examples demonstrating common patterns
- **Module documentation** - Overview and guidance for each module
- **Cross-references** - Links between related types and concepts

## Key Modules

### Core Library

- **[`eventcore`](../../api/eventcore/index.html)** - Core library with command execution, event stores, and projections
- **[`eventcore::prelude`](../../api/eventcore/prelude/index.html)** - Common imports for EventCore applications

### Event Store Adapters

- **[`eventcore_postgres`](../../api/eventcore_postgres/index.html)** - PostgreSQL event store adapter
- **[`eventcore_memory`](../../api/eventcore_memory/index.html)** - In-memory event store for testing

### Derive Macros

- **[`eventcore_macros`](../../api/eventcore_macros/index.html)** - Derive macros for commands

## Quick Reference

For quick access to commonly used items:

- [`Command`](../../api/eventcore/trait.Command.html) - Core command trait
- [`CommandExecutor`](../../api/eventcore/struct.CommandExecutor.html) - Executes commands
- [`EventStore`](../../api/eventcore/trait.EventStore.html) - Event persistence trait
- [`Projection`](../../api/eventcore/trait.Projection.html) - Read model projections
- [`StreamId`](../../api/eventcore/struct.StreamId.html) - Stream identifier
- [`EventId`](../../api/eventcore/struct.EventId.html) - Event identifier