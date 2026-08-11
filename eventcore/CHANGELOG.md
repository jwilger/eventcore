# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v1.0.1.html).

## [Unreleased]

## [1.1.2](https://github.com/jwilger/eventcore/compare/eventcore-v1.1.1...eventcore-v1.1.2) - 2026-08-11

### Refactoring

- *(testing)* stabilize reconstruction benchmarks

## [1.1.1](https://github.com/jwilger/eventcore/compare/eventcore-v1.1.0...eventcore-v1.1.1) - 2026-08-06

### Bug Fixes

- *(model)* complete experimental checker acceptance
- *(model)* preserve checker API compatibility

## [1.1.0](https://github.com/jwilger/eventcore/compare/eventcore-v1.1.0-alpha.1...eventcore-v1.1.0) - 2026-08-06

### Documentation

- *(api)* clarify experimental feature stability

## [1.0.2](https://github.com/jwilger/eventcore/compare/eventcore-v1.0.1...eventcore-v1.0.2) - 2026-08-02

### Miscellaneous Tasks

- update repository references after transfer ([#4](https://github.com/jwilger/eventcore/pull/4))

## [1.0.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-v1.0.0...eventcore-v1.0.1) - 2026-06-15

### Documentation

- overhaul API documentation across all crates ([#420](https://git.johnwilger.com/Slipstream/eventcore/pulls/420))
- fix stale and incomplete API doc comments ([#422](https://git.johnwilger.com/Slipstream/eventcore/pulls/422))
- align all documentation with the 1.0 API ([#424](https://git.johnwilger.com/Slipstream/eventcore/pulls/424))

### Testing

- harden doctests and guard docs against fabricated APIs ([#426](https://git.johnwilger.com/Slipstream/eventcore/pulls/426))

## [1.0.0](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-v0.9.0...eventcore-v1.0.0) - 2026-06-13

### Documentation

- documentation completeness audit ([#244](https://git.johnwilger.com/Slipstream/eventcore/pulls/244)) ([#416](https://git.johnwilger.com/Slipstream/eventcore/pulls/416))

### Features

- _(eventcore)_ [**breaking**] streaming reads for read_stream ([#364](https://git.johnwilger.com/Slipstream/eventcore/pulls/364)) ([#414](https://git.johnwilger.com/Slipstream/eventcore/pulls/414))

### Miscellaneous Tasks

- graduate workspace to 1.0.0 for first stable release ([#418](https://git.johnwilger.com/Slipstream/eventcore/pulls/418))

## [0.8.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-v0.8.0...eventcore-v0.8.1) - 2026-06-13

### Deps

- _(deps)_ bump the minor-and-patch group across 1 directory with 11 updates ([#383](https://git.johnwilger.com/Slipstream/eventcore/pulls/383))
- _(deps)_ bump nutype from 0.6.2 to 0.7.0 ([#381](https://git.johnwilger.com/Slipstream/eventcore/pulls/381))

## [0.7.1](https://github.com/jwilger/eventcore/compare/eventcore-v0.7.0...eventcore-v0.7.1) - 2026-04-15

### Bug Fixes

- filter read_events by event_type to prevent projection stalls ([#373](https://github.com/jwilger/eventcore/pull/373))

### Features

- add load-testing/stress-testing suite ([#370](https://github.com/jwilger/eventcore/pull/370))

## [0.7.0](https://github.com/jwilger/eventcore/compare/eventcore-v0.6.0...eventcore-v0.7.0) - 2026-04-13

### Bug Fixes

- improve error message consistency, context, and safety across all crates ([#352](https://github.com/jwilger/eventcore/pull/352))

### Features

- enhance require! macro to accept typed error values ([#335](https://github.com/jwilger/eventcore/pull/335))
- add required event_type_name() to Event trait for stable storage ([#344](https://github.com/jwilger/eventcore/pull/344))

### Miscellaneous Tasks

- adopt han plugins, blueprints, and project conventions ([#330](https://github.com/jwilger/eventcore/pull/330))
- consolidate workspace lints and enforce strict lint policy ([#351](https://github.com/jwilger/eventcore/pull/351))

### Refactoring

- replace into_inner() with into() for nutype domain types ([#334](https://github.com/jwilger/eventcore/pull/334))
- extract pure state machines from execute() and run() ([#349](https://github.com/jwilger/eventcore/pull/349))
- expose projection config via free function API, then reduce public surface ([#357](https://github.com/jwilger/eventcore/pull/357))

## [0.6.0](https://github.com/jwilger/eventcore/compare/eventcore-v0.5.1...eventcore-v0.6.0) - 2026-03-15

### Bug Fixes

- add projection error logging and propagate checkpoint failures ([#313](https://github.com/jwilger/eventcore/pull/313))
- improve type encapsulation, error context, and clean up stale TODOs ([#315](https://github.com/jwilger/eventcore/pull/315))

### Features

- add eventcore-sqlite crate with SQLCipher encryption support ([#310](https://github.com/jwilger/eventcore/pull/310))

## [0.5.1](https://github.com/jwilger/eventcore/compare/eventcore-v0.5.0...eventcore-v0.5.1) - 2026-02-22

### Bug Fixes

- add Sync bound to StreamResolver trait object so execute() future is Send ([#304](https://github.com/jwilger/eventcore/pull/304))

## [0.5.0](https://github.com/jwilger/eventcore/compare/eventcore-v0.4.0...eventcore-v0.5.0) - 2025-12-31

### Documentation

- align issues and ADRs with ARCHITECTURE.md guidance ([#256](https://github.com/jwilger/eventcore/pull/256))

### Features

- implement run_projection free function (ADR-029) ([#263](https://github.com/jwilger/eventcore/pull/263))

## [0.4.0](https://github.com/jwilger/eventcore/compare/eventcore-v0.3.0...eventcore-v0.4.0) - 2025-12-29

### Features

- _(testing)_ contract-first CheckpointStore with unified backend verification ([#234](https://github.com/jwilger/eventcore/pull/234))

### Refactoring

- remove vestigial LocalCoordinator and CoordinatorGuard ([#255](https://github.com/jwilger/eventcore/pull/255))

## [0.3.0](https://github.com/jwilger/eventcore/compare/eventcore-v0.2.0...eventcore-v0.3.0) - 2025-12-27

### Features

- add configurable poll behavior for projections ([#213](https://github.com/jwilger/eventcore/pull/213))
- _(eventcore)_ implement EventRetryConfig for event processing failures ([#215](https://github.com/jwilger/eventcore/pull/215))

### Refactoring

- eliminate primitive obsession across configuration structs ([#216](https://github.com/jwilger/eventcore/pull/216))
- _(release)_ switch to workspace version inheritance for full lockstep versioning ([#221](https://github.com/jwilger/eventcore/pull/221))

## [0.2.0](https://github.com/jwilger/eventcore/releases/tag/v0.2.0) - 2025-12-26

### Bug Fixes

- add clippy allow attribute to emit! macro to suppress vec_init_then_push warning ([#106](https://github.com/jwilger/eventcore/pull/106))
- _(projection)_ add retry with exponential backoff for database poll errors ([#191](https://github.com/jwilger/eventcore/pull/191))

### Features

- _(eventcore)_ implement subscription system with position tracking
- Implement command definition macros (Phase 13.1)
- Implement fluent CommandExecutorBuilder API
- Implement enhanced error diagnostics with miette
- Add comprehensive interactive documentation tutorials (Phase 13.4)
- Add comprehensive final testing suite (Phase 14.2)
- _(projection)_ implement poll-based projection runner with error handling ([#190](https://github.com/jwilger/eventcore/pull/190))
- _(projection)_ implement EventReader contract tests ([#193](https://github.com/jwilger/eventcore/pull/193))
- _(testing)_ create unified event_store_suite! macro ([#194](https://github.com/jwilger/eventcore/pull/194))

### Miscellaneous Tasks

- release v0.1.4 ([#71](https://github.com/jwilger/eventcore/pull/71))
- release v0.1.6 ([#103](https://github.com/jwilger/eventcore/pull/103))
- release v0.1.8 ([#108](https://github.com/jwilger/eventcore/pull/108))
- align all workspace crate versions to 0.2.0 ([#198](https://github.com/jwilger/eventcore/pull/198))

### Refactoring

- _(executor)_ extract execute_type_safe and remove dead code ([#92](https://github.com/jwilger/eventcore/pull/92))
- reorganize workspace per ADR-022 for feature flag re-exports ([#188](https://github.com/jwilger/eventcore/pull/188))
- extract InMemoryEventStore into separate eventcore-memory crate ([#196](https://github.com/jwilger/eventcore/pull/196))
- _(types)_ use UUID7 event IDs as global positions ([#197](https://github.com/jwilger/eventcore/pull/197))

### Deps

- _(deps)_ bump the minor-and-patch group with 14 updates
- _(deps)_ bump bincode from 1.3.3 to 2.0.1 ([#6](https://github.com/jwilger/eventcore/pull/6))
- _(deps)_ bump the minor-and-patch group with 17 updates ([#192](https://github.com/jwilger/eventcore/pull/192))
