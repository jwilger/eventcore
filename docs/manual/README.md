# EventCore User Manual

Welcome to the EventCore documentation! This manual provides comprehensive guidance for using EventCore, a type-safe event sourcing library that implements multi-stream event sourcing with dynamic consistency boundaries.

## Manual Organization

This manual is organized into the following sections:

### 🚀 [Getting Started](./getting-started)
New to EventCore? Start here to understand the core concepts and why EventCore might be the right choice for your project.

- [Why EventCore? A Decision Guide](./getting-started/why-eventcore.md) - Understand when EventCore is the right choice
- [EventCore vs Traditional Event Sourcing](./getting-started/eventcore-vs-alternatives.md) - Compare EventCore's approach with alternatives

### 📚 [Tutorials](./tutorials)
Step-by-step guides to help you build with EventCore.

- [Writing Your First Command](./tutorials/first-command.md) - Build a simple bank account system
- [Implementing Projections](./tutorials/implementing-projections.md) - Create read models from your events
- [Error Handling Best Practices](./tutorials/error-handling.md) - Handle errors gracefully in EventCore
- [Using the Command Macro DSL](./tutorials/macro-dsl.md) - Simplify command creation with macros
- [Building Distributed Systems](./tutorials/distributed-systems.md) - Scale EventCore across services
- [Projection Rebuild Strategies](./tutorials/projection-rebuild.md) - Manage projection rebuilds effectively

### 🔧 [Core Concepts](./core-concepts)
Deep dives into EventCore's architecture and design principles.

*(Documentation coming soon)*

### 🎯 [Advanced Topics](./advanced)
Advanced features and patterns for complex use cases.

- [CQRS Implementation with EventCore](./advanced/cqrs-design.md) - Build full CQRS systems
- [CQRS API Reference](./advanced/cqrs-api-summary.md) - Complete API documentation
- [CQRS Rebuild Reference](./advanced/cqrs-rebuild-reference.md) - Detailed rebuild capabilities
- [Schema Evolution Strategies](./advanced/schema-evolution.md) - Handle event schema changes over time

### 🔌 [Integration](./integration)
Connect EventCore with other technologies and frameworks.

- [Web Framework Integration](./integration/web-framework-integration.md) - Use EventCore with Actix, Axum, etc.
- [Serialization Format Support](./integration/serialization-formats.md) - Beyond JSON: MessagePack, CBOR, and more

### 🚦 [Operations](./operations)
Deploy and operate EventCore in production.

- [Deployment Guide](./operations/deployment-guide.md) - Deploy EventCore applications
- [Operations Guide](./operations/operations-guide.md) - Day-to-day operations
- [Monitoring and Observability](./operations/monitoring-and-observability.md) - Monitor your event store
- [Troubleshooting Guide](./operations/troubleshooting.md) - Common issues and solutions

### 📖 [Reference](./reference)
Detailed API documentation and performance characteristics.

- [Enhanced Command Macro Reference](./reference/enhanced-command-macro.md) - Complete macro documentation
- [Performance Characteristics](./reference/performance-characteristics.md) - Understanding EventCore's performance

### 🔄 [Migration](./migration)
Guides for migrating existing code to newer EventCore versions.

- [Command Trait Separation Guide](./migration/migration-guide-trait-separation.md) - Migrate to the simplified trait model
- [Input Type Removal Guide](./migration/migration-guide-input-removal.md) - Simplify command implementations
- [General Migration Guide](./migration/migration-guide.md) - Comprehensive migration strategies

## Quick Links

- [GitHub Repository](https://github.com/jwilger/eventcore)
- [API Documentation](https://docs.rs/eventcore)
- [Examples](https://github.com/jwilger/eventcore/tree/main/eventcore-examples)

## Getting Help

- **Issues**: Report bugs or request features on [GitHub Issues](https://github.com/jwilger/eventcore/issues)
- **Discussions**: Join the community on [GitHub Discussions](https://github.com/jwilger/eventcore/discussions)

## License

EventCore is dual-licensed under MIT and Apache 2.0. See the [LICENSE files](https://github.com/jwilger/eventcore) for details.