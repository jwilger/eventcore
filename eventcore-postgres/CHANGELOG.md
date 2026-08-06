# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v1.0.1.html).

## [Unreleased]

## [1.1.0-alpha.1](https://github.com/jwilger/eventcore/compare/eventcore-postgres-v1.0.2...eventcore-postgres-v1.1.0-alpha.1) - 2026-08-06

### Features

- *(model)* add experimental runtime-coupled checking

## [1.0.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-postgres-v1.0.0...eventcore-postgres-v1.0.1) - 2026-06-15

### Documentation

- overhaul API documentation across all crates ([#420](https://git.johnwilger.com/Slipstream/eventcore/pulls/420))
- align all documentation with the 1.0 API ([#424](https://git.johnwilger.com/Slipstream/eventcore/pulls/424))

### Testing

- harden doctests and guard docs against fabricated APIs ([#426](https://git.johnwilger.com/Slipstream/eventcore/pulls/426))

## [1.0.0](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-postgres-v0.9.0...eventcore-postgres-v1.0.0) - 2026-06-13

### Features

- _(eventcore-types)_ glob pattern matching for subscriptions ([#246](https://git.johnwilger.com/Slipstream/eventcore/pulls/246)) ([#410](https://git.johnwilger.com/Slipstream/eventcore/pulls/410))
- _(eventcore)_ [**breaking**] streaming reads for read_stream ([#364](https://git.johnwilger.com/Slipstream/eventcore/pulls/364)) ([#414](https://git.johnwilger.com/Slipstream/eventcore/pulls/414))

### Miscellaneous Tasks

- graduate workspace to 1.0.0 for first stable release ([#418](https://git.johnwilger.com/Slipstream/eventcore/pulls/418))

### Performance

- _(eventcore-postgres)_ batch INSERT in append_events ([#360](https://git.johnwilger.com/Slipstream/eventcore/pulls/360)) ([#406](https://git.johnwilger.com/Slipstream/eventcore/pulls/406))
- serialize events once in the append path ([#361](https://git.johnwilger.com/Slipstream/eventcore/pulls/361)) ([#408](https://git.johnwilger.com/Slipstream/eventcore/pulls/408))

## [0.8.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-postgres-v0.8.0...eventcore-postgres-v0.8.1) - 2026-06-13

### Miscellaneous Tasks

- migrate CI and metadata from GitHub to Forgejo ([#385](https://git.johnwilger.com/Slipstream/eventcore/pulls/385))

### Deps

- _(deps)_ bump the minor-and-patch group across 1 directory with 11 updates ([#383](https://git.johnwilger.com/Slipstream/eventcore/pulls/383))
- _(deps)_ bump nutype from 0.6.2 to 0.7.0 ([#381](https://git.johnwilger.com/Slipstream/eventcore/pulls/381))

## [0.7.1](https://github.com/jwilger/eventcore/compare/eventcore-postgres-v0.7.0...eventcore-postgres-v0.7.1) - 2026-04-15

### Bug Fixes

- filter read_events by event_type to prevent projection stalls ([#373](https://github.com/jwilger/eventcore/pull/373))

### Features

- add load-testing/stress-testing suite ([#370](https://github.com/jwilger/eventcore/pull/370))

## [0.7.0](https://github.com/jwilger/eventcore/compare/eventcore-postgres-v0.6.0...eventcore-postgres-v0.7.0) - 2026-04-13

### Bug Fixes

- improve error message consistency, context, and safety across all crates ([#352](https://github.com/jwilger/eventcore/pull/352))

### Features

- add required event_type_name() to Event trait for stable storage ([#344](https://github.com/jwilger/eventcore/pull/344))

### Miscellaneous Tasks

- adopt han plugins, blueprints, and project conventions ([#330](https://github.com/jwilger/eventcore/pull/330))
- consolidate workspace lints and enforce strict lint policy ([#351](https://github.com/jwilger/eventcore/pull/351))

## [0.6.0](https://github.com/jwilger/eventcore/compare/eventcore-postgres-v0.5.1...eventcore-postgres-v0.6.0) - 2026-03-15

### Miscellaneous Tasks

- remove claude-code-review CI workflow ([#316](https://github.com/jwilger/eventcore/pull/316))

## [0.5.0](https://github.com/jwilger/eventcore/compare/eventcore-postgres-v0.4.0...eventcore-postgres-v0.5.0) - 2025-12-31

### Features

- add ProjectorCoordinator trait and PostgreSQL advisory lock implementation ([#259](https://github.com/jwilger/eventcore/pull/259))
- implement run_projection free function (ADR-029) ([#263](https://github.com/jwilger/eventcore/pull/263))

## [0.4.0](https://github.com/jwilger/eventcore/compare/eventcore-postgres-v0.3.0...eventcore-postgres-v0.4.0) - 2025-12-29

### Features

- _(eventcore-postgres)_ add database triggers to enforce event log immutability ([#229](https://github.com/jwilger/eventcore/pull/229))
- _(testing)_ contract-first CheckpointStore with unified backend verification ([#234](https://github.com/jwilger/eventcore/pull/234))

### Refactoring

- _(eventcore-postgres)_ replace testcontainers with docker-compose ([#224](https://github.com/jwilger/eventcore/pull/224))
- _(testing)_ unify contract test macros into backend_contract_tests! ([#233](https://github.com/jwilger/eventcore/pull/233))

## [0.3.0](https://github.com/jwilger/eventcore/compare/eventcore-postgres-v0.2.0...eventcore-postgres-v0.3.0) - 2025-12-27

### Refactoring

- eliminate primitive obsession across configuration structs ([#216](https://github.com/jwilger/eventcore/pull/216))
- _(release)_ switch to workspace version inheritance for full lockstep versioning ([#221](https://github.com/jwilger/eventcore/pull/221))

## [0.2.0](https://github.com/jwilger/eventcore/releases/tag/v0.2.0) - 2025-12-26

### Features

- Implement command definition macros (Phase 13.1)
- _(postgres)_ add PostgreSQL event store implementation ([#169](https://github.com/jwilger/eventcore/pull/169))
- _(observability)_ add error logging in map_sqlx_error ([#181](https://github.com/jwilger/eventcore/pull/181))
- _(postgres)_ add idle_timeout configuration option ([#180](https://github.com/jwilger/eventcore/pull/180))
- _(postgres)_ implement EventReader trait for PostgresEventStore ([#195](https://github.com/jwilger/eventcore/pull/195))

### Miscellaneous Tasks

- release v0.1.3 ([#59](https://github.com/jwilger/eventcore/pull/59))
- release v0.1.4 ([#71](https://github.com/jwilger/eventcore/pull/71))
- release v0.1.6 ([#103](https://github.com/jwilger/eventcore/pull/103))
- align all workspace crate versions to 0.2.0 ([#198](https://github.com/jwilger/eventcore/pull/198))

### Refactoring

- _(store)_ replace mutex unwrap with proper error handling ([#179](https://github.com/jwilger/eventcore/pull/179))
- _(postgres)_ replace docker-compose with testcontainers ([#177](https://github.com/jwilger/eventcore/pull/177))
- reorganize workspace per ADR-022 for feature flag re-exports ([#188](https://github.com/jwilger/eventcore/pull/188))
- _(types)_ use UUID7 event IDs as global positions ([#197](https://github.com/jwilger/eventcore/pull/197))

### Deps

- _(deps)_ bump the minor-and-patch group with 14 updates
