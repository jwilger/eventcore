# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4](https://github.com/jwilger/eventcore/compare/v0.1.3...v0.1.4) - 2025-07-21

### Changes

- Fix emit! and require! macros to use public exports ([#98](https://github.com/jwilger/eventcore/pull/98))
- Fix missing imports in examples and tests ([#95](https://github.com/jwilger/eventcore/pull/95))
- *(executor)* extract execute_type_safe and remove dead code ([#92](https://github.com/jwilger/eventcore/pull/92))
- Add CHANGELOG.md files to all workspace packages ([#72](https://github.com/jwilger/eventcore/pull/72))
- Update pre-commit hooks to use nextest for all tests ([#70](https://github.com/jwilger/eventcore/pull/70))

### Changes
- Update pre-commit hooks to use nextest for all tests (#70)
- Add CHANGELOG.md files to all workspace packages
- Configure release-plz to include all commits in changelog
- Simplify changelog format to flat list of changes

## [0.1.3] - 2025-07-07

### Changes
- Fix release workflow and circular dependencies (#68)
- Fix release workflow to only create PRs, not publish (#67) 
- Explicitly enable release for all publishable crates (#62)
- Use PAT for release-plz to trigger CI on release PRs (#61)
- Configure release-plz to only create PRs, not publish (#60)
- Bump version to 0.1.3 to bypass existing git tags (#58)
- Replace PR validation and template with Definition of Done bot (#64)
- Reorganize CLAUDE.md for better LLM effectiveness (#47)
- Configure Dependabot to ignore internal workspace crates (#44)
- Fix crates.io publishing order to resolve circular dependency (#42)
- Extract execute_once function into focused single-responsibility methods (#10)
- Add comprehensive refactoring plan to address codebase maintainability (#9)
- Improve development workflow and pre-commit experience (#7)
- Add security infrastructure and documentation (#2)

## [0.1.0] - 2025-07-04

### Changes
- Initial release of EventCore
- Multi-stream event sourcing with dynamic consistency boundaries
- Type-safe command system with compile-time stream access guarantees
- CommandLogic trait with automatic CommandStreams derivation
- Comprehensive test infrastructure with property-based testing
- Performance benchmarks showing 86 ops/sec for single-stream commands
- Complete examples: banking transfers, e-commerce, sagas
- Comprehensive user manual and API documentation
- Integration with OpenTelemetry and Prometheus monitoring
- Production hardening with circuit breakers and resilience patterns
- CQRS projection system with checkpointing and rebuilds
- Multiple serialization formats (JSON, MessagePack, Bincode)
- Axum web framework integration example

[unreleased]: https://github.com/jwilger/eventcore/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/jwilger/eventcore/releases/tag/v0.1.3
[0.1.0]: https://github.com/jwilger/eventcore/releases/tag/v0.1.0