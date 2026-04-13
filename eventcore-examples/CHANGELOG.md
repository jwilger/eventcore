# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0](https://github.com/jwilger/eventcore/compare/eventcore-examples-v0.6.0...eventcore-examples-v0.7.0) - 2026-04-13

### Bug Fixes

- improve error message consistency, context, and safety across all crates ([#352](https://github.com/jwilger/eventcore/pull/352))

### Features

- add required event_type_name() to Event trait for stable storage ([#344](https://github.com/jwilger/eventcore/pull/344))
- add TestScenario GWT testing helpers to eventcore-testing ([#346](https://github.com/jwilger/eventcore/pull/346))

### Miscellaneous Tasks

- consolidate workspace lints and enforce strict lint policy ([#351](https://github.com/jwilger/eventcore/pull/351))

### Refactoring

- replace into_inner() with into() for nutype domain types ([#334](https://github.com/jwilger/eventcore/pull/334))
- expose projection config via free function API, then reduce public surface ([#357](https://github.com/jwilger/eventcore/pull/357))

## [0.5.1](https://github.com/jwilger/eventcore/compare/eventcore-examples-v0.5.0...eventcore-examples-v0.5.1) - 2026-02-22

### Features

- add eventcore-examples crate ([#285](https://github.com/jwilger/eventcore/pull/285))
- migrate integration tests to eventcore-examples and add deterministic store ([#295](https://github.com/jwilger/eventcore/pull/295))

### Miscellaneous Tasks

- release v0.1.3 ([#59](https://github.com/jwilger/eventcore/pull/59))

### Deps

- *(deps)* bump the minor-and-patch group with 14 updates
