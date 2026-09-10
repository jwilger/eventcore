# ADR-0050: Transactional PostgreSQL Projections

## Status

Accepted

## Date

2026-09-09

## Deciders

Project maintainers

## Supersession

This ADR supersedes the PostgreSQL transaction-ownership and
runner-composition decisions in ADR-021, ADR-026, ADR-029, ADR-030, and
ADR-037. Their decisions remain in force for the retained legacy projection
API.

## Context

EventCore 2.0.1 exposes a synchronous `Projector`, `run_projection`,
`EventReader`, `CheckpointStore`, and `ProjectorCoordinator`. The PostgreSQL
adapter supplies implementations of the storage traits. These abstractions
provide useful vocabulary, polling, retry configuration, named checkpoints,
and single-runner coordination, but they cannot provide atomic read-model
effects and progress:

- `Projector::apply` is synchronous and receives an application-created
  context rather than a transaction owned by the runner.
- Applying an event and saving its checkpoint are separate effects.
- PostgreSQL checkpoint writes execute through a pool, independently of
  application read-model writes.
- Checkpoint failures are logged while the in-memory cursor continues.
- The advisory-lock guard owns an otherwise idle connection. Application and
  checkpoint writes use other connections, so losing the lock session does not
  fence the old runner.

Applications consequently need their own transactional runner, checkpoint
store, and fencing scheme to obtain correctness. EventCore should own those
reusable mechanics while applications retain ownership of event contracts,
read-model schemas, SQL mutations, query APIs, and product-specific
notifications.

The design must support an event source and read model in different databases.
It must not couple backend-independent EventCore traits to SQLx. It must also
preserve the stable 2.0.1 API unless a major release is justified.

## Decision

Add an opt-in transactional projection facility to
`eventcore-postgres::projections`. Keep the existing projection traits and
`eventcore::run_projection` operational and source-compatible.

### Crate boundaries

- `eventcore-types` owns only backend-independent delivery vocabulary and the
  strict source contract defined by ADR-0051. It has no SQLx dependency.
- `eventcore-postgres::projections` owns the asynchronous PostgreSQL projector
  trait, runner, read-model transaction lifecycle, progress schema,
  coordination, reset/replay, and PostgreSQL-specific errors and configuration.
- `eventcore` continues to expose the adapter through its existing optional
  `postgres` feature and `eventcore::postgres` re-export.
- `eventcore-testing` owns a reusable transactional projection contract suite.
  Backend-specific fault injection remains in the backend fixture.

A separate `eventcore-projections-postgres` crate is not introduced. SQLx is
already required by `eventcore-postgres`, and another published crate would add
versioning and documentation overhead without creating a necessary dependency
boundary.

The new PostgreSQL runner has dedicated configuration rather than depending on
`eventcore::ProjectionConfig`; depending from `eventcore-postgres` back to the
facade would create a cycle.

### Public API direction

The public entry point is named `run_transactional_projection`; its destination
handle and configuration are `PostgresProjectionStore` and
`PostgresProjectionConfig`:

```rust,ignore
eventcore::postgres::projections::run_transactional_projection(
    projector,
    &source,
    &projection_store,
    config,
).await
```

The event source and PostgreSQL read-model destination are distinct arguments.
Progress and coordination live with the read model because progress must commit
in the same transaction as its effects. The event source participates through
the replayable delivery contract; it is not part of a distributed transaction.

Applications implement a separate SQLx-facing trait conceptually equivalent to:

```rust,ignore
pub trait PostgresProjector: Send {
    type Event: Send + Sync;
    type Error: std::error::Error + Send + Sync + 'static;
    type AfterCommit: Send + 'static;

    fn name(&self) -> &ProjectorName;

    fn decode(
        &self,
        envelope: &PersistedEventEnvelope,
    ) -> Result<Self::Event, BoxedProjectionError>;

    fn apply<'a, 'c>(
        &'a mut self,
        event: &'a Self::Event,
        position: DeliveryPosition,
        tx: &'a mut sqlx::Transaction<'c, sqlx::Postgres>,
    ) -> impl Future<Output = Result<Self::AfterCommit, Self::Error>> + Send + 'a
    where
        'c: 'a;
}
```

