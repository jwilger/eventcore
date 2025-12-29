# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/jwilger/eventcore/compare/eventcore-postgres-v0.3.0...eventcore-postgres-v0.4.0) - 2025-12-29

### Features

- *(eventcore-postgres)* add database triggers to enforce event log immutability ([#229](https://github.com/jwilger/eventcore/pull/229))
- *(testing)* contract-first CheckpointStore with unified backend verification ([#234](https://github.com/jwilger/eventcore/pull/234))

### Refactoring

- *(eventcore-postgres)* replace testcontainers with docker-compose ([#224](https://github.com/jwilger/eventcore/pull/224))
- *(testing)* unify contract test macros into backend_contract_tests! ([#233](https://github.com/jwilger/eventcore/pull/233))

## [0.3.0](https://github.com/jwilger/eventcore/compare/eventcore-postgres-v0.2.0...eventcore-postgres-v0.3.0) - 2025-12-27

### Refactoring

- eliminate primitive obsession across configuration structs ([#216](https://github.com/jwilger/eventcore/pull/216))
- *(release)* switch to workspace version inheritance for full lockstep versioning ([#221](https://github.com/jwilger/eventcore/pull/221))

## [0.2.0](https://github.com/jwilger/eventcore/releases/tag/v0.2.0) - 2025-12-26

### Features

- Implement command definition macros (Phase 13.1)
- *(postgres)* add PostgreSQL event store implementation ([#169](https://github.com/jwilger/eventcore/pull/169))
- *(observability)* add error logging in map_sqlx_error ([#181](https://github.com/jwilger/eventcore/pull/181))
- *(postgres)* add idle_timeout configuration option ([#180](https://github.com/jwilger/eventcore/pull/180))
- *(postgres)* implement EventReader trait for PostgresEventStore ([#195](https://github.com/jwilger/eventcore/pull/195))

### Miscellaneous Tasks

- release v0.1.3 ([#59](https://github.com/jwilger/eventcore/pull/59))
- release v0.1.4 ([#71](https://github.com/jwilger/eventcore/pull/71))
- release v0.1.6 ([#103](https://github.com/jwilger/eventcore/pull/103))
- align all workspace crate versions to 0.2.0 ([#198](https://github.com/jwilger/eventcore/pull/198))

### Refactoring

- *(store)* replace mutex unwrap with proper error handling ([#179](https://github.com/jwilger/eventcore/pull/179))
- *(postgres)* replace docker-compose with testcontainers ([#177](https://github.com/jwilger/eventcore/pull/177))
- reorganize workspace per ADR-022 for feature flag re-exports ([#188](https://github.com/jwilger/eventcore/pull/188))
- *(types)* use UUID7 event IDs as global positions ([#197](https://github.com/jwilger/eventcore/pull/197))

### Deps

- *(deps)* bump the minor-and-patch group with 14 updates
