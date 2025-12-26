# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
