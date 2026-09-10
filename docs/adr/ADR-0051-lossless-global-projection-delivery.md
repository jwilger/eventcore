# ADR-0051: Lossless Global Projection Delivery

## Status

Accepted

## Date

2026-09-09

## Deciders

Project maintainers

## Supersession

This ADR supersedes the PostgreSQL cross-stream cursor and decode
behavior assumed by ADR-021, ADR-029, ADR-030, and ADR-037 for the transactional
projection facility. Existing `EventReader` behavior remains available for the
legacy API.

## Context

PostgreSQL `EventReader` in EventCore 2.0.1 uses a UUID event identifier as a
cross-stream cursor:

```sql
WHERE event_id > $checkpoint
ORDER BY event_id
```

UUIDs are allocated before their append transaction commits. If transaction A
allocates a lower UUID and commits after transaction B, a reader can observe B,
checkpoint its higher UUID, and permanently exclude A when it later commits.
UUID version 7 improves approximate creation ordering but does not establish a
commit frontier.

Replacing UUIDs with `BIGSERIAL`, `nextval`, timestamps, transaction IDs, or
commit timestamps does not by itself solve the problem. PostgreSQL sequence
values are allocated outside transaction rollback and concurrent transactions
may commit in another order.

The current reader also deserializes rows directly into a requested event type
and silently removes failures with `filter_map(... .ok())`. This behavior is
documented by the existing `EventReader`, so changing that trait in a minor
release would be a compatibility break. For a transactional projection,
however, a malformed selected row must be visible and must prevent progress.

A reusable source contract needs a stable, lossless, resumable sequence while
preserving event identity and explicit decode failure.

## Decision

Introduce a backend-independent strict delivery contract and a distinct,
source-scoped `DeliveryPosition`. The PostgreSQL implementation uses a positive
`BIGINT` global position allocated from a transactionally locked singleton
frontier.

### Delivery vocabulary

`eventcore-types` adds role-named types conceptually equivalent to:

```rust,ignore
pub struct DeliveryPosition(NonZeroU64);
pub struct DeliverySourceId(String);
pub struct ProjectorName(String);
pub struct ProjectionSelectionId(String);
pub struct PersistedEventId(Uuid);
pub struct EventTypeName(String);

pub enum DeliveryUpperBound {
    Inclusive(DeliveryPosition),
    Unbounded,
}

pub struct PersistedEventEnvelope {
    pub source_id: DeliverySourceId,
    pub position: DeliveryPosition,
    pub event_id: EventId,
    pub stream_id: StreamId,
    pub stream_version: StreamVersion,
    pub event_type: EventTypeName,
    pub payload: Box<serde_json::value::RawValue>,
    pub metadata: Box<serde_json::value::RawValue>,
}
```

Fields are private. `DeliveryPosition` and `PersistedEventId` have infallible
constructors for backend values plus read-only accessors. String-backed
identifiers reject empty or whitespace-only input and expose `as_ref()`.
Construction and persisted/configured decoding are property-tested.

The source trait is named `ProjectionSource` and exposes the following logical
operations without referring to SQLx:

```rust,ignore
pub trait ProjectionSource: Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    fn source_id(&self) -> &DeliverySourceId;

    fn high_watermark(
        &self,
    ) -> impl Future<Output = Result<Option<DeliveryPosition>, Self::Error>> + Send;

    fn read_envelopes(
        &self,
        selection: &ProjectionSelection,
        after: Option<DeliveryPosition>,
        through: DeliveryUpperBound,
        limit: BatchSize,
    ) -> impl Future<Output = Result<Vec<PersistedEventEnvelope>, Self::Error>> + Send;
}
```

`ProjectionSelection` contains the existing stream prefix-or-pattern selector,
one or more persisted event type names, and a stable
`ProjectionSelectionId`. The identifier is explicitly supplied and validated;
changing selection semantics requires changing it. This avoids pretending a
hash of configuration is a durable application contract.

The source contract must provide:

- a stable source identity;
- a committed high-water mark;
- bounded pages strictly after a position and optionally capped at a target;
- a stable, unique position for every selected persisted event;
- preservation of per-stream version order; and
- explicit source/storage errors without dropping rows.

The runner asks the projector's application-owned decode boundary to turn each
selected envelope into its application event contract. The default decoder uses
the payload JSON; an override can route using the persisted discriminator or
metadata. An unrelated event type excluded by the projector's declared filter
is not an error. A selected event whose discriminator or payload cannot decode
is a typed terminal failure, does not enter application failure policy, and does
not advance progress.

### PostgreSQL frontier

The PostgreSQL event database adds:

1. A singleton frontier row containing the last allocated delivery position.
2. An immutable mapping table from `delivery_position` to existing `event_id`.
3. An `AFTER INSERT ... REFERENCING NEW TABLE` statement trigger covering every
   insert statement against `eventcore_events`.

For each insert statement, the trigger locks the frontier once, allocates one
contiguous range, and inserts mappings ordered by
`(stream_id, stream_version, event_id)` within that statement. All insert
statements in one append share the outer append transaction, so the frontier row
lock is held until commit or rollback. A later transaction cannot allocate its
positions until the earlier allocator transaction resolves.

Consequently, if position N is visible to a reader, no lower allocated position
can become visible later. Rollback removes both the event mapping and the
transactional frontier update, permitting the range to be reused without an
observable hole. Positions are delivery order, not event identity or distributed
causality.

The mapping is stored separately because `eventcore_events` is protected by an
immutability trigger that rejects updates. Existing event IDs remain stable.

The delivery schema is an explicit, opt-in migration owned by the new
`PostgresProjectionSource`, not another entry in the existing `_sqlx_migrations`
ledger. SQLx 0.8.6 has no custom migration-table support, and an older binary's
default migrator rejects applied versions absent from its bundled migration
set. The new facility therefore serializes its idempotent schema steps with a
stable advisory lock and records their versions in a separate
`eventcore_projection_schema_versions` table.

The opt-in migration takes `ACCESS EXCLUSIVE` on `eventcore_events`, installs
its schema and insert trigger, then backfills all committed historical events
in `(stream_id, stream_version, event_id)` order. Writers blocked during the
migration resume only after the trigger is active. Already-running EventCore
2.0.1 writers and restarted 2.0.1 binaries can continue using the event store:
the trigger covers their inserts, while their bundled SQLx migrator never sees
the separate projection migration ledger.

Historical cross-stream commit order cannot be reconstructed from the existing
schema and is not claimed. The backfill defines a documented deterministic
delivery order for history while preserving per-stream order. New events use the
transactional frontier.

### Progress identity

Transactional progress is stored in a new table and namespace. Each row binds:

- projector name;
- delivery source identity;
- selected event/filter contract identity;
- delivery position; and
- update metadata needed for operations and observability.

Changing the source or selection identity cannot silently reuse progress. The
old UUID checkpoint table remains for the legacy API.

An existing UUID checkpoint cannot be translated safely by looking up the same
event in the new mapping: lower UUID events might already have been skipped.
Adoption therefore requires coordinated reset/replay under ADR-0050. An
application stops the legacy runner before transferring ownership of a read
model, atomically clears that model and its new progress, and replays the
deterministic delivery sequence.

### Paging and execution modes

Pages are bounded by validated nonzero batch size and ordered by
`DeliveryPosition`. The source never removes malformed selected rows from a
page.

Batch execution captures a committed high-water position once. If the source is
empty and returns no high-water position, batch returns immediately. Otherwise
it requests selected pages with `DeliveryUpperBound::Inclusive(high_water)` and
continues until the source returns an empty page. That empty bounded page
certifies that no selected event remains in `(checkpoint, high_water]`; the
projector's checkpoint need not equal the global frontier when trailing events
are unselected. Events committed after capture are left for a later run.

