# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

- *(testing)* contract-first CheckpointStore with unified backend verification ([#234](https://github.com/jwilger/eventcore/pull/234))

### Refactoring

- *(testing)* unify contract test macros into backend_contract_tests! ([#233](https://github.com/jwilger/eventcore/pull/233))

## [0.3.0](https://github.com/jwilger/eventcore/compare/eventcore-testing-v0.2.0...eventcore-testing-v0.3.0) - 2025-12-27

### Refactoring

- eliminate primitive obsession across configuration structs ([#216](https://github.com/jwilger/eventcore/pull/216))
- *(release)* switch to workspace version inheritance for full lockstep versioning ([#221](https://github.com/jwilger/eventcore/pull/221))

## [0.2.0](https://github.com/jwilger/eventcore/releases/tag/v0.2.0) - 2025-12-26

### Features

- *(postgres)* add PostgreSQL event store implementation ([#169](https://github.com/jwilger/eventcore/pull/169))
- *(projection)* implement EventReader contract tests ([#193](https://github.com/jwilger/eventcore/pull/193))
- *(testing)* create unified event_store_suite! macro ([#194](https://github.com/jwilger/eventcore/pull/194))

### Miscellaneous Tasks

- *(testing)* move chaos harness into dev crate ([#167](https://github.com/jwilger/eventcore/pull/167))
- align all workspace crate versions to 0.2.0 ([#198](https://github.com/jwilger/eventcore/pull/198))

### Refactoring

- *(store)* replace mutex unwrap with proper error handling ([#179](https://github.com/jwilger/eventcore/pull/179))
- reorganize workspace per ADR-022 for feature flag re-exports ([#188](https://github.com/jwilger/eventcore/pull/188))
- extract InMemoryEventStore into separate eventcore-memory crate ([#196](https://github.com/jwilger/eventcore/pull/196))
- *(types)* use UUID7 event IDs as global positions ([#197](https://github.com/jwilger/eventcore/pull/197))
