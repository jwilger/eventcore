# Chapter 7.4: Glossary

This glossary defines all terms and concepts used throughout EventCore documentation. Use this as a reference to understand EventCore terminology and concepts.

## Core Concepts

### Aggregate

In traditional event sourcing, an aggregate is a cluster of domain objects that can be treated as a single unit. EventCore eliminates traditional aggregates in favor of dynamic consistency boundaries defined by commands.

### Command

A request to change the state of the system by writing events to one or more streams. Commands in EventCore can read from and write to multiple streams atomically, defining their own consistency boundaries.

### Command Executor

The component responsible for executing commands against the event store. It handles stream reading, state reconstruction, command logic execution, and event writing.

### Consistency Boundary

The scope within which ACID properties are maintained. In EventCore, each command defines its own consistency boundary by specifying which streams it needs to read from and write to.

### CQRS (Command Query Responsibility Segregation)

An architectural pattern that separates read and write operations. EventCore naturally supports CQRS through its command system (writes) and projection system (reads).

### Dynamic Consistency Boundaries

EventCore's approach where consistency boundaries are determined at runtime by individual commands, rather than being fixed by aggregate design.

### Event

An immutable fact that represents something that happened in the system. Events are stored in streams and contain a payload, metadata, and system-generated identifiers.

### Event Sourcing

A data storage pattern where the state of entities is derived from a sequence of events, rather than storing current state directly.

### Event Store

The database or storage system that persists events in streams. EventCore provides abstractions for different event store implementations.

### Event Stream

See **Stream**.

### Multi-Stream Event Sourcing

EventCore's approach where a single command can atomically read from and write to multiple event streams, enabling complex business operations across multiple entities.

### Projection

A read model built by processing events from one or more streams. Projections transform event data into formats optimized for querying.

### Stream

A sequence of events identified by a unique StreamId. Streams represent the event history for a particular entity or concept.

## EventCore Specific Terms

### CommandStreams Trait

A trait that defines which streams a command needs to read from. Typically implemented automatically by the `#[derive(Command)]` macro.

### CommandLogic Trait

A trait containing the domain logic for command execution. Separates business logic from infrastructure concerns.

### EventId

A UUIDv7 identifier for events that provides both uniqueness and chronological ordering across the entire event store.

### EventVersion

A monotonically increasing number representing the position of an event within its stream, starting from 0.

### ExecutionResult

The result of executing a command, containing information about events written, affected streams, and execution metadata.

### ReadStreams

A type-safe container providing access to stream data during command execution. Prevents commands from accessing streams they didn't declare.

### StreamData

The collection of events from a single stream, along with metadata like the current version.

### StreamId

A validated identifier for event streams. Must be non-empty and under 255 characters.

### StreamResolver

A component that allows commands to dynamically request additional streams during execution.

### StreamWrite

A type-safe wrapper for writing events to streams that enforces stream access control.

### TypeState Pattern

A compile-time safety pattern used in EventCore's execution engine to prevent race conditions and ensure proper execution flow.

## Architecture Terms

### Functional Core, Imperative Shell

An architectural pattern where pure business logic (functional core) is separated from side effects and I/O operations (imperative shell).

### Phantom Types

Types that exist only at compile time to provide additional type safety. EventCore uses phantom types to track stream access permissions.

### Smart Constructor

A constructor function that validates input and returns a Result type, ensuring that successfully constructed values are always valid.

### Type-Driven Development

A development approach where types are designed first to make illegal states unrepresentable, followed by implementation guided by the type system.

## Event Store Terms

### Checkpoint

A saved position in an event stream indicating how far a projection has processed events.

### Expected Version

A constraint used for optimistic concurrency control when writing events to a stream.

### Optimistic Concurrency Control

A concurrency control method that assumes conflicts are rare and checks for conflicts only when committing changes.

### Position

The global ordering position of an event across all streams in the event store.

### Snapshot

A saved state of an entity at a particular point in time, used to optimize event replay performance.

### Subscription

A mechanism for receiving real-time notifications of new events as they're written to streams.

### WAL (Write-Ahead Log)