The runner owns `begin`, checkpoint advancement, rollback, and `commit`. It
lends the transaction to `apply` only for the application mutation. The
application cannot replace the checkpoint executor with an independently pooled
store.

The projector receives the lossless persisted envelope at its application-owned
decode boundary. The default decoder retains payload-only JSON deserialization;
applications can override it to route multiple persisted event types using the
event-type discriminator or metadata. Decode errors are terminal and do not
enter application failure policy. The decoded event is owned and then borrowed
by `apply`, so retries do not require `Clone`. The after-commit value is owned
and cannot borrow the transaction. SQLx types used by the public API are
re-exported by `eventcore-postgres` so consumers can name the compatible types
without guessing the adapter's SQLx version.

This transaction-borrow signature compiles with the workspace's locked SQLx
0.8.6. The short mutable borrow remains distinct from the transaction's
connection lifetime so the runner can subsequently advance progress and commit.

### Transaction ownership and duplicate suppression

For every delivered event, the leader performs:

1. Begin a transaction on the leader-owned read-model connection.
2. Read the named projector's committed progress in that transaction.
3. If the delivered position is already committed, suppress it without calling
   application code.
4. Decode the selected envelope into the application event contract.
5. Await `projector.apply(event, position, &mut transaction)`.
6. Advance the same projector's progress using the same transaction.
7. Commit the transaction.
8. Invoke any in-process after-commit hook only after commit acknowledgement.

This gives the following invariant:

> An event's read-model effects and its progress advancement are one PostgreSQL
> commit on the read-model database.

Therefore a failed mutation cannot advance progress, a failed progress write
cannot expose the mutation, and a crash before commit leaves neither visible.
After a successful commit, restart sees the progress before deciding whether to
call the projector again.

Mutable Rust fields on the projector are not transactionally restored. Durable
or retry-sensitive projection state belongs in SQL executed through the supplied
transaction.

### Leadership and fencing

The runner checks out one physical read-model connection, configures it to close
rather than return to the pool on cancellation, and attempts the named
session-level PostgreSQL advisory lock on that connection. Disposal behavior is
established before the lock query is submitted so cancellation cannot return an
ambiguously lock-owning session to the pool.

The same physical connection:

- holds the advisory lock for the run;
- begins every application/progress transaction; and
- is never transparently replaced while the leadership grant is in force.

If the session remains alive, a second runner cannot acquire the lock. If the
session is lost, PostgreSQL releases its lock and aborts any open transaction;
the old runner has no other connection on which it can continue writing. A new
connection must acquire leadership and reload durable progress before work
resumes.

This contract requires PostgreSQL session affinity. Transaction-pooling proxies
are not supported for the leadership/read-model connection. Ordinary event
source reads may use their own pool or database.

### Failure semantics

The new API uses new, deliberately extensible error and control types rather
than adding variants to the exhaustive 2.0.1 `ProjectionError` or
`FailureStrategy` enums.

- **Retry**: Roll back the failed attempt, wait according to validated bounded
  policy, begin a fresh transaction, recheck durable progress, and reapply.
  Exhaustion is reported as a terminal error.
- **Skip**: Roll back the failed attempt. Only an explicit application decision
  permits a fresh transaction to record the skip and advance progress. Partial
  failed effects are never committed.
- **Stop**: Roll back and return a typed stopped outcome without advancing
  progress. The event remains pending.
- **Fatal**: Roll back and return the application failure immediately.
- **Progress failure**: Roll back and stop. Never retain a locally advanced
  cursor.
- **Leadership loss**: Stop; the lost session cannot perform further writes.
- **Commit acknowledgement loss**: Return a typed indeterminate outcome. A
  commit error does not prove rollback. Recovery reacquires leadership and
  checks durable progress before deciding whether delivery is pending.
- **After-commit failure**: Report that the projection position is committed and
  the hook failed. Never reapply the event because an in-process hook failed.

In-process after-commit hooks are best effort. Their guarantee is that they do
not run before a confirmed commit and never run for rolled-back state. Durable
notification requires an application-owned outbox row, or another transactional
mechanism, written through the supplied transaction.

### Batch and continuous execution

