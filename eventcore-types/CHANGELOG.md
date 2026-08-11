# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v1.0.1.html).

## [Unreleased]

## [2.0.0](https://github.com/jwilger/eventcore/compare/eventcore-types-v1.1.1...eventcore-types-v2.0.0) - 2026-08-11

### Features

- *(snapshots)* persist command state projections

### Refactoring

- *(testing)* stabilize reconstruction benchmarks

## [1.0.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-types-v1.0.0...eventcore-types-v1.0.1) - 2026-06-15

### Documentation

- overhaul API documentation across all crates ([#420](https://git.johnwilger.com/Slipstream/eventcore/pulls/420))
- fix stale and incomplete API doc comments ([#422](https://git.johnwilger.com/Slipstream/eventcore/pulls/422))
- align all documentation with the 1.0 API ([#424](https://git.johnwilger.com/Slipstream/eventcore/pulls/424))

### Testing

- harden doctests and guard docs against fabricated APIs ([#426](https://git.johnwilger.com/Slipstream/eventcore/pulls/426))

## [1.0.0](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-types-v0.9.0...eventcore-types-v1.0.0) - 2026-06-13

### Features

- _(eventcore-types)_ glob pattern matching for subscriptions ([#246](https://git.johnwilger.com/Slipstream/eventcore/pulls/246)) ([#410](https://git.johnwilger.com/Slipstream/eventcore/pulls/410))
- _(eventcore)_ [**breaking**] streaming reads for read_stream ([#364](https://git.johnwilger.com/Slipstream/eventcore/pulls/364)) ([#414](https://git.johnwilger.com/Slipstream/eventcore/pulls/414))

### Performance

- serialize events once in the append path ([#361](https://git.johnwilger.com/Slipstream/eventcore/pulls/361)) ([#408](https://git.johnwilger.com/Slipstream/eventcore/pulls/408))

## [0.8.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-types-v0.8.0...eventcore-types-v0.8.1) - 2026-06-13

### Deps

- _(deps)_ bump the minor-and-patch group across 1 directory with 11 updates ([#383](https://git.johnwilger.com/Slipstream/eventcore/pulls/383))
- _(deps)_ bump nutype from 0.6.2 to 0.7.0 ([#381](https://git.johnwilger.com/Slipstream/eventcore/pulls/381))

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

- _(eventcore-postgres)_ add database triggers to enforce event log immutability ([#229](https://github.com/jwilger/eventcore/pull/229))
- _(testing)_ contract-first CheckpointStore with unified backend verification ([#234](https://github.com/jwilger/eventcore/pull/234))

## [0.3.0](https://github.com/jwilger/eventcore/compare/eventcore-types-v0.2.0...eventcore-types-v0.3.0) - 2025-12-27

### Refactoring

- eliminate primitive obsession across configuration structs ([#216](https://github.com/jwilger/eventcore/pull/216))
- _(release)_ switch to workspace version inheritance for full lockstep versioning ([#221](https://github.com/jwilger/eventcore/pull/221))

## [0.2.0](https://github.com/jwilger/eventcore/releases/tag/v0.2.0) - 2025-12-26

### Features

- _(projection)_ implement poll-based projection runner with error handling ([#190](https://github.com/jwilger/eventcore/pull/190))
- _(projection)_ implement EventReader contract tests ([#193](https://github.com/jwilger/eventcore/pull/193))

### Miscellaneous Tasks

- align all workspace crate versions to 0.2.0 ([#198](https://github.com/jwilger/eventcore/pull/198))

### Refactoring

- reorganize workspace per ADR-022 for feature flag re-exports ([#188](https://github.com/jwilger/eventcore/pull/188))
- _(types)_ use UUID7 event IDs as global positions ([#197](https://github.com/jwilger/eventcore/pull/197))