A logging mechanism where changes are written to a log before being applied to the main database.

## Patterns and Techniques

### Circuit Breaker

A pattern that prevents cascading failures by temporarily disabling operations that are likely to fail.

### Dead Letter Queue

A queue that stores messages that could not be processed successfully, allowing for later analysis and reprocessing.

### Event Envelope

A wrapper around event data that includes metadata like event type, version, and timestamps.

### Event Upcasting

The process of transforming old event formats to new formats when event schemas evolve.

### Idempotency

The property where performing an operation multiple times has the same effect as performing it once.

### Process Manager

A component that coordinates long-running business processes by reacting to events and issuing commands.

### Railway-Oriented Programming

A functional programming pattern for chaining operations that might fail, using Result types to handle errors gracefully.

### Saga

A pattern for managing complex business transactions that span multiple services or aggregates.

### Temporal Coupling

A coupling between components based on timing, which EventCore helps avoid through its event-driven architecture.

## Database and Storage Terms

### ACID Properties

**Atomicity** (all or nothing), **Consistency** (valid state), **Isolation** (concurrent safety), **Durability** (persistent storage).

### Connection Pool

A cache of database connections that can be reused across multiple requests to improve performance.

### Connection Pooling

The practice of maintaining a pool of reusable database connections.

### Index

A database structure that improves query performance by creating ordered access paths to data.

### Materialized View

A database object that contains the results of a query, physically stored and periodically refreshed.

### PostgreSQL

The primary database system supported by EventCore for production event storage.

### Transaction

A unit of work that is either completed entirely or not at all, maintaining database consistency.

### UUIDv7

A UUID variant that includes a timestamp component, providing both uniqueness and chronological ordering.

## Monitoring and Operations Terms

### Alert

A notification triggered when monitored metrics exceed predefined thresholds.

### Dashboard

A visual display of key metrics and system status information.

### Health Check

An endpoint or service that reports the operational status of a system component.

### Metrics

Quantitative measurements of system behavior and performance.

### Observability

The ability to understand the internal state of a system based on its external outputs.

### SLI (Service Level Indicator)

A metric that measures the performance of a service.

### SLO (Service Level Objective)

A target value or range for an SLI.

### Telemetry

The automated collection and transmission of data from remote sources.

### Tracing

The practice of tracking requests through distributed systems to understand performance and behavior.

## Security Terms

### Authentication

The process of verifying the identity of a user or system.

### Authorization

The process of determining what actions an authenticated user is allowed to perform.

### JWT (JSON Web Token)

A standard for securely transmitting information between parties as a JSON object.

### RBAC (Role-Based Access Control)

An access control method where permissions are associated with roles, and users are assigned roles.

### TLS (Transport Layer Security)

A cryptographic protocol for securing communications over a network.

## Development Terms

### Cargo

Rust's package manager and build system.

### CI/CD (Continuous Integration/Continuous Deployment)

Practices for automating the integration, testing, and deployment of code changes.

### Integration Test

A test that verifies the interaction between multiple components or systems.

### Mock

A test double that simulates the behavior of real objects in controlled ways.

### Property-Based Testing

A testing approach that verifies system properties hold for a wide range of generated inputs.

### Regression Test

A test that ensures previously working functionality continues to work after changes.

### Unit Test

A test that verifies the behavior of individual components in isolation.

## Error Handling Terms

### Backoff

A delay mechanism that increases wait time between retry attempts.

### Circuit Breaker

See **Patterns and Techniques** section.

### Error Boundary

A component that catches and handles errors from child components.

### Exponential Backoff

A backoff strategy where delays increase exponentially with each retry attempt.

### Failure Mode

A specific way in which a system can fail.

### Graceful Degradation

The ability of a system to continue operating with reduced functionality when components fail.

### Retry Logic

Code that automatically retries failed operations with appropriate delays and limits.

### Timeout

A limit on how long an operation is allowed to run before being considered failed.

## Configuration Terms

### Environment Variable

A value set in the operating system environment that can be read by applications.

### Configuration File

A file containing settings and parameters for application behavior.

