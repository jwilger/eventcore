# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v1.0.1.html).

## [Unreleased]

## [1.0.2](https://github.com/jwilger/eventcore/compare/eventcore-testing-v1.0.1...eventcore-testing-v1.0.2) - 2026-08-03

### Miscellaneous Tasks

- release v1.0.2

## [1.0.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-testing-v1.0.0...eventcore-testing-v1.0.1) - 2026-06-15

### Documentation

- overhaul API documentation across all crates ([#420](https://git.johnwilger.com/Slipstream/eventcore/pulls/420))
- align all documentation with the 1.0 API ([#424](https://git.johnwilger.com/Slipstream/eventcore/pulls/424))

### Testing

- harden doctests and guard docs against fabricated APIs ([#426](https://git.johnwilger.com/Slipstream/eventcore/pulls/426))

## [1.0.0](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-testing-v0.9.0...eventcore-testing-v1.0.0) - 2026-06-13

### Features

- _(eventcore-types)_ glob pattern matching for subscriptions ([#246](https://git.johnwilger.com/Slipstream/eventcore/pulls/246)) ([#410](https://git.johnwilger.com/Slipstream/eventcore/pulls/410))
- _(eventcore)_ [**breaking**] streaming reads for read_stream ([#364](https://git.johnwilger.com/Slipstream/eventcore/pulls/364)) ([#414](https://git.johnwilger.com/Slipstream/eventcore/pulls/414))

### Miscellaneous Tasks

- graduate workspace to 1.0.0 for first stable release ([#418](https://git.johnwilger.com/Slipstream/eventcore/pulls/418))

## [0.8.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-testing-v0.8.0...eventcore-testing-v0.8.1) - 2026-06-13

### Deps

- _(deps)_ bump the minor-and-patch group across 1 directory with 11 updates ([#383](https://git.johnwilger.com/Slipstream/eventcore/pulls/383))
- _(deps)_ bump nutype from 0.6.2 to 0.7.0 ([#381](https://git.johnwilger.com/Slipstream/eventcore/pulls/381))

## [0.7.1](https://github.com/jwilger/eventcore/compare/eventcore-testing-v0.7.0...eventcore-testing-v0.7.1) - 2026-04-15

### Features

- add load-testing/stress-testing suite ([#370](https://github.com/jwilger/eventcore/pull/370))

## [0.7.0](https://github.com/jwilger/eventcore/compare/eventcore-testing-v0.6.0...eventcore-testing-v0.7.0) - 2026-04-13

### Bug Fixes

- make InMemoryEventStore error on read_stream type mismatch ([#342](https://github.com/jwilger/eventcore/pull/342))
- improve error message consistency, context, and safety across all crates ([#352](https://github.com/jwilger/eventcore/pull/352))

### Features

- add required event_type_name() to Event trait for stable storage ([#344](https://github.com/jwilger/eventcore/pull/344))
- add TestScenario GWT testing helpers to eventcore-testing ([#346](https://github.com/jwilger/eventcore/pull/346))

### Miscellaneous Tasks

- consolidate workspace lints and enforce strict lint policy ([#351](https://github.com/jwilger/eventcore/pull/351))

### Refactoring

- expose projection config via free function API, then reduce public surface ([#357](https://github.com/jwilger/eventcore/pull/357))

## [0.6.0](https://github.com/jwilger/eventcore/compare/eventcore-testing-v0.5.1...eventcore-testing-v0.6.0) - 2026-03-15

### Bug Fixes

- add projection error logging and propagate checkpoint failures ([#313](https://github.com/jwilger/eventcore/pull/313))

## [0.5.1](https://github.com/jwilger/eventcore/compare/eventcore-testing-v0.5.0...eventcore-testing-v0.5.1) - 2026-02-22

### Features

- migrate integration tests to eventcore-examples and add deterministic store ([#295](https://github.com/jwilger/eventcore/pull/295))

## [0.5.0](https://github.com/jwilger/eventcore/compare/eventcore-testing-v0.4.0...eventcore-testing-v0.5.0) - 2025-12-31

### Features

- add ProjectorCoordinator trait and PostgreSQL advisory lock implementation ([#259](https://github.com/jwilger/eventcore/pull/259))

## [0.4.0](https://github.com/jwilger/eventcore/compare/eventcore-testing-v0.3.0...eventcore-testing-v0.4.0) - 2025-12-29

### Features

- _(testing)_ contract-first CheckpointStore with unified backend verification ([#234](https://github.com/jwilger/eventcore/pull/234))

### Refactoring

- _(testing)_ unify contract test macros into backend_contract_tests! ([#233](https://github.com/jwilger/eventcore/pull/233))

## [0.3.0](https://github.com/jwilger/eventcore/compare/eventcore-testing-v0.2.0...eventcore-testing-v0.3.0) - 2025-12-27

### Refactoring

- eliminate primitive obsession across configuration structs ([#216](https://github.com/jwilger/eventcore/pull/216))
- _(release)_ switch to workspace version inheritance for full lockstep versioning ([#221](https://github.com/jwilger/eventcore/pull/221))

## [0.2.0](https://github.com/jwilger/eventcore/releases/tag/v0.2.0) - 2025-12-26

### Features

- _(postgres)_ add PostgreSQL event store implementation ([#169](https://github.com/jwilger/eventcore/pull/169))
- _(projection)_ implement EventReader contract tests ([#193](https://github.com/jwilger/eventcore/pull/193))
- _(testing)_ create unified event_store_suite! macro ([#194](https://github.com/jwilger/eventcore/pull/194))

### Miscellaneous Tasks

- _(testing)_ move chaos harness into dev crate ([#167](https://github.com/jwilger/eventcore/pull/167))
- align all workspace crate versions to 0.2.0 ([#198](https://github.com/jwilger/eventcore/pull/198))

### Refactoring

- _(store)_ replace mutex unwrap with proper error handling ([#179](https://github.com/jwilger/eventcore/pull/179))
- reorganize workspace per ADR-022 for feature flag re-exports ([#188](https://github.com/jwilger/eventcore/pull/188))
- extract InMemoryEventStore into separate eventcore-memory crate ([#196](https://github.com/jwilger/eventcore/pull/196))
- _(types)_ use UUID7 event IDs as global positions ([#197](https://github.com/jwilger/eventcore/pull/197))
