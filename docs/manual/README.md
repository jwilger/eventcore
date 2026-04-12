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

### [Part 4: Building Web APIs](./04-building-web-apis/README.md)

Integrate EventCore with web frameworks.

- [Chapter 4.1: Setting Up Endpoints](./04-building-web-apis/01-setting-up-endpoints.md)
- [Chapter 4.2: Command Handlers](./04-building-web-apis/02-command-handlers.md)
- [Chapter 4.3: Query Endpoints](./04-building-web-apis/03-query-endpoints.md)
- [Chapter 4.4: Authentication](./04-building-web-apis/04-authentication.md)
- [Chapter 4.5: API Versioning](./04-building-web-apis/05-api-versioning.md)

### [Part 5: Advanced Topics](./05-advanced-topics/README.md)

Advanced patterns and lower-level APIs.

- [Chapter 5.1: Schema Evolution](./05-advanced-topics/01-schema-evolution.md)
- [Chapter 5.2: Event Versioning](./05-advanced-topics/02-event-versioning.md)
- [Chapter 5.3: Long-Running Processes](./05-advanced-topics/03-long-running-processes.md)
- [Chapter 5.4: Distributed Systems](./05-advanced-topics/04-distributed-systems.md)
- [Chapter 5.5: Performance Optimization](./05-advanced-topics/05-performance-optimization.md)

### [Part 6: Security](./06-security/README.md)

Security considerations for EventCore applications.

- [Chapter 6.1: Overview](./06-security/01-overview.md)
- [Chapter 6.2: Authentication](./06-security/02-authentication.md)
- [Chapter 6.3: Encryption](./06-security/03-encryption.md)
- [Chapter 6.4: Validation](./06-security/04-validation.md)
- [Chapter 6.5: Compliance](./06-security/05-compliance.md)

### [Part 7: Operations](./07-operations/README.md)

Deploy and operate EventCore applications.

- [Chapter 7.1: Deployment Strategies](./07-operations/01-deployment-strategies.md)
- [Chapter 7.2: Monitoring and Metrics](./07-operations/02-monitoring-metrics.md)
- [Chapter 7.3: Backup and Recovery](./07-operations/03-backup-recovery.md)
- [Chapter 7.4: Troubleshooting](./07-operations/04-troubleshooting.md)
- [Chapter 7.5: Production Checklist](./07-operations/05-production-checklist.md)

### [Part 8: Reference](./08-reference/README.md)

API documentation and reference material.

- [Chapter 8.1: API Documentation](./08-reference/01-api-documentation.md)
- [Chapter 8.2: Configuration Reference](./08-reference/02-configuration-reference.md)
- [Chapter 8.3: Error Reference](./08-reference/03-error-reference.md)
- [Chapter 8.4: Glossary](./08-reference/04-glossary.md)

## 🚀 Quick Start

If you want to jump right in:

1. Read [What is EventCore?](./01-introduction/01-what-is-eventcore.md) (5 minutes)
2. Follow the [Getting Started Tutorial](./02-getting-started/README.md) (30 minutes)
3. Build your first web API with [Chapter 4](./04-building-web-apis/README.md)

## 📖 Reading Path

### For Event Sourcing Beginners

1. Start with [Event Modeling Fundamentals](./01-introduction/03-event-modeling.md)
2. Work through the complete [Getting Started](./02-getting-started/README.md) tutorial
3. Study [Core Concepts](./03-core-concepts/README.md) as needed

### For Experienced Developers

1. Skim [Architecture Overview](./01-introduction/04-architecture.md)
2. Jump to [Commands and the Macro System](./03-core-concepts/01-commands-and-macros.md)
3. Review [API examples](./04-building-web-apis/02-command-handlers.md)

### For Production Deployment

1. Review [Multi-Stream Atomicity](./03-core-concepts/04-multi-stream-atomicity.md) guarantees
2. Study [Operations](./07-operations/README.md) thoroughly
3. Understand [Performance Optimization](./05-advanced-topics/05-performance-optimization.md)

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

This manual is part of the EventCore project, licensed under the MIT License.
