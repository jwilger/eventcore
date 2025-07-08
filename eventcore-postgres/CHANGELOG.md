# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/jwilger/eventcore/releases/tag/eventcore-postgres-v0.1.2) - 2025-07-08

### Added

- Implement command definition macros (Phase 13.1)

### Other

- Fix additional release workflow failures ([#33](https://github.com/jwilger/eventcore/pull/33))
- *(deps)* bump the minor-and-patch group with 14 updates
- Update README.md
- Update README.md
- Update documentation for consistency and accuracy
- Organize documentation into structured user manual
- Complete deployment and operations documentation
- Add serialization format flexibility beyond JSON
- Add web framework integration example with Axum
- Fix CI build failures by completing simplified Command API migration
- Fix CI build failures - migrate all tests to new simplified Command API
- Complete core migration to simplified command API (363 tests passing)
- Ensure product documentation is complete, accurate, and internally consistent
- Update performance documentation with comprehensive environment details
- Complete caching strategy analysis for PostgreSQL adapter
- Fix PostgreSQL function dependency order in schema initialization
- Fix CI build failures with PostgreSQL batch event insertion
- Implement comprehensive database-level gap detection for event versioning
- Fix PostgreSQL trigger for batch inserts to prevent null event_id errors
- Complete stream batching optimization with comprehensive tests
- Document PostgreSQL prepared statement performance
- Remove unnecessary explanatory comments from documentation
- Update documentation to showcase EventCore macros
- Fix test isolation in PostgreSQL integration tests
- Fix health check schema verification after event_streams removal
- Fix executor race condition in version checking
- Revert "Fix failing concurrent creation test on beta channel"
- Fix failing concurrent creation test on beta channel
- Document and enhance PostgreSQL connection pool configuration
- Fix missing database initialization in integration tests
- Implement batch event insertion for PostgreSQL adapter
- Fix multi-stream event writing bug causing 0% success rate
- Complete Phase 15.8 Developer Experience Polish
- Complete performance validation with real PostgreSQL benchmarks
- Fix formatting and clippy issues
- Fix flaky test_multi_stream_atomicity on beta Rust
- Implement comprehensive production hardening features
- Implement timeout controls for EventStore operations
- Implement unified error types with automatic conversions
- Fix CI integration test failures and add to pre-commit
- Fix integration tests Docker Hub rate limit issue in CI
- Complete comprehensive subscription system testing and documentation cleanup
- Simplify API surface and reduce complexity per expert review
- Replace 'aggregate-per-command' with 'multi-stream event sourcing' terminology
- Add comprehensive README documentation for all crates
- Fix PostgreSQL schema initialization concurrency issue for CI
- Implement flexible command-controlled dynamic stream discovery
- Implement type-safe command system with complete concurrency control
- Make PostgreSQL adapter generic over event type E
- Complete Phase 12.1: Library Public API & Documentation
- Implement Phase 11.2: Performance optimizations for EventCore library
- Fix fundamental issue with multi-stream command state reconstruction
- Implement PostgreSQL integration tests and benchmarks (Phase 8.5)
- Implement PostgreSQL EventStore trait with multi-stream atomic operations
- Implement PostgreSQL database schema migrations for EventCore
- Implement PostgreSQL adapter crate setup and structure
- Create workspace structure with all crates
