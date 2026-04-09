---
name: store-backends
summary: Pluggable EventStore implementations for PostgreSQL, SQLite, and in-memory testing.
---

# Store Backends

Three interchangeable implementations of the event sourcing traits. Domain code is backend-agnostic; the choice of backend is a deployment decision.

## Overview

Each backend implements `EventStore`, `EventReader`, `CheckpointStore`, and `ProjectorCoordinator`. The in-memory backend is for testing, SQLite for embedded/single-instance, and PostgreSQL for production distributed deployments.

## Trait Implementation Matrix

| Trait                  | PostgreSQL                      | SQLite                           | In-Memory             |
| ---------------------- | ------------------------------- | -------------------------------- | --------------------- |
| `EventStore`           | ACID transactions               | WAL mode                         | Mutex-guarded HashMap |
| `EventReader`          | SQL queries with UUID7 ordering | SQL queries                      | Global log scan       |
| `CheckpointStore`      | Dedicated table                 | Dedicated table                  | HashMap               |
| `ProjectorCoordinator` | Advisory locks (distributed)    | In-memory locks (single-process) | In-memory locks       |

## PostgreSQL Backend

**Connection:** `sqlx` pool with configurable max connections, timeouts.

**Schema:**

- `eventcore_events` — stream_id, stream_version, event_type, event_data (JSON), event_id (UUID7)
- `eventcore_subscription_versions` — subscription_name, last_position (UUID7)

**Key features:**

- Database triggers enforce immutability (reject UPDATE/DELETE)
- Version checking via session-level config + trigger (atomic within transaction)
- Advisory locks (`pg_try_advisory_lock`) for distributed projector coordination
- FNV-1a hash of subscription name for lock key
- Guard releases lock via `pg_advisory_unlock()` on drop

**Files:** `eventcore-postgres/src/lib.rs`

## SQLite Backend

**Connection:** `rusqlite` with `Arc<Mutex<Connection>>`, async via `spawn_blocking`.

**Schema:** Same logical structure as PostgreSQL, TEXT columns for UUIDs/timestamps.

**Key features:**

- Optional SQLCipher encryption via `apply_encryption_key()`
- WAL journaling mode for better read concurrency
- Manual version checking (no trigger-based validation)
- In-memory coordination locks (single-process only)

**Files:** `eventcore-sqlite/src/lib.rs`

## In-Memory Backend

**Storage:** `Mutex<StoreData>` with `HashMap<StreamId, Vec<Box<dyn Any>>>` per stream and a `Vec<GlobalLogEntry>` for global ordering.

**Key features:**

- Type erasure via `Box<dyn Any + Send>` with downcast on read
- Zero dependencies beyond std
- All-or-nothing atomicity via single mutex lock
- Suitable for testing and single-process development

**Files:** `eventcore-memory/src/lib.rs`

## Common Patterns

All backends share:

- JSON serialization for event storage
- UUID7 for global event ordering
- Per-stream version tracking (event count from 0)
- Stream prefix filtering for EventReader
- Checkpoint-based resumable projection processing

## Files

| File                            | Description                          |
| ------------------------------- | ------------------------------------ |
| `eventcore-postgres/src/lib.rs` | PostgresEventStore + all trait impls |
| `eventcore-sqlite/src/lib.rs`   | SqliteEventStore + all trait impls   |
| `eventcore-memory/src/lib.rs`   | InMemoryEventStore + all trait impls |

## Related Systems

- [event-sourcing](event-sourcing.md) — Traits these backends implement
- [testing-infrastructure](testing-infrastructure.md) — Contract tests that verify backend correctness
- ADR-011: In-memory store crate location
- ADR-022: Crate reorganization for feature flags
