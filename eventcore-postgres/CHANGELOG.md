# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.3] - 2025-07-09

### Added
- Initial release of PostgreSQL event store adapter
- Full EventStore trait implementation
- Production-ready with connection pooling
- Configurable retry and timeout behavior
- Health monitoring and metrics
- Subscription support with position tracking

### Fixed
- Fixed flaky connection pool timeout test

[unreleased]: https://github.com/jwilger/eventcore/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/jwilger/eventcore/releases/tag/v0.1.3