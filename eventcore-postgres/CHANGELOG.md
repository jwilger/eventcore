# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.9](https://github.com/jwilger/eventcore/compare/eventcore-postgres-v0.1.8...eventcore-postgres-v0.1.9) - 2025-07-24

### Other

- update Cargo.toml dependencies

## [0.1.6](https://github.com/jwilger/eventcore/compare/eventcore-postgres-v0.1.5...eventcore-postgres-v0.1.6) - 2025-07-21

### Changes

- update Cargo.toml dependencies

## [0.1.4](https://github.com/jwilger/eventcore/compare/v0.1.3...v0.1.4) - 2025-07-21

### Changes

- Add CHANGELOG.md files to all workspace packages ([#72](https://github.com/jwilger/eventcore/pull/72))
- Update pre-commit hooks to use nextest for all tests ([#70](https://github.com/jwilger/eventcore/pull/70))

### Changes
- Update pre-commit hooks to use nextest for all tests (#70)
- Fix flaky query timeout test by increasing timeout from 100ms to 500ms

## [0.1.3] - 2025-07-07

### Changes
- Initial release of PostgreSQL event store adapter

## [0.1.0] - 2025-07-04

### Changes
- Full EventStore trait implementation
- Production-ready connection pooling with configurable settings
- Comprehensive health monitoring and metrics
- Subscription support with position tracking
- Batch event insertion for improved performance
- Stream batching optimization for large reads
- Database-level gap detection for event versioning
- Prepared statement caching
- Configurable retry and timeout behavior
- Schema initialization with triggers and functions

[unreleased]: https://github.com/jwilger/eventcore/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/jwilger/eventcore/releases/tag/v0.1.3
[0.1.0]: https://github.com/jwilger/eventcore/releases/tag/v0.1.0