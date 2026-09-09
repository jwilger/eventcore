# Task 3 GREEN Report — Transactional PostgreSQL Projections

## Result

`GREEN_READY`. The first shared transactional atomicity contract now passes against a real
PostgreSQL delivery source and read model.

The batch runner captures a source high-water mark, acquires a dedicated leader connection,
pages selected envelopes, and for each one begins a transaction on that exact connection. It
locks and loads progress, validates source/selection identity, suppresses committed positions,
decodes the untouched raw payload, invokes `PostgresProjector::apply` with the same transaction,
upserts progress in it, commits, and only then invokes the after-commit hook.

## Leader and error behavior

- `PostgresProjectionStore::acquire_leader` checks out one `PoolConnection<Postgres>`, calls
  `close_on_drop()` before `pg_try_advisory_lock`, and holds it in internal `ProjectionLeader`.
- The advisory key is a stable 64-bit FNV-1a hash of the explicit
  `eventcore:transactional-projection:` namespace plus projector name.
- Only the leader obtains transactions; progress read/advance execute exclusively through those
  leader-originated transactions. Its release path unlocks and closes the connection.
- Any pre-commit error drops the transaction, so PostgreSQL rolls it back. Commit failure maps to
  `CommitIndeterminate`; no post-commit action runs without commit acknowledgement.

## Shared contract proof

The reusable `eventcore-testing::transactional_projection_contract` remains backend-neutral (it
does not import SQLx or the PostgreSQL adapter) and owns assertions for:

- effect count one after the initial selected event;
- exact durable source ID, selection ID, and position;
- first `CaughtUp { processed: 1, skipped: 0, through: Some(position) }` outcome;
- redelivery effect still one, unchanged progress, and exact second
  `CaughtUp { processed: 0, skipped: 0, through: Some(position) }` outcome.

Its PostgreSQL fixture appends valid `InvoiceIssued` data through public
`EventStore::append_events`/`StreamWrites`, reads it through `PostgresProjectionSource`, and
deserializes the source payload into the projector event before running. Raw SQL is limited to the
application-owned read model and deterministic fixture faults.

Each schema-isolated PostgreSQL fixture derives and retains its projector name from its unique
schema (`invoice-effect-<schema>`). This prevents a database-wide advisory lock collision between
parallel nextest fixtures while preserving one fixture's progress identity across repeated runs.
The concurrent regression synchronizes both projectors at `apply` with a barrier, proving two
independent fixtures acquire leadership and process one event each concurrently; a constant name
would make one runner return `LeadershipBusy` while the other waits at the barrier.

## Dependency and configuration evidence

Context7 verified tokio-util 0.7 cancellation behavior. Executed exactly:

```text
nix develop -c cargo add tokio-util@0.7 --package eventcore-postgres --features rt
```

The public configuration test covers all documented defaults (batch size 100, batch mode, zero
retries, 100 ms initial delay, multiplier 2.0, 30 s maximum delay, 1 s polling), every builder,
zero-poll rejection, and invalid retry multipliers. The destination migration test proves exact
component ledger row `projection-destination`, version `2`.

`eventcore-testing` has no PostgreSQL or SQLx dependency. The graph remains
`eventcore-postgres --dev--> eventcore-testing` without a reverse edge.

## Verification

- `nix develop -c cargo fmt --all` — passed.
- `nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test` — 5 passed.
- `nix develop -c cargo check -p eventcore-postgres --all-targets` — passed.
- `nix develop -c cargo check -p eventcore --all-features` — passed.
- `git diff --check` — passed.

## Remaining scope

Task 3 implements only the approved batch atomic GREEN path. Retry policy execution, explicit
retry/skip/stop/fatal decisions, continuous polling, reset, and leadership-loss/fault behavior
remain for later tasks.
