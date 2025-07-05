# EventCore Manual

Welcome to the EventCore manual! This comprehensive guide will take you from understanding event sourcing concepts through building production-ready applications.

## 📚 Table of Contents

### [Part 1: Introduction](./01-introduction/README.md)
Start here if you're new to EventCore or event sourcing.

- [Chapter 1.1: What is EventCore?](./01-introduction/01-what-is-eventcore.md)
- [Chapter 1.2: When to Use EventCore](./01-introduction/02-when-to-use-eventcore.md)
- [Chapter 1.3: Event Modeling Fundamentals](./01-introduction/03-event-modeling.md)
- [Chapter 1.4: Architecture Overview](./01-introduction/04-architecture.md)

### [Part 2: Getting Started](./02-getting-started/README.md)
A complete walkthrough building a task management system.

- [Chapter 2.1: Setting Up Your Project](./02-getting-started/01-setup.md)
- [Chapter 2.2: Modeling the Domain](./02-getting-started/02-domain-modeling.md)
- [Chapter 2.3: Implementing Commands](./02-getting-started/03-commands.md)
- [Chapter 2.4: Working with Projections](./02-getting-started/04-projections.md)
- [Chapter 2.5: Testing Your Application](./02-getting-started/05-testing.md)

### [Part 3: Core Concepts](./03-core-concepts/README.md)
Deep dive into EventCore's design and features.

- [Chapter 3.1: Commands and the Macro System](./03-core-concepts/01-commands-and-macros.md)
- [Chapter 3.2: Events and Event Stores](./03-core-concepts/02-events-and-stores.md)
- [Chapter 3.3: State Reconstruction](./03-core-concepts/03-state-reconstruction.md)
- [Chapter 3.4: Multi-Stream Atomicity](./03-core-concepts/04-multi-stream-atomicity.md)
- [Chapter 3.5: Error Handling](./03-core-concepts/05-error-handling.md)

### [Part 4: Building Web APIs](./04-building-apis/README.md)
Integrate EventCore with web frameworks.

- [Chapter 4.1: API Design Principles](./04-building-apis/01-api-design.md)
- [Chapter 4.2: Axum Integration](./04-building-apis/02-axum-integration.md)
- [Chapter 4.3: Authentication and Authorization](./04-building-apis/03-auth.md)
- [Chapter 4.4: Real-time Updates with WebSockets](./04-building-apis/04-websockets.md)

### [Part 5: Advanced Topics](./05-advanced-topics/README.md)
Advanced patterns and lower-level APIs.

- [Chapter 5.1: CQRS and Read Models](./05-advanced-topics/01-cqrs.md)
- [Chapter 5.2: Schema Evolution](./05-advanced-topics/02-schema-evolution.md)
- [Chapter 5.3: Distributed Systems Patterns](./05-advanced-topics/03-distributed-systems.md)
- [Chapter 5.4: Lower-Level APIs](./05-advanced-topics/04-lower-level-apis.md)
- [Chapter 5.5: Performance Optimization](./05-advanced-topics/05-performance.md)

### [Part 6: Operations](./06-operations/README.md)
Deploy and operate EventCore applications.

- [Chapter 6.1: Deployment Strategies](./06-operations/01-deployment.md)
- [Chapter 6.2: Monitoring and Observability](./06-operations/02-monitoring.md)
- [Chapter 6.3: Backup and Recovery](./06-operations/03-backup-recovery.md)
- [Chapter 6.4: Troubleshooting](./06-operations/04-troubleshooting.md)

### [Part 7: Reference](./07-reference/README.md)
API documentation and reference material.

- [Chapter 7.1: API Reference](./07-reference/01-api-reference.md)
- [Chapter 7.2: Configuration Reference](./07-reference/02-configuration.md)
- [Chapter 7.3: Migration Guides](./07-reference/03-migration-guides.md)
- [Chapter 7.4: Glossary](./07-reference/04-glossary.md)

## 🚀 Quick Start

If you want to jump right in:

1. Read [What is EventCore?](./01-introduction/01-what-is-eventcore.md) (5 minutes)
2. Follow the [Getting Started Tutorial](./02-getting-started/README.md) (30 minutes)
3. Build your first web API with [Chapter 4](./04-building-apis/README.md)

## 📖 Reading Path

### For Event Sourcing Beginners
1. Start with [Event Modeling Fundamentals](./01-introduction/03-event-modeling.md)
2. Work through the complete [Getting Started](./02-getting-started/README.md) tutorial
3. Study [Core Concepts](./03-core-concepts/README.md) as needed

### For Experienced Developers
1. Skim [Architecture Overview](./01-introduction/04-architecture.md)
2. Jump to [Commands and the Macro System](./03-core-concepts/01-commands-and-macros.md)
3. Review [API examples](./04-building-apis/02-axum-integration.md)

### For Production Deployment
1. Review [Multi-Stream Atomicity](./03-core-concepts/04-multi-stream-atomicity.md) guarantees
2. Study [Operations](./06-operations/README.md) thoroughly
3. Understand [Performance Optimization](./05-advanced-topics/05-performance.md)

## 💡 Key Features Covered

- **Event Modeling**: Learn to design systems using events
- **Type-Driven Development**: Use Rust's type system for correctness
- **Multi-Stream Commands**: Atomic operations across multiple streams
- **Production Ready**: Monitoring, deployment, and operations
- **Web Integration**: Build REST and WebSocket APIs

## 🛠 Prerequisites

- Basic Rust knowledge (ownership, traits, async/await)
- Familiarity with web development concepts
- PostgreSQL for production deployments

## 📝 License

This manual is part of the EventCore project, dual-licensed under MIT and Apache 2.0.