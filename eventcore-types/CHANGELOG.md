# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.1](https://github.com/jwilger/eventcore/compare/eventcore-types-v0.8.0...eventcore-types-v0.8.1) - 2026-04-27

### Deps

- *(deps)* bump the minor-and-patch group across 1 directory with 11 updates ([#383](https://github.com/jwilger/eventcore/pull/383))
- *(deps)* bump nutype from 0.6.2 to 0.7.0 ([#381](https://github.com/jwilger/eventcore/pull/381))

## [0.7.1](https://github.com/jwilger/eventcore/compare/eventcore-types-v0.7.0...eventcore-types-v0.7.1) - 2026-04-15

### Bug Fixes

- filter read_events by event_type to prevent projection stalls ([#373](https://github.com/jwilger/eventcore/pull/373))

## [0.7.0](https://github.com/jwilger/eventcore/compare/eventcore-types-v0.6.0...eventcore-types-v0.7.0) - 2026-04-13

### Bug Fixes

- add Send+Sync bounds to CommandLogic for Send futures ([#332](https://github.com/jwilger/eventcore/pull/332))
- improve error message consistency, context, and safety across all crates ([#352](https://github.com/jwilger/eventcore/pull/352))

### Features

- enhance require! macro to accept typed error values ([#335](https://github.com/jwilger/eventcore/pull/335))
- add required event_type_name() to Event trait for stable storage ([#344](https://github.com/jwilger/eventcore/pull/344))

### Miscellaneous Tasks

- consolidate workspace lints and enforce strict lint policy ([#351](https://github.com/jwilger/eventcore/pull/351))

### Refactoring

- replace into_inner() with into() for nutype domain types ([#334](https://github.com/jwilger/eventcore/pull/334))

## [0.6.0](https://github.com/jwilger/eventcore/compare/eventcore-types-v0.5.1...eventcore-types-v0.6.0) - 2026-03-15

### Bug Fixes

- add projection error logging and propagate checkpoint failures ([#313](https://github.com/jwilger/eventcore/pull/313))
- improve type encapsulation, error context, and clean up stale TODOs ([#315](https://github.com/jwilger/eventcore/pull/315))

## [0.5.1](https://github.com/jwilger/eventcore/compare/eventcore-types-v0.5.0...eventcore-types-v0.5.1) - 2026-02-22

### Bug Fixes

- add Sync bound to StreamResolver trait object so execute() future is Send ([#304](https://github.com/jwilger/eventcore/pull/304))

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