Continuous execution repeats the same bounded catch-up cycle and immediately
requests another page while selected events remain. It waits only after an
empty bounded page, including when the selection matches no events. On wake it
captures a new high-water position before reading again. Polling remains
authoritative; notifications may only shorten the wait.

### Ordering scope

The position establishes one total delivery order within a source identity. It
does not claim wall-clock order, universal order across databases, or causal
order across replicas. Per-stream versions remain the authoritative order inside
each event stream.

## Compatibility and release strategy

The strict delivery trait and `DeliveryPosition` are additive and target
EventCore 2.1.0. They do not replace UUID-backed `StreamPosition` or change the
existing `EventReader` promise that incompatible deserializations are skipped.
Legacy projections continue to use their existing checkpoint table and cursor
semantics.

Public documentation must distinguish the legacy reader from the transactional
delivery source and must not describe UUID identifiers as a safe PostgreSQL
commit frontier.

## Contract verification

Every reusable delivery backend must prove:

1. Every committed selected event is eventually delivered.
2. No committed selected event is delivered twice at different positions.
3. Resumption strictly after a committed position loses no later commit.
4. Per-stream version order is preserved.
5. Concurrent transactions cannot be skipped when identifier order differs from
   commit order.
6. A selected malformed event is returned or reported explicitly and blocks
   advancement.
7. Unselected event types do not consume selected-page capacity.
8. Pages larger than one batch are completely drainable.
9. A captured batch high-water mark produces finite catch-up.
10. Continuous polling observes events committed after initial catch-up.
11. Source or filter identity mismatch rejects incompatible saved progress.
12. An initially empty source completes batch even if producers start later.
13. An unselected event at the global frontier does not prevent catch-up.
14. A selection matching no events completes batch and waits in continuous
    mode.

The PostgreSQL ordering test must not wait for transaction B to commit while A
is deliberately paused after frontier allocation: correct serialization makes B
wait. The fixture instead controls transaction release independently and asserts
that the delivered event-ID set equals the committed set with no duplicates.

One deterministic regression fixture also commits a higher UUID first, consumes
it, then commits a lower UUID and verifies that the lower UUID still appears at
a later `DeliveryPosition`. The UUID values are test inputs, not asserted
ordering semantics.

## Consequences

### Positive

- A cross-stream checkpoint cannot move beyond a transaction that will appear
  later at a lower position.
- Event identity is no longer conflated with delivery progress.
- Decode failures become observable and non-lossy for transactional projections.
- Batch completion and continuous catch-up have precise contracts.
- Database enforcement protects the invariant when older application binaries
  append events.

### Negative

- Frontier allocation serializes the final insertion portion of concurrent
  append transactions across streams.
- Large appends hold the frontier lock longer and can increase tail latency.
- Migration requires a database lock and a full historical mapping backfill.
- Existing read models require coordinated reset/replay to adopt the new
  progress contract.
- Historical global commit order is unrecoverable and must be replaced with a
  deterministic replay order.

## Alternatives considered

### Plain sequence or identity column

Sequence allocation does not follow commit order and is not rolled back. It can
reproduce the same late-commit hole as UUID allocation.

### Per-stream progress vectors

Tracking `(projector, stream, version)` avoids a global append frontier but adds
stream discovery, vector checkpoints, scheduling/fairness, and no single
cross-stream fold order. It is a valid future facility, not the smallest
contract matching EventCore's current global projection model.

### Lexicographic stream/version cursor

New streams can sort behind an existing cursor, so a single lexicographic cursor
is not a lossless discovery mechanism.

### Post-commit materialized delivery feed

A durable dispatcher or logical-decoding consumer can preserve more append
concurrency, but introduces another service, recovery protocol, and operational
checkpoint. This contradicts the goal of eliminating application-owned
coordination machinery for the baseline facility.

### Commit timestamps or transaction IDs

These values are not a simple durable total-order cursor with the required
retention and visibility guarantees. Depending on them would also tie the API to
database configuration and wraparound/replication details.
