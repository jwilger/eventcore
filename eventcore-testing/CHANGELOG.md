# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