Batch mode captures the source's committed high-water mark at start and drains
successive pages through that target before returning. This makes completion
finite even while producers continue writing.

Continuous mode drains pages without sleeping while work remains. Once caught
up, it waits using a validated positive polling interval or cancellation signal;
it does not busy-loop. Database notifications may later be added only as wakeup
hints, with periodic polling retained as the correctness mechanism.

### Reset and replay

Basic reset uses the same named advisory lock as normal execution. If an active
runner holds leadership, reset returns `Busy`; it does not steal leadership.

After acquiring leadership, the runner invokes an application reset callback
and clears the named progress record in one read-model transaction. Failure
preserves both the old read model and its progress. A replay convenience retains
leadership across reset and catch-up so another compliant runner cannot observe
reset progress and race the requested replay.

Basic rebuild may expose an empty or partially reconstructed read model until
catch-up finishes. Atomic shadow-generation handoff is explicitly outside this
facility and will be designed only for a concrete consumer that requires
uninterrupted reads.

## Compatibility and release strategy

This facility is additive and targets a lockstep EventCore 2.1.0 release.
Existing synchronous projector implementations, custom legacy backends,
feature defaults, UUID-backed `StreamPosition`, and legacy checkpoint tables
remain available. The existing APIs are documented as legacy mechanics without
the new atomicity guarantee; they are not immediately deprecated.

Changing `Projector::apply`, replacing `StreamPosition`, tightening the existing
`EventReader` decode contract, or adding required methods to the existing
checkpoint/coordinator traits would require EventCore 3.0.0 and is rejected for
this increment.

Foundry should consume exact version `=2.1.0` with the `postgres` feature only
after implementation, CI, independent review, merge, and separately authorized
publication complete. This ADR does not authorize a crate release.

## Contract verification

The reusable public-boundary suite must cover:

1. Effect and progress commit atomically.
2. Mutation failure rolls back progress.
3. Progress failure rolls back mutation.
4. Interrupted or indeterminate commit reports a truthful, recoverable outcome.
5. Restart resumes from committed progress.
6. Redelivery does not duplicate committed non-idempotent effects.
7. Leadership excludes a second writer.
8. Leadership loss prevents stale writes.
9. Retry exhaustion is bounded and observable.
10. Skip advances only after an explicit decision and without failed effects.
11. Stop and fatal outcomes leave the event pending.
12. Reset plus replay reconstructs the expected state.
13. Reset exclusion and reset rollback preserve consistency.
14. After-commit hooks run only after confirmed commit.

PostgreSQL fixtures may use backend PIDs, conditional triggers, or a narrow
connection proxy for deterministic faults. Assertions remain about the public
outcome, read-model state, progress, and hook observations rather than internal
SQL text or retry counters.

## Consequences

### Positive

- Applications no longer need parallel runners, progress stores, or fencing
  frameworks for PostgreSQL read models.
- Non-idempotent effects gain transaction-scoped duplicate suppression.
- Leadership loss is fenced by the same resource that authorizes writes.
- Mixed event-store/read-model databases are first-class.
- SQLx remains outside backend-independent EventCore traits.
- Existing 2.0.1 consumers retain source compatibility.

### Negative

- EventCore exposes a second projection API with stronger, PostgreSQL-specific
  guarantees.
- One physical read-model connection is reserved per active projector.
- Application SQL performed outside the supplied transaction is outside the
  atomicity and fencing guarantees.
- In-process after-commit hooks remain lossy across process failure.
- Basic rebuild has a read-availability window.

## Alternatives considered

### Change the existing `Projector` trait

This offers one conceptual API but breaks every projector and still requires a
transaction ownership redesign. It is appropriate only for a future major
release.

### Add a generic transaction abstraction to `eventcore-types`

This introduces lifetime-carrying backend transaction types without a second
transactional backend demonstrating the shared abstraction. It is deferred.

### Add `eventcore-projections-postgres`

This avoids placing the runner in the store adapter but adds a published crate
and release boundary. It can be reconsidered if the facility later gains an
independent dependency or release lifecycle.

### Fence with leases or epoch columns

Epoch fencing is necessary when writes can occur on connections independent of
leadership. Keeping every write on the session that owns the lock is smaller and
provides the required fence directly.
