# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/jwilger/eventcore/releases/tag/v0.2.0) - 2025-12-26

### Bug Fixes

- add clippy allow attribute to emit! macro to suppress vec_init_then_push warning ([#106](https://github.com/jwilger/eventcore/pull/106))
- *(projection)* add retry with exponential backoff for database poll errors ([#191](https://github.com/jwilger/eventcore/pull/191))

### Features

- *(eventcore)* implement subscription system with position tracking
- Implement command definition macros (Phase 13.1)
- Implement fluent CommandExecutorBuilder API
- Implement enhanced error diagnostics with miette
- Add comprehensive interactive documentation tutorials (Phase 13.4)
- Add comprehensive final testing suite (Phase 14.2)
- *(projection)* implement poll-based projection runner with error handling ([#190](https://github.com/jwilger/eventcore/pull/190))
- *(projection)* implement EventReader contract tests ([#193](https://github.com/jwilger/eventcore/pull/193))
- *(testing)* create unified event_store_suite! macro ([#194](https://github.com/jwilger/eventcore/pull/194))

### Miscellaneous Tasks

- release v0.1.4 ([#71](https://github.com/jwilger/eventcore/pull/71))
- release v0.1.6 ([#103](https://github.com/jwilger/eventcore/pull/103))
- release v0.1.8 ([#108](https://github.com/jwilger/eventcore/pull/108))
- align all workspace crate versions to 0.2.0 ([#198](https://github.com/jwilger/eventcore/pull/198))

### Refactoring

- *(executor)* extract execute_type_safe and remove dead code ([#92](https://github.com/jwilger/eventcore/pull/92))
- reorganize workspace per ADR-022 for feature flag re-exports ([#188](https://github.com/jwilger/eventcore/pull/188))
- extract InMemoryEventStore into separate eventcore-memory crate ([#196](https://github.com/jwilger/eventcore/pull/196))
- *(types)* use UUID7 event IDs as global positions ([#197](https://github.com/jwilger/eventcore/pull/197))

### Deps

- *(deps)* bump the minor-and-patch group with 14 updates
- *(deps)* bump bincode from 1.3.3 to 2.0.1 ([#6](https://github.com/jwilger/eventcore/pull/6))
- *(deps)* bump the minor-and-patch group with 17 updates ([#192](https://github.com/jwilger/eventcore/pull/192))
