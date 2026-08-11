# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v1.0.1.html).

## [Unreleased]

## [2.0.0](https://github.com/jwilger/eventcore/compare/eventcore-fs-v1.1.1...eventcore-fs-v2.0.0) - 2026-08-11

### Features

- *(snapshots)* persist command state projections

### Refactoring

- *(testing)* stabilize reconstruction benchmarks

## [1.0.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-fs-v1.0.0...eventcore-fs-v1.0.1) - 2026-06-15

### Documentation

- overhaul API documentation across all crates ([#420](https://git.johnwilger.com/Slipstream/eventcore/pulls/420))
- align all documentation with the 1.0 API ([#424](https://git.johnwilger.com/Slipstream/eventcore/pulls/424))

## [1.0.0](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-fs-v0.9.0...eventcore-fs-v1.0.0) - 2026-06-13

### Features

- _(eventcore-types)_ glob pattern matching for subscriptions ([#246](https://git.johnwilger.com/Slipstream/eventcore/pulls/246)) ([#410](https://git.johnwilger.com/Slipstream/eventcore/pulls/410))
- _(eventcore)_ [**breaking**] streaming reads for read_stream ([#364](https://git.johnwilger.com/Slipstream/eventcore/pulls/364)) ([#414](https://git.johnwilger.com/Slipstream/eventcore/pulls/414))

### Miscellaneous Tasks

- graduate workspace to 1.0.0 for first stable release ([#418](https://git.johnwilger.com/Slipstream/eventcore/pulls/418))

### Performance

- serialize events once in the append path ([#361](https://git.johnwilger.com/Slipstream/eventcore/pulls/361)) ([#408](https://git.johnwilger.com/Slipstream/eventcore/pulls/408))

## [0.9.0](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-fs-v0.8.1...eventcore-fs-v0.9.0) - 2026-06-13

### Features

- _(eventcore-fs)_ local-ingestion cursor for projections ([#398](https://git.johnwilger.com/Slipstream/eventcore/pulls/398))
- _(eventcore-fs)_ replica-id fingerprint + conflict check ([#400](https://git.johnwilger.com/Slipstream/eventcore/pulls/400))
- _(eventcore-fs)_ read-time fsck + dangling-transaction handling ([#402](https://git.johnwilger.com/Slipstream/eventcore/pulls/402))

## [0.8.1](https://git.johnwilger.com/Slipstream/eventcore/compare/eventcore-fs-v0.8.0...eventcore-fs-v0.8.1) - 2026-06-13

### Features

- _(eventcore-fs)_ git-mergeable file-based event store backend ([#390](https://git.johnwilger.com/Slipstream/eventcore/pulls/390))
