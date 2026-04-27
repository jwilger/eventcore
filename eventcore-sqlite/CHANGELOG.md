# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.1](https://github.com/jwilger/eventcore/compare/eventcore-sqlite-v0.8.0...eventcore-sqlite-v0.8.1) - 2026-04-27

### Deps

- *(deps)* bump the minor-and-patch group across 1 directory with 11 updates ([#383](https://github.com/jwilger/eventcore/pull/383))

## [0.8.0](https://github.com/jwilger/eventcore/compare/eventcore-sqlite-v0.7.1...eventcore-sqlite-v0.8.0) - 2026-04-27

### Features

- *(sqlite)* make encryption opt-in, re-export rusqlite, add BYOC constructors ([#379](https://github.com/jwilger/eventcore/pull/379))

### Breaking Changes

- The `encryption` feature is no longer enabled by default. Consumers who
  rely on SQLCipher must opt in explicitly via
  `features = ["encryption"]`. The default feature set is now `bundled`,
  which vendors vanilla SQLite without OpenSSL/SQLCipher native linking.

### Features

- Re-export `rusqlite` at the crate root so consumers can use
  `eventcore_sqlite::rusqlite` without declaring a separate dependency.
- Add `SqliteEventStore::from_connection` and
  `SqliteCheckpointStore::from_connection` for consumers that need to
  bring their own pre-configured `rusqlite::Connection`.

## [0.7.1](https://github.com/jwilger/eventcore/compare/eventcore-sqlite-v0.7.0...eventcore-sqlite-v0.7.1) - 2026-04-15

### Bug Fixes

- filter read_events by event_type to prevent projection stalls ([#373](https://github.com/jwilger/eventcore/pull/373))

## [0.7.0](https://github.com/jwilger/eventcore/compare/eventcore-sqlite-v0.6.0...eventcore-sqlite-v0.7.0) - 2026-04-13

### Bug Fixes

- apply PRAGMA key before WAL mode in SQLite encrypted stores ([#333](https://github.com/jwilger/eventcore/pull/333))
- improve error message consistency, context, and safety across all crates ([#352](https://github.com/jwilger/eventcore/pull/352))

### Miscellaneous Tasks

- consolidate workspace lints and enforce strict lint policy ([#351](https://github.com/jwilger/eventcore/pull/351))

## [0.6.0](https://github.com/jwilger/eventcore/compare/eventcore-sqlite-v0.5.1...eventcore-sqlite-v0.6.0) - 2026-03-15

### Bug Fixes

- improve type encapsulation, error context, and clean up stale TODOs ([#315](https://github.com/jwilger/eventcore/pull/315))

### Features

- add eventcore-sqlite crate with SQLCipher encryption support ([#310](https://github.com/jwilger/eventcore/pull/310))
