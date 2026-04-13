# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0](https://github.com/jwilger/eventcore/compare/eventcore-macros-v0.6.0...eventcore-macros-v0.7.0) - 2026-04-13

### Bug Fixes

- improve error message consistency, context, and safety across all crates ([#352](https://github.com/jwilger/eventcore/pull/352))

### Features

- enhance require! macro to accept typed error values ([#335](https://github.com/jwilger/eventcore/pull/335))
- add required event_type_name() to Event trait for stable storage ([#344](https://github.com/jwilger/eventcore/pull/344))

### Miscellaneous Tasks

- adopt han plugins, blueprints, and project conventions ([#330](https://github.com/jwilger/eventcore/pull/330))
- consolidate workspace lints and enforce strict lint policy ([#351](https://github.com/jwilger/eventcore/pull/351))

### Refactoring

- expose projection config via free function API, then reduce public surface ([#357](https://github.com/jwilger/eventcore/pull/357))

## [0.3.0](https://github.com/jwilger/eventcore/compare/eventcore-macros-v0.2.0...eventcore-macros-v0.3.0) - 2025-12-27

### Refactoring

- *(release)* switch to workspace version inheritance for full lockstep versioning ([#221](https://github.com/jwilger/eventcore/pull/221))

## [0.2.1](https://github.com/jwilger/eventcore/compare/v0.2.0...v0.2.1) - 2025-12-26

### Bug Fixes

- add version specs to workspace path dependencies ([#209](https://github.com/jwilger/eventcore/pull/209))

## [0.2.0](https://github.com/jwilger/eventcore/releases/tag/v0.2.0) - 2025-12-26

### Features

- Implement command definition macros (Phase 13.1)
- *(eventcore-006)* deliver derive macro and acceptance evidence ([#158](https://github.com/jwilger/eventcore/pull/158))
- *(macros)* add require! guard macro ([#163](https://github.com/jwilger/eventcore/pull/163))
- *(postgres)* add PostgreSQL event store implementation ([#169](https://github.com/jwilger/eventcore/pull/169))
- re-export Command macro via feature flag ([#178](https://github.com/jwilger/eventcore/pull/178))

### Miscellaneous Tasks

- release v0.1.3 ([#59](https://github.com/jwilger/eventcore/pull/59))
- release v0.1.4 ([#71](https://github.com/jwilger/eventcore/pull/71))
- align all workspace crate versions to 0.2.0 ([#198](https://github.com/jwilger/eventcore/pull/198))

### Refactoring

- reorganize workspace per ADR-022 for feature flag re-exports ([#188](https://github.com/jwilger/eventcore/pull/188))
- extract InMemoryEventStore into separate eventcore-memory crate ([#196](https://github.com/jwilger/eventcore/pull/196))
- *(types)* use UUID7 event IDs as global positions ([#197](https://github.com/jwilger/eventcore/pull/197))
