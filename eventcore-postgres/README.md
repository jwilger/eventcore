# EventCore PostgreSQL

`eventcore-postgres` provides EventCore's PostgreSQL event store and the
transactional PostgreSQL projection runner. Most applications enable
`eventcore`'s `postgres` feature and use the facade:

```rust,ignore
use eventcore::postgres::{PostgresEventStore, projections::*};
```

The transactional projection API is additive for EventCore 2.1.0. The legacy
`eventcore::Projector` and `eventcore::run_projection` APIs from 2.0.1 remain
source-compatible and retain their existing non-atomic effect/checkpoint
semantics.

## Transactional guarantee

`run_transactional_projection` owns a transaction on the read-model database,
lends it to `PostgresProjector::apply`, advances named progress through the same
transaction, and commits once. Acknowledged commits therefore provide
effect-plus-progress exactly-once behavior inside that read-model database.

This guarantee does not include the event-store database, network calls, or any
other external system. The event source and read-model destination are separate
arguments and may use different pools, schemas, or PostgreSQL databases.

## Minimal wiring

Applications use their own SQLx API for pools and queries. Import `Postgres` and
`Transaction` from EventCore's facade so the projector signature cannot drift
from EventCore's SQLx version. This workspace currently resolves SQLx 0.8.6 in
`Cargo.lock`; downstream applications should use a compatible SQLx release for
their own query APIs. The complete compiled example is
`eventcore/tests/postgres_transactional_projection_api_test.rs`; its essential
wiring is:

```rust,ignore
use std::future::Future;

use eventcore::postgres::projections::{
    DeliveryPosition, DeliverySourceId, EventTypeName, NoopAfterCommit, Postgres,
    PostgresProjectionConfig, PostgresProjectionSource, PostgresProjectionStore,
    PostgresProjector, ProjectionSelection, ProjectionSelectionId,
    ProjectionStreamFilter, ProjectorName, Transaction, run_transactional_projection,
};
use serde::Deserialize;
use sqlx::{PgPool, query};

#[derive(Deserialize)]
struct ItemAdded {
    quantity: i64,
}

struct InventoryTotals {
    name: ProjectorName,
}

impl PostgresProjector for InventoryTotals {
    type Event = ItemAdded;
    type Error = sqlx::Error;
    type AfterCommit = NoopAfterCommit;

    fn name(&self) -> &ProjectorName {
        &self.name
    }

    fn apply<'a, 'c>(
        &'a mut self,
        event: &'a Self::Event,
        _position: DeliveryPosition,
        tx: &'a mut Transaction<'c, Postgres>,
    ) -> impl Future<Output = Result<Self::AfterCommit, Self::Error>> + Send + 'a
    where
        'c: 'a,
    {
        async move {
            let _ = query(
                "INSERT INTO inventory_total (singleton, quantity) VALUES (TRUE, $1) \
                 ON CONFLICT (singleton) DO UPDATE \
                 SET quantity = inventory_total.quantity + EXCLUDED.quantity",
            )
            .bind(event.quantity)
            .execute(&mut **tx)
            .await?;
            Ok(NoopAfterCommit)
        }
    }
}

async fn catch_up(source_pool: PgPool, destination_pool: PgPool)
    -> Result<(), Box<dyn std::error::Error>>
{
    let source = PostgresProjectionSource::from_pool(
        source_pool,
        DeliverySourceId::try_new("primary-event-store")?,
    );
    source.migrate().await?;

    let destination = PostgresProjectionStore::from_pool(destination_pool);
    destination.migrate().await?;

    let selection = ProjectionSelection::try_new(
        ProjectionSelectionId::try_new("inventory-events-v1")?,
        ProjectionStreamFilter::All,
        vec![EventTypeName::try_new("item-added")?],
    )?;
    let projector = InventoryTotals {
        name: ProjectorName::try_new("inventory-totals-v1")?,
    };

    let _ = run_transactional_projection(
        projector,
        &source,
        &destination,
        PostgresProjectionConfig::new(selection),
    )
    .await?;
    Ok(())
}
```

Before this wiring, run `PostgresEventStore::migrate()` on the source and the
application's read-model migration on the destination. Source and destination
projection migrations are separate and use an EventCore component ledger, not
the application's `_sqlx_migrations` history.

Treat the first `PostgresProjectionSource::migrate()` as a maintenance
operation. It takes an `ACCESS EXCLUSIVE` lock on `eventcore_events` and
backfills the complete event history, which can block reads and writes for a
substantial period on a large store. Measure it against representative data and
schedule an appropriate maintenance window. Historical rows are assigned a
deterministic backfill order of `(stream_id, stream_version, event_id)`; that is
not their original commit order. Subsequent appends allocate the global
delivery frontier inside the event-store transaction, providing commit-safe
ordering for newly written events.

## Operating the runner

- EventCore binds source and selection semantics only through caller-supplied
  IDs. Keep `DeliverySourceId`, `ProjectorName`, and `ProjectionSelectionId`
  stable while their meanings are stable. A changed stream filter or event-type
  set requires a new selection ID; changed source ordering semantics require a
  new source ID. Existing progress is rejected when the configured IDs differ.
- The selection contains persisted event-type names plus an all/prefix/pattern
  stream filter. By default, `PostgresProjector::decode` deserializes the
  envelope payload as JSON into `PostgresProjector::Event`. Override `decode`
  when one selection contains multiple persisted event types that need the
  envelope's `event_type` or `metadata` for application-owned routing, including
  types with identical payload shapes. Any selected decode or discriminator
  error stops terminally with `Decode`; it does not invoke `on_error`, retry, or
  advance progress.
- Batch mode captures a high-water mark and drains every page through it.
  Continuous mode polls successive high-water marks until its
  `CancellationToken` is cancelled. Cancellation is observed while idle after
  the current bounded catch-up cycle; it does not interrupt `apply`, retry
  delay, commit, or `AfterCommit` work.
- Application failures default to `Fatal`. `on_error` may explicitly choose
  `Retry`, `Skip`, `Stop`, or `Fatal`; retry attempts are bounded by
  `ProjectionRetryPolicy`. Only `Skip` commits progress without the effect.
- An `AfterCommit` action runs only after an acknowledged commit. If it fails,
  progress is already committed. Put durable external work in an outbox row
  written by `apply`; a callback or direct network call is not transactionally
  exactly once.
- `CommitIndeterminate` means PostgreSQL may have committed both effect and
  progress before the connection failed. Inspect durable progress/read-model or
  outbox state, then restart with the same identities.

A database rollback does not rewind fields mutated through the projector's
`&mut self`. Keep durable or retry-sensitive attempt state in the supplied
transaction, derive it again from the event/database, or make instance-state
changes explicitly rollback-safe.

## Leadership, reset, and replay

The destination holds a session advisory lock on the same physical connection
that owns every effect/progress transaction. Use a direct connection or session
pool. Transaction-pooling middleware cannot preserve this fencing guarantee.

Reset uses the same named leadership. `reset_transactional_projection` resets
the model and deletes progress atomically;
`reset_and_replay_transactional_projection` retains leadership through the
subsequent replay. Stop other read-model writers and schedule downtime. Legacy
UUID checkpoints cannot be adopted as delivery positions: reset, choose stable
transactional identities, and replay. To change an existing transactional
source or selection, invoke reset with the old source/selection IDs persisted
in progress, then run with the new IDs. A new `ProjectorName` alone does not
empty an already-populated model and can duplicate non-idempotent effects; use
it only with a new or empty model. Reset operates on the live model and does not
provide shadow-generation rebuilding.

## Release status

The additive API targets EventCore 2.1.0 but no crate containing it has been
published. Foundry and other consumers must not select it until publication is
explicitly approved. After that approved publication, Foundry's exact
recommendation is:

```toml
eventcore = { version = "=2.1.0", features = ["postgres"] }
```
