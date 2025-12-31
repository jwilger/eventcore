# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/jwilger/eventcore/compare/eventcore-types-v0.4.0...eventcore-types-v0.5.0) - 2025-12-31

### Features

- add ProjectorCoordinator trait and PostgreSQL advisory lock implementation ([#259](https://github.com/jwilger/eventcore/pull/259))
- implement run_projection free function (ADR-029) ([#263](https://github.com/jwilger/eventcore/pull/263))

## [0.4.0](https://github.com/jwilger/eventcore/compare/eventcore-types-v0.3.0...eventcore-types-v0.4.0) - 2025-12-29

### Features

- *(eventcore-postgres)* add database triggers to enforce event log immutability ([#229](https://github.com/jwilger/eventcore/pull/229))
- *(testing)* contract-first CheckpointStore with unified backend verification ([#234](https://github.com/jwilger/eventcore/pull/234))

## [0.3.0](https://github.com/jwilger/eventcore/compare/eventcore-types-v0.2.0...eventcore-types-v0.3.0) - 2025-12-27

### Refactoring

- eliminate primitive obsession across configuration structs ([#216](https://github.com/jwilger/eventcore/pull/216))
- *(release)* switch to workspace version inheritance for full lockstep versioning ([#221](https://github.com/jwilger/eventcore/pull/221))

## [0.2.0](https://github.com/jwilger/eventcore/releases/tag/v0.2.0) - 2025-12-26

### Features

- *(projection)* implement poll-based projection runner with error handling ([#190](https://github.com/jwilger/eventcore/pull/190))
- *(projection)* implement EventReader contract tests ([#193](https://github.com/jwilger/eventcore/pull/193))

### Miscellaneous Tasks

- align all workspace crate versions to 0.2.0 ([#198](https://github.com/jwilger/eventcore/pull/198))

### Refactoring

- reorganize workspace per ADR-022 for feature flag re-exports ([#188](https://github.com/jwilger/eventcore/pull/188))
- *(types)* use UUID7 event IDs as global positions ([#197](https://github.com/jwilger/eventcore/pull/197))
