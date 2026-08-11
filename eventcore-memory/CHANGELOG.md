# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v1.0.1.html).

## [Unreleased]

## [1.1.2](https://github.com/jwilger/eventcore/compare/eventcore-memory-v1.1.1...eventcore-memory-v1.1.2) - 2026-08-11

### Refactoring

- *(testing)* stabilize reconstruction benchmarks

## [1.0.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-memory-v1.0.0...eventcore-memory-v1.0.1) - 2026-06-15

### Documentation

- align all documentation with the 1.0 API ([#424](https://git.johnwilger.com/Slipstream/eventcore/pulls/424))

### Testing

- harden doctests and guard docs against fabricated APIs ([#426](https://git.johnwilger.com/Slipstream/eventcore/pulls/426))

## [1.0.0](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-memory-v0.9.0...eventcore-memory-v1.0.0) - 2026-06-13

### Features

- _(eventcore-types)_ glob pattern matching for subscriptions ([#246](https://git.johnwilger.com/Slipstream/eventcore/pulls/246)) ([#410](https://git.johnwilger.com/Slipstream/eventcore/pulls/410))
- _(eventcore)_ [**breaking**] streaming reads for read_stream ([#364](https://git.johnwilger.com/Slipstream/eventcore/pulls/364)) ([#414](https://git.johnwilger.com/Slipstream/eventcore/pulls/414))

### Miscellaneous Tasks

- graduate workspace to 1.0.0 for first stable release ([#418](https://git.johnwilger.com/Slipstream/eventcore/pulls/418))

### Performance

- serialize events once in the append path ([#361](https://git.johnwilger.com/Slipstream/eventcore/pulls/361)) ([#408](https://git.johnwilger.com/Slipstream/eventcore/pulls/408))

## [0.8.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-memory-v0.8.0...eventcore-memory-v0.8.1) - 2026-06-13

### Deps

- _(deps)_ bump the minor-and-patch group across 1 directory with 11 updates ([#383](https://git.johnwilger.com/Slipstream/eventcore/pulls/383))

## [0.7.1](https://github.com/jwilger/eventcore/compare/eventcore-memory-v0.7.0...eventcore-memory-v0.7.1) - 2026-04-15

### Bug Fixes

- filter read_events by event_type to prevent projection stalls ([#373](https://github.com/jwilger/eventcore/pull/373))

## [0.7.0](https://github.com/jwilger/eventcore/compare/eventcore-memory-v0.6.0...eventcore-memory-v0.7.0) - 2026-04-13

### Bug Fixes

- make InMemoryEventStore error on read_stream type mismatch ([#342](https://github.com/jwilger/eventcore/pull/342))
- improve error message consistency, context, and safety across all crates ([#352](https://github.com/jwilger/eventcore/pull/352))

### Features

- add required event_type_name() to Event trait for stable storage ([#344](https://github.com/jwilger/eventcore/pull/344))

### Miscellaneous Tasks

- consolidate workspace lints and enforce strict lint policy ([#351](https://github.com/jwilger/eventcore/pull/351))

## [0.6.0](https://github.com/jwilger/eventcore/compare/eventcore-memory-v0.5.1...eventcore-memory-v0.6.0) - 2026-03-15

### Bug Fixes

- improve type encapsulation, error context, and clean up stale TODOs ([#315](https://github.com/jwilger/eventcore/pull/315))

### Miscellaneous Tasks

- remove claude-code-review CI workflow ([#316](https://github.com/jwilger/eventcore/pull/316))

## [0.5.0](https://github.com/jwilger/eventcore/compare/eventcore-memory-v0.4.0...eventcore-memory-v0.5.0) - 2025-12-31

### Features

- add ProjectorCoordinator trait and PostgreSQL advisory lock implementation ([#259](https://github.com/jwilger/eventcore/pull/259))
- implement run_projection free function (ADR-029) ([#263](https://github.com/jwilger/eventcore/pull/263))

## [0.4.0](https://github.com/jwilger/eventcore/compare/eventcore-memory-v0.3.0...eventcore-memory-v0.4.0) - 2025-12-29

### Features

- _(testing)_ contract-first CheckpointStore with unified backend verification ([#234](https://github.com/jwilger/eventcore/pull/234))

## [0.3.0](https://github.com/jwilger/eventcore/compare/eventcore-memory-v0.2.0...eventcore-memory-v0.3.0) - 2025-12-27

### Refactoring

- _(release)_ switch to workspace version inheritance for full lockstep versioning ([#221](https://github.com/jwilger/eventcore/pull/221))

## [0.2.0](https://github.com/jwilger/eventcore/releases/tag/v0.2.0) - 2025-12-26

### Miscellaneous Tasks

- release v0.1.3 ([#59](https://github.com/jwilger/eventcore/pull/59))
- release v0.1.3 ([#66](https://github.com/jwilger/eventcore/pull/66))
- release v0.1.4 ([#71](https://github.com/jwilger/eventcore/pull/71))
- release v0.1.6 ([#103](https://github.com/jwilger/eventcore/pull/103))

### Refactoring

- extract InMemoryEventStore into separate eventcore-memory crate ([#196](https://github.com/jwilger/eventcore/pull/196))
- _(types)_ use UUID7 event IDs as global positions ([#197](https://github.com/jwilger/eventcore/pull/197))