### Secret

Sensitive configuration data like passwords or API keys that must be protected.

### TOML

A configuration file format that is easy to read and write.

### YAML

A human-readable data serialization standard often used for configuration files.

## Performance Terms

### Benchmark

A test that measures system performance under specific conditions.

### Bottleneck

The component or operation that limits overall system performance.

### Latency

The time it takes for a single operation to complete.

### Load Test

A test that simulates expected system load to verify performance characteristics.

### Profiling

The process of analyzing system performance to identify optimization opportunities.

### Scalability

The ability of a system to handle increased load by adding resources.

### Throughput

The number of operations a system can handle per unit of time.

## Data Terms

### Immutable

Data that cannot be changed after creation.

### Normalization

The process of organizing data to reduce redundancy and improve integrity.

### Payload

The actual data content of an event, excluding metadata.

### Schema

The structure and constraints that define how data is organized.

### Serialization

The process of converting data structures into a format that can be stored or transmitted.

### Validation

The process of checking that data meets specified requirements and constraints.

## Rust-Specific Terms

### Async/Await

Rust's asynchronous programming model for non-blocking operations.

### Borrow Checker

Rust's compile-time mechanism that ensures memory safety without garbage collection.

### Cargo.toml

The manifest file for Rust projects that specifies dependencies and metadata.

### Crate

A compilation unit in Rust; equivalent to a library or package in other languages.

### Derive Macro

A Rust macro that automatically generates implementations of traits for types.

### Lifetime

A construct in Rust that tracks how long references are valid.

### Option

Rust's type for representing optional values, similar to nullable types in other languages.

### Result

Rust's type for representing operations that might fail, containing either a success value or an error.

### Trait

Rust's mechanism for defining shared behavior that types can implement.

### Ownership

Rust's system for managing memory through compile-time tracking of resource ownership.

## Acronyms and Abbreviations

**API** - Application Programming Interface
**CI** - Continuous Integration
**CLI** - Command Line Interface
**CQRS** - Command Query Responsibility Segregation
**DDD** - Domain-Driven Design
**DNS** - Domain Name System
**HTTP** - Hypertext Transfer Protocol
**HTTPS** - HTTP Secure
**I/O** - Input/Output
**JSON** - JavaScript Object Notation
**JWT** - JSON Web Token
**CRUD** - Create, Read, Update, Delete
**ORM** - Object-Relational Mapping
**REST** - Representational State Transfer
**SQL** - Structured Query Language
**SSL** - Secure Sockets Layer
**TDD** - Test-Driven Development
**TLS** - Transport Layer Security
**UUID** - Universally Unique Identifier
**XML** - eXtensible Markup Language

## EventCore Command Reference

Common EventCore CLI commands and their purposes:

### `eventcore-cli`

The main command-line interface for EventCore operations.

### `health-check`

Verify system health and connectivity.

### `migrate`

Run database migrations.

### `config validate`

Validate configuration files and settings.

### `projections status`

Check the status of all projections.

### `projections rebuild`

Rebuild projections from event history.

### `streams list`

List available event streams.

### `events export`

Export events for backup or analysis.

## Common Patterns

### Builder Pattern

A creational pattern for constructing complex objects step by step.

### Factory Pattern

A creational pattern for creating objects without specifying their exact classes.

### Observer Pattern

A behavioral pattern where objects notify observers of state changes.

### Repository Pattern

A design pattern that encapsulates data access logic.

### Strategy Pattern

A behavioral pattern that enables selecting algorithms at runtime.

## Best Practices

### Fail Fast

The practice of detecting and reporting errors as early as possible.

### Immutable Infrastructure

Infrastructure that is never modified after deployment, only replaced.

### Least Privilege

The security principle of granting minimum necessary permissions.

### Separation of Concerns

The principle of dividing software into distinct sections with specific responsibilities.

### Single Responsibility Principle

Each class or module should have only one reason to change.

This glossary provides comprehensive coverage of EventCore terminology and related concepts. Use it as a reference when working with EventCore or reading the documentation.

That completes Part 7: Reference documentation for EventCore!
