# Changelog

All notable changes to the EventCore project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Comprehensive interactive documentation tutorials
- Enhanced error diagnostics with miette integration
- Fluent CommandExecutorBuilder API for configuration
- Command definition macros for cleaner code
- Multi-stream event sourcing with dynamic consistency boundaries
- Type-safe command system with compile-time stream access control
- Flexible command-controlled dynamic stream discovery
- PostgreSQL adapter with full type safety
- In-memory event store adapter for testing
- Comprehensive benchmarking suite
- Complete examples for banking and e-commerce domains
- Property-based testing throughout the codebase
- Extensive monitoring and observability features
- Projection system with checkpointing and recovery
- Event serialization with schema evolution support
- Command retry mechanisms with configurable policies
- Developer experience improvements with macros
- Complete CI/CD pipeline with PostgreSQL integration

### Changed
- Replaced aggregate-per-command terminology with multi-stream event sourcing
- Made PostgreSQL adapter generic over event type for better type safety
- Updated Command trait to include StreamResolver for flexible stream discovery
- Enhanced concurrency control to check all read stream versions
- Improved CI configuration with PostgreSQL services and coverage optimization

### Fixed
- PostgreSQL schema initialization concurrency issues in CI
- All pre-commit hook failures across the codebase
- CI workflow syntax errors and configuration issues
- Test isolation with unique stream IDs and database cleanup
- Race conditions in concurrent command execution

### Security
- Forbid unsafe code throughout the workspace
- Comprehensive security audit integration in CI
- Protection against dependency vulnerabilities

## [0.1.0] - Initial Release

### Added
- **Core Event Sourcing Foundation**
  - `StreamId`, `EventId`, `EventVersion` types with validation
  - Command trait system with type-safe execution
  - Event store abstraction with pluggable backends
  - Multi-stream atomicity with optimistic concurrency control
  - Event metadata tracking (causation, correlation, user)

- **Type-Driven Development**
  - Extensive use of `nutype` for domain type validation
  - Smart constructors that make illegal states unrepresentable
  - Result types for all fallible operations
  - Property-based testing with `proptest`

- **PostgreSQL Adapter** (`eventcore-postgres`)
  - Full PostgreSQL event store implementation
  - Database schema migrations
  - Transaction-based multi-stream writes
  - Optimistic concurrency control with version checking
  - Connection pooling and error mapping

- **In-Memory Adapter** (`eventcore-memory`)
  - Fast in-memory event store for testing
  - Thread-safe storage with Arc<RwLock>
  - Complete EventStore trait implementation
  - Version tracking per stream

- **Command System**
  - Type-safe command execution
  - Automatic state reconstruction from events
  - Multi-stream read/write operations
  - Retry mechanisms with exponential backoff
  - Command context and metadata support

- **Projection System**
  - Projection trait for building read models
  - Checkpoint management for resume capability
  - Projection manager with lifecycle control
  - Event subscription and processing
  - Error recovery and retry logic

- **Monitoring & Observability**
  - Metrics collection (counters, gauges, timers)
  - Health checks for event store and projections
  - Structured logging with tracing integration
  - Performance monitoring and alerts

- **Serialization & Persistence**
  - JSON event serialization with schema evolution
  - Type registry for dynamic deserialization
  - Unknown event type handling
  - Migration chain support

- **Developer Experience**
  - Comprehensive test utilities and fixtures
  - Property test generators for all domain types
  - Event and command builders
  - Assertion helpers for testing
  - Test harness for end-to-end scenarios

- **Macro System** (`eventcore-macros`)
  - `#[derive(Command)]` procedural macro
  - Automatic stream field detection
  - Type-safe StreamSet generation
  - Declarative `command!` macro

- **Examples** (`eventcore-examples`)
  - Banking domain with money transfers
  - E-commerce domain with order management
  - Complete integration tests
  - Usage patterns and best practices

- **Benchmarks** (`eventcore-benchmarks`)
  - Command execution performance tests
  - Event store read/write benchmarks
  - Projection processing benchmarks
  - Memory allocation profiling

- **Documentation**
  - Comprehensive rustdoc for all public APIs
  - Interactive tutorials for common scenarios
  - Usage examples in documentation
  - Migration guides and best practices

### Performance
- Target: 5,000-10,000 single-stream commands/sec
- Target: 2,000-5,000 multi-stream commands/sec
- Target: 20,000+ events/sec (batched writes)
- Target: P95 command latency < 10ms

### Breaking Changes
- N/A (initial release)

### Migration Guide
- N/A (initial release)

### Dependencies
- **Rust**: Minimum supported version 1.70.0
- **PostgreSQL**: Version 15+ (for PostgreSQL adapter)
- **Key Dependencies**:
  - `tokio` 1.45+ for async runtime
  - `sqlx` 0.8+ for PostgreSQL integration
  - `uuid` 1.17+ with v7 support for event ordering
  - `serde` 1.0+ for serialization
  - `nutype` 0.6+ for type safety
  - `miette` 7.6+ for enhanced error diagnostics
  - `proptest` 1.7+ for property-based testing

### Architecture Highlights
- **Multi-Stream Event Sourcing**: Commands define their own consistency boundaries
- **Type-Driven Development**: Leverage Rust's type system for domain modeling
- **Functional Core, Imperative Shell**: Pure business logic with side effects at boundaries
- **Parse, Don't Validate**: Transform unstructured data at system boundaries only
- **Railway-Oriented Programming**: Chain operations using Result types

---

## Versioning Strategy

EventCore follows [Semantic Versioning](https://semver.org/) with the following guidelines:

### Major Version (X.0.0)
- Breaking changes to public APIs
- Changes to the Command trait signature
- Database schema changes requiring migration
- Changes to serialization format requiring migration

### Minor Version (0.X.0)
- New features and capabilities
- New optional methods on traits
- New crates in the workspace
- Performance improvements
- New configuration options

### Patch Version (0.0.X)
- Bug fixes
- Documentation improvements
- Dependency updates (compatible versions)
- Internal refactoring without API changes

### Workspace Versioning
All crates in the EventCore workspace share the same version number to ensure compatibility:
- `eventcore` (core library)
- `eventcore-postgres` (PostgreSQL adapter)
- `eventcore-memory` (in-memory adapter)
- `eventcore-examples` (example implementations)
- `eventcore-benchmarks` (performance benchmarks)
- `eventcore-macros` (procedural macros)

### Pre-release Versions
- Alpha: `X.Y.Z-alpha.N` - Early development, APIs may change
- Beta: `X.Y.Z-beta.N` - Feature complete, testing phase
- RC: `X.Y.Z-rc.N` - Release candidate, final testing

### Compatibility Promise
- **Patch versions**: Fully compatible, safe to upgrade
- **Minor versions**: Backward compatible, safe to upgrade
- **Major versions**: May contain breaking changes, migration guide provided

---

## Contributing

When contributing to EventCore:

1. Follow the [conventional commits](https://www.conventionalcommits.org/) format
2. Update this CHANGELOG.md with your changes
3. Ensure all tests pass and coverage remains high
4. Update documentation for any API changes
5. Add property-based tests for new functionality

### Commit Message Format
```
type(scope): description

[optional body]

[optional footer]
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`
Scopes: `core`, `postgres`, `memory`, `examples`, `macros`, `benchmarks`