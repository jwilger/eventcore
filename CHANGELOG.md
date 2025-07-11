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
- Complete examples for banking, e-commerce, and saga pattern domains
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

## [0.1.3] - 2025-07-07

### Fixed
- PostgreSQL test configuration (missing TEST_DATABASE_URL) in PR #31
- Documentation sync script symlink issue in PR #31
- Cargo.toml version specifications for crates.io publishing in PR #33
- CSS directory creation for documentation builds in PR #33
- Workspace dependency syntax errors (`rand.workspace = true` to `rand = { workspace = true }`)
- Version conflicts preventing re-release after partial crates.io publishing
- Circular dependency in eventcore-macros preventing crates.io release
- Publishing order to resolve dev-dependency circular dependencies

### Changed
- Implemented workspace dependencies for internal crates to enable automatic lockstep versioning
- Updated publishing order to macros → memory → postgres → eventcore
- Added PR template compliance rules to CLAUDE.md
- Improved PR validation workflow with debouncing and comment deduplication
- Replaced PR validation workflow with Definition of Done bot
- Removed version numbers from internal workspace dependencies for cleaner dependency management

### Added
- PR template compliance enforcement in development workflow
- Definition of Done bot configuration for automatic PR checklists
- Critical rule #4 to CLAUDE.md: Always stop and ask for help rather than taking shortcuts

## [0.1.2] - 2025-07-05

### Fixed
- Rand crate v0.9.1 deprecation errors:
  - Updated `thread_rng()` to `rng()` across codebase
  - Updated `gen()` to `random()` and `gen_range()` to `random_range()`
  - Fixed ThreadRng Send issue in stress tests
- OpenTelemetry v0.30.0 API breaking changes:
  - Updated `Resource::new()` to `Resource::builder()` pattern
  - Removed unnecessary runtime parameter from `PeriodicReader::builder()`
  - Added required `grpc-tonic` feature to opentelemetry-otlp dependency
- Bincode v2.0.1 API breaking changes:
  - Updated to use `bincode::serde::encode_to_vec()` and `bincode::serde::decode_from_slice()`
  - Added "serde" feature to bincode dependency
  - Replaced deprecated `bincode::serialize()` and `bincode::deserialize()` functions

### Changed
- Updated actions/configure-pages from v4 to v5 (PR #3)
- Updated codecov/codecov-action from v3 to v5 (PR #4)

## [0.1.1] - 2025-07-05

### Added
- Modern documentation website with mdBook
  - GitHub Pages deployment workflow
  - Custom EventCore branding and responsive design
  - Automated documentation synchronization from markdown sources
  - Deployment on releases with version information
- Comprehensive security infrastructure:
  - SECURITY.md with vulnerability reporting via GitHub Security Advisories
  - Improved cargo-audit CI job using rustsec/audit-check action
  - Dependabot configuration for automated dependency updates
  - CONTRIBUTING.md with GPG signing documentation
  - Security guide in user manual covering authentication, encryption, validation, and compliance
  - COMPLIANCE_CHECKLIST.md mapping to OWASP/NIST/SOC2/PCI/GDPR/HIPAA
  - Pull request template with security and performance review checklists
- GitHub Copilot instructions for automated PR reviews
- Pre-commit hook improvements:
  - Added doctests to pre-commit hooks
  - Auto-format and stage files instead of failing
- GitHub MCP server integration for all GitHub operations

### Fixed
- Outdated Command trait references (now CommandLogic) in documentation
- Broken documentation links in README.md
- License information to reflect MIT-only licensing
- Doctest compilation error in resource.rs

### Changed
- Reorganized documentation structure (renumbered operations to 07, reference to 08)
- Consolidated documentation to single source (symlinked docs/manual to website/src/manual)
- Updated PR template to remove redundant pre-merge checklist and add Review Focus section
- Enhanced CLAUDE.md with GitHub MCP integration and PR-based workflow documentation
- Simplified PR template by consolidating multiple checklists into single Submitter Checklist

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
  - Order fulfillment saga with distributed transaction coordination
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