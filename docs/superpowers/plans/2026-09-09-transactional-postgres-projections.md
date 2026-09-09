# Transactional PostgreSQL Projections Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver an additive PostgreSQL projection facility that provides lossless ordered delivery, same-transaction read-model effects and progress, session-fenced single-writer execution, explicit failures, reset/replay, reusable contract tests, documentation, and a reviewed pull request.

**Architecture:** `eventcore-types` defines a SQLx-free strict delivery contract. `eventcore-postgres::projections` implements the PostgreSQL delivery source, destination progress store, projector API, runner, leadership, and reset/replay; the leader's one physical read-model connection owns both the session advisory lock and every application/progress transaction. A transactional frontier and immutable event-position mapping make PostgreSQL cross-stream delivery resumable without treating pre-commit UUIDs as commit order.

**Tech Stack:** Rust 2024, SQLx 0.8.6, PostgreSQL 17, Tokio 1.52, tokio-util cancellation, serde/serde_json, thiserror, nutype, proptest, cargo-nextest, GitHub Actions.

**Spec:** `docs/adr/ADR-0050-transactional-postgresql-projections.md` and `docs/adr/ADR-0051-lossless-global-projection-delivery.md`

## Global Constraints

- Preserve the existing `Projector`, `run_projection`, `EventReader`, `CheckpointStore`, `ProjectorCoordinator`, UUID `StreamPosition`, and legacy PostgreSQL tables as source-compatible 2.0.1 APIs.
- Target a lockstep additive EventCore `2.1.0`; do not publish any crate without explicit owner approval.
- Keep backend-independent traits free of SQLx.
- Support distinct event-source and read-model PostgreSQL databases.
- Use one leader-owned physical read-model connection for the advisory lock and every effect/progress transaction; transaction-pooling proxies are unsupported for that connection.
- Decode selected envelopes explicitly; malformed selected events never advance progress.
- Batch mode captures one high-water mark and drains through it; continuous mode waits only after an empty bounded page and never busy-loops.
- Reset/replay uses the same named leadership lock and does not implement shadow generations.
- Add dependencies only with Cargo CLI commands and commit `Cargo.lock`.
- Tests use public APIs and Given/When/Then structure; backend-specific deterministic fault injection may use raw PostgreSQL controls.
- Run all commands inside `nix develop`; format before every signed Conventional Commit.

---

## File Structure

- `eventcore-types/src/projection_delivery.rs`: validated delivery identities, positions, envelopes, selections, bounds, and `ProjectionSource`.
- `eventcore-types/src/projection_delivery.test.rs`: constructors, validation, accessors, and property tests.
- `eventcore-types/src/lib.rs`: public delivery-vocabulary re-exports.
- `eventcore-postgres/projection-migrations/0001_delivery_source.sql`: separately ledgered source frontier, immutable mapping, trigger, and history backfill.
- `eventcore-postgres/projection-migrations/0002_projection_destination.sql`: separately ledgered destination progress table.
- `eventcore-postgres/src/projections/mod.rs`: stable public exports and top-level module documentation.
- `eventcore-postgres/src/projections/migration.rs`: advisory-locked, component-scoped, idempotent projection migrations.
- `eventcore-postgres/src/projections/source.rs`: `PostgresProjectionSource` and strict bounded-page queries.
- `eventcore-postgres/src/projections/config.rs`: validated run modes, retry policy, polling, and cancellation configuration.
- `eventcore-postgres/src/projections/projector.rs`: `PostgresProjector`, failure decisions, reset callback, and after-commit action contracts.
- `eventcore-postgres/src/projections/error.rs`: typed run, reset, commit-indeterminate, and after-commit outcomes.
- `eventcore-postgres/src/projections/store.rs`: `PostgresProjectionStore`, progress identity, leader acquisition, and same-transaction progress operations.
- `eventcore-postgres/src/projections/runner.rs`: bounded page loop, per-event transaction state machine, retry/skip/stop/fatal logic, and continuous waiting.
- `eventcore-postgres/src/projections/reset.rs`: reset and reset-and-replay while retaining leadership.
- `eventcore-postgres/src/lib.rs`: `pub mod projections` and compatible SQLx type re-exports.
- `eventcore-testing/src/projection_contract.rs`: backend-neutral transactional projection fixture and reusable assertions.
- `eventcore-testing/src/lib.rs`: contract-suite exports.
- `eventcore-postgres/tests/projection_delivery_contract_test.rs`: PostgreSQL strict-delivery fixture and contract invocation.
- `eventcore-postgres/tests/transactional_projection_contract_test.rs`: public runner fixture, deterministic failures, and contract invocation.
- `eventcore-postgres/tests/projection_reset_test.rs`: reset exclusion, rollback, and replay tests.
- `eventcore-postgres/tests/projection_continuous_test.rs`: post-catch-up delivery and cancellation tests.
- `eventcore-postgres/tests/common/mod.rs`: isolated database/schema helpers shared by projection tests.
- `eventcore/tests/postgres_transactional_projection_api_test.rs`: facade re-export compile/use test behind `postgres`.
- `docs/manual/01-introduction/04-architecture.md`: correct legacy guarantees and document the transactional read path.
- `docs/manual/02-getting-started/04-projections.md`: legacy/new API distinction and complete PostgreSQL example.
- `eventcore-postgres/README.md`: operational migration, proxy, identity, reset, and failure guidance.
- `.github/workflows/ci.yml`: narrow PostgreSQL fault-contract job if measured runtime would make the ordinary test job exceed ten minutes.

---

### Task 1: Strict Delivery Vocabulary and Source Contract

**Files:**

- Create: `eventcore-types/src/projection_delivery.rs`
- Create: `eventcore-types/src/projection_delivery.test.rs`
- Modify: `eventcore-types/src/lib.rs`

**Interfaces:**

- Produces: `DeliveryPosition`, `DeliverySourceId`, `ProjectorName`, `ProjectionSelectionId`, `PersistedEventId`, `EventTypeName`, `DeliveryUpperBound`, `ProjectionStreamFilter`, `ProjectionSelection`, `PersistedEventEnvelope`, `ProjectionSource`.
- Consumes: existing `BatchSize`, `StreamId`, `StreamPrefix`, `StreamPattern`, and `StreamVersion`.

- [ ] **Step 1: Write failing public-boundary and property tests**

Add tests proving positive positions, trimmed non-empty identities, non-empty unique event-type selections, exact accessors, raw JSON preservation, and strict forwarding through `&T`. Include property tests equivalent to:

```rust
proptest! {
    #[test]
    fn identity_accepts_exactly_non_blank_values(value in ".*") {
        let expected = !value.trim().is_empty();
        prop_assert_eq!(DeliverySourceId::try_new(value).is_ok(), expected);
    }

    #[test]
    fn delivery_position_round_trips(value in 1_u64..=u64::MAX) {
        let position = DeliveryPosition::new(NonZeroU64::new(value).unwrap());
        prop_assert_eq!(position.get(), value);
    }
}
```

- [ ] **Step 2: Run RED and obtain independent review**

Run:

```bash
nix develop -c cargo nextest run -p eventcore-types -E 'test(projection_delivery)'
```

Expected: compile failure because the delivery API is absent. Preserve the focused diagnostic and have an independent test reviewer confirm the tests express ADR-0051 without coupling to SQLx.

- [ ] **Step 3: Implement the validated vocabulary and source trait**

Use the following public shape, with private fields, documented constructors, accessors, and `thiserror` validation errors:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeliveryPosition(NonZeroU64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryUpperBound {
    Inclusive(DeliveryPosition),
    Unbounded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionStreamFilter {
    All,
    Prefix(StreamPrefix),
    Pattern(StreamPattern),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionSelection {
    id: ProjectionSelectionId,
    stream_filter: ProjectionStreamFilter,
    event_types: Vec<EventTypeName>,
}

pub trait ProjectionSource: Sync {
    type Error: Error + Send + Sync + 'static;

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

`ProjectionSelection::try_new` must reject an empty event-type vector and duplicate event type names. `PersistedEventEnvelope::new` takes all validated fields and `Box<RawValue>` payload/metadata, and exposes borrowed accessors plus copy accessors for position/version/event ID.

- [ ] **Step 4: Run GREEN and compatibility checks**

Run:

```bash
nix develop -c cargo nextest run -p eventcore-types
nix develop -c cargo check -p eventcore --all-features
```

Expected: all tests pass; no existing projection signature changes.

- [ ] **Step 5: Format, review GREEN, and commit**

```bash
nix develop -c cargo fmt --all
git add eventcore-types/src/projection_delivery.rs eventcore-types/src/projection_delivery.test.rs eventcore-types/src/lib.rs
nix develop -c git commit -S -m "feat(projections): add strict delivery contract"
```

---

### Task 2: PostgreSQL Lossless Delivery Frontier and Source

**Files:**

- Create: `eventcore-postgres/projection-migrations/0001_delivery_source.sql`
- Create: `eventcore-postgres/src/projections/mod.rs`
- Create: `eventcore-postgres/src/projections/migration.rs`
- Create: `eventcore-postgres/src/projections/source.rs`
- Create: `eventcore-postgres/tests/projection_delivery_contract_test.rs`
- Modify: `eventcore-postgres/src/lib.rs`
- Modify: `eventcore-postgres/tests/common/mod.rs`

**Interfaces:**

- Consumes: all Task 1 delivery types and `PostgresEventStore`'s existing `eventcore_events` schema.
- Produces: `PostgresProjectionSource::{new, with_config, from_pool, migrate}`, `PostgresProjectionSourceError`, and a `ProjectionSource` implementation.

- [ ] **Step 1: Write RED delivery contract tests**

Create public-boundary tests for empty source, bounded pages, more than one page, prefix/pattern/event-type filtering before `LIMIT`, unselected trailing frontier, no-match selection, malformed raw payload visibility, stable source identity, historical backfill, direct writes by legacy clients, and the controlled UUID/commit-order regression. The concurrency fixture must begin transaction A, allocate its trigger frontier, begin transaction B, verify B is blocked rather than awaiting it inline, release A, then assert the delivered event-ID set equals the committed set.

The core regression assertion is:

```rust
let first_page = source.read_envelopes(&selection, None, DeliveryUpperBound::Unbounded, one).await?;
commit_lower_uuid_transaction().await?;
let second_page = source.read_envelopes(
    &selection,
    Some(first_page[0].position()),
    DeliveryUpperBound::Unbounded,
    one,
).await?;
assert_eq!(second_page[0].event_id(), lower_uuid_event_id);
assert!(second_page[0].position() > first_page[0].position());
```

- [ ] **Step 2: Run RED and obtain independent transaction/concurrency review**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test projection_delivery_contract_test
```

Expected: compile failure because `PostgresProjectionSource` is absent. Have independent transaction and concurrency reviewers verify the test controls do not assume sequence allocation equals commit order and cannot deadlock by waiting for B while A is paused.

- [ ] **Step 3: Implement the separate source migration ledger**

`run_component_migration(pool, "delivery-source", 1, SQL)` must acquire a stable transaction-level advisory lock, create `eventcore_projection_schema_versions(component TEXT, version BIGINT, applied_at TIMESTAMPTZ, PRIMARY KEY(component, version))`, check the component/version row, execute the SQL, insert the ledger row, and commit. It must not touch `_sqlx_migrations`.

The migration SQL must use this schema and trigger algorithm:

```sql
LOCK TABLE eventcore_events IN ACCESS EXCLUSIVE MODE;

CREATE TABLE IF NOT EXISTS eventcore_projection_delivery_frontier (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    last_position BIGINT NOT NULL CHECK (last_position >= 0)
);
INSERT INTO eventcore_projection_delivery_frontier(singleton, last_position)
VALUES (TRUE, 0) ON CONFLICT (singleton) DO NOTHING;

CREATE TABLE IF NOT EXISTS eventcore_projection_delivery (
    delivery_position BIGINT PRIMARY KEY CHECK (delivery_position > 0),
    event_id UUID NOT NULL UNIQUE REFERENCES eventcore_events(event_id)
);
```

Backfill unmapped committed history ordered by `(stream_id, stream_version, event_id)`, update the frontier in the same transaction, then install an `AFTER INSERT ... REFERENCING NEW TABLE AS inserted_events FOR EACH STATEMENT` trigger. Its function locks the singleton row `FOR UPDATE`, orders transition rows by `(stream_id, stream_version, event_id)`, assigns a contiguous range with `row_number()`, inserts immutable mappings, and advances the frontier by exactly the transition-row count.

- [ ] **Step 4: Implement strict source queries**

`high_watermark` reads `NULLIF(last_position, 0)`. `read_envelopes` joins mapping to events, applies the exclusive `after` bound and optional inclusive `through` bound, applies stream and event-type predicates before `ORDER BY delivery_position LIMIT`, and maps every row with fallible conversions. Never deserialize the application event and never use `filter_map`.

- [ ] **Step 5: Run GREEN and legacy migration compatibility proof**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test projection_delivery_contract_test
nix develop -c cargo nextest run -p eventcore-postgres --test postgres_internals_test
```

Also run the repository's 2.0.1 `PostgresEventStore::migrate()` code path after projection migration in a test and assert it succeeds because `_sqlx_migrations` contains no projection versions.

- [ ] **Step 6: Format, review GREEN, and commit**

```bash
nix develop -c cargo fmt --all
git add eventcore-postgres/projection-migrations eventcore-postgres/src/projections eventcore-postgres/src/lib.rs eventcore-postgres/tests/common eventcore-postgres/tests/projection_delivery_contract_test.rs
nix develop -c git commit -S -m "feat(postgres): add lossless projection delivery"
```

---

### Task 3: Public Transactional Projector API and Reusable Contract Harness

**Files:**

- Create: `eventcore-postgres/projection-migrations/0002_projection_destination.sql`
- Create: `eventcore-postgres/src/projections/config.rs`
- Create: `eventcore-postgres/src/projections/projector.rs`
- Create: `eventcore-postgres/src/projections/error.rs`
- Create: `eventcore-postgres/src/projections/store.rs`
- Create: `eventcore-postgres/src/projections/runner.rs`
- Create: `eventcore-testing/src/projection_contract.rs`
- Create: `eventcore-postgres/tests/transactional_projection_contract_test.rs`
- Modify: `eventcore-postgres/Cargo.toml` through Cargo CLI
- Modify: `Cargo.lock` through Cargo CLI
- Modify: `eventcore-postgres/src/projections/mod.rs`
- Modify: `eventcore-postgres/src/lib.rs`
- Modify: `eventcore-testing/src/lib.rs`

**Interfaces:**

- Consumes: Task 1 `ProjectionSource`, Task 2 source migration helper, SQLx `Transaction<'_, Postgres>`.
- Produces: `PostgresProjector`, `AfterCommit`, `NoopAfterCommit`, `ProjectionFailureContext`, `ProjectionFailureDecision`, `PostgresProjectionConfig`, `PostgresProjectionMode`, `ProjectionRetryPolicy`, `PostgresProjectionStore`, `ProjectionRunOutcome`, `TransactionalProjectionError`, `run_transactional_projection`, and `transactional_projection_contract`.

- [ ] **Step 1: Add cancellation dependency through Cargo CLI**

After refreshing tokio-util documentation through Context7, run:

```bash
nix develop -c cargo add tokio-util@0.7 --package eventcore-postgres --features rt
```

Confirm `Cargo.toml` and `Cargo.lock` are the only dependency files changed.

- [ ] **Step 2: Define the public API skeleton and reusable fixture contract**

The projector and after-commit contracts are:

```rust
pub trait AfterCommit: Send + 'static {
    type Error: Error + Send + Sync + 'static;
    fn execute(self) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

pub trait PostgresProjector: Send {
    type Event: DeserializeOwned + Send + Sync;
    type Error: Error + Send + Sync + 'static;
    type AfterCommit: AfterCommit;

    fn name(&self) -> &ProjectorName;

    fn apply<'a, 'c>(
        &'a mut self,
        event: &'a Self::Event,
        position: DeliveryPosition,
        tx: &'a mut Transaction<'c, Postgres>,
    ) -> impl Future<Output = Result<Self::AfterCommit, Self::Error>> + Send + 'a
    where
        'c: 'a;

    fn on_error(
        &mut self,
        failure: ProjectionFailureContext<'_, Self::Error>,
    ) -> ProjectionFailureDecision {
        let _ = failure;
        ProjectionFailureDecision::Fatal
    }
}
```

`ProjectionFailureDecision` is `Retry | Skip | Stop | Fatal`. `ProjectionFailureContext` exposes the position, error, and `AttemptNumber`. `NoopAfterCommit` implements `AfterCommit<Error = Infallible>`.

The config is constructed as `PostgresProjectionConfig::new(selection)` with default batch size 100, batch mode, zero retries, 100 ms initial retry delay, multiplier 2.0, 30 s maximum retry delay, and a 1 s positive continuous poll interval. Builders validate zero poll durations and non-finite/less-than-one multipliers. `continuous(cancellation: CancellationToken)` changes mode.

The reusable `eventcore-testing` fixture must expose only behavioral controls/results: append values/malformed input, select application behavior, run batch/continuous, inject progress failure or connection loss, read effect count/progress/hook log, start/lose leadership, and reset. The suite owns the sixteen named assertions from the request; the PostgreSQL fixture implements them only through public EventCore APIs plus raw SQL fault setup.

The primary entry point has this stable shape; its non-generic error enum uses
named variants and boxed sources so source and application error types do not
infect every caller signature:

```rust
pub async fn run_transactional_projection<P, S>(
    projector: P,
    source: &S,
    store: &PostgresProjectionStore,
    config: PostgresProjectionConfig,
) -> Result<ProjectionRunOutcome, TransactionalProjectionError>
where
    P: PostgresProjector,
    S: ProjectionSource;
```

`ProjectionRunOutcome` contains `CaughtUp { processed, skipped, through }`,
`Stopped { position, processed, skipped }`, and
`Cancelled { processed, skipped }`. `TransactionalProjectionError` has distinct
variants for leadership busy/lost, source, decode, application fatal, retry
exhaustion, progress, commit indeterminate, identity mismatch, configuration,
and after-commit failure.

- [ ] **Step 3: Write the first meaningful RED contract test**

Implement and invoke `effect_and_progress_commit_atomically`. Its projector executes a non-idempotent increment through the supplied transaction; after one run it asserts both increment and progress exist, then redelivers and asserts the increment remains one. Stage the RED test and obtain independent review before runner implementation.

- [ ] **Step 4: Run RED**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test -E 'test(effect_and_progress_commit_atomically)'
```

Expected: behavioral failure because the runner has not yet processed and committed the event.

- [ ] **Step 5: Implement destination migration, leadership, and progress primitives**

The destination table is:

```sql
CREATE TABLE IF NOT EXISTS eventcore_projection_progress (
    projector_name TEXT PRIMARY KEY,
    source_id TEXT NOT NULL,
    selection_id TEXT NOT NULL,
    last_position BIGINT NOT NULL CHECK (last_position > 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

`PostgresProjectionStore::migrate` uses component `projection-destination`, version 2, and the separate ledger. Leader acquisition checks out one `PoolConnection<Postgres>`, immediately calls `close_on_drop()`, then submits `pg_try_advisory_lock`. The lock key hashes an explicit namespace plus projector name with the existing stable FNV-1a implementation. The acquired connection stays inside an internal `ProjectionLeader` and is the only executor accepted by `begin`, progress load/validate, progress advance, and unlock/close operations.

- [ ] **Step 6: Implement minimal atomic GREEN path**

For each envelope: begin on the leader connection; lock/load progress; reject source/selection mismatch; suppress `position <= committed`; deserialize payload with `serde_json::from_str`; call `apply`; upsert progress through the same transaction; commit; execute after-commit action. Any pre-commit error rolls back or drops the transaction before return.

- [ ] **Step 7: Run GREEN and focused compatibility checks**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test -E 'test(effect_and_progress_commit_atomically)'
nix develop -c cargo check -p eventcore-postgres --all-targets
nix develop -c cargo check -p eventcore --all-features
```

- [ ] **Step 8: Review GREEN and commit**

```bash
nix develop -c cargo fmt --all
git add Cargo.lock eventcore-postgres/Cargo.toml eventcore-postgres/projection-migrations/0002_projection_destination.sql eventcore-postgres/src/projections eventcore-postgres/src/lib.rs eventcore-testing/src/projection_contract.rs eventcore-testing/src/lib.rs eventcore-postgres/tests/transactional_projection_contract_test.rs
nix develop -c git commit -S -m "feat(postgres): run projections transactionally"
```

---

### Task 4: Rollback, Restart, Decode, and Identity Guarantees

**Files:**

- Modify: `eventcore-postgres/src/projections/runner.rs`
- Modify: `eventcore-postgres/src/projections/error.rs`
- Modify: `eventcore-postgres/src/projections/store.rs`
- Modify: `eventcore-testing/src/projection_contract.rs`
- Modify: `eventcore-postgres/tests/transactional_projection_contract_test.rs`

**Interfaces:**

- Consumes: Task 3 runner and fixture.
- Produces: typed `Decode`, `Progress`, `SourceIdentityMismatch`, `SelectionIdentityMismatch`, and `CommitIndeterminate` errors with truthful positions; durable restart and duplicate suppression.

- [ ] **Step 1: Add RED contract cases**

Enable contract cases for mutation failure rolling back progress, progress failure rolling back mutation, restart from last committed position, redelivery suppression, malformed selected envelope stopping without advancement, source mismatch, selection mismatch, and interrupted commit. Assert database state and public variants, not SQL strings or backend PIDs.

- [ ] **Step 2: Run RED and independently review fault truthfulness**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test -E 'test(/(rolls_back|restart|redelivery|malformed|identity|commit)/)'
```

The reviewer must verify the commit fault can yield an unknown acknowledgement and that the assertion accepts only `CommitIndeterminate { position, .. }`, never a false rollback claim.

- [ ] **Step 3: Implement error mapping and recovery behavior**

All begin/load/decode/apply/progress failures happen before commit and leave the position pending. Any `Transaction::commit()` error maps to `CommitIndeterminate`; the runner must not invoke the after-commit action or retry the same in-memory delivery. A later invocation reacquires leadership and reads durable progress before deciding whether to apply. Identity mismatch is detected before application code runs.

- [ ] **Step 4: Run GREEN and commit**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test
nix develop -c cargo fmt --all
git add eventcore-postgres/src/projections eventcore-testing/src/projection_contract.rs eventcore-postgres/tests/transactional_projection_contract_test.rs
nix develop -c git commit -S -m "fix(projections): preserve transactional recovery invariants"
```

---

### Task 5: Typed Retry, Skip, Stop, Fatal, and After-Commit Semantics

**Files:**

- Modify: `eventcore-postgres/src/projections/config.rs`
- Modify: `eventcore-postgres/src/projections/projector.rs`
- Modify: `eventcore-postgres/src/projections/error.rs`
- Modify: `eventcore-postgres/src/projections/runner.rs`
- Modify: `eventcore-testing/src/projection_contract.rs`
- Modify: `eventcore-postgres/tests/transactional_projection_contract_test.rs`

**Interfaces:**

- Produces: bounded fresh-transaction retry, explicit skip advancement, stopped outcome, fatal error, and `AfterCommitFailed { committed_position, source }`.

- [ ] **Step 1: Add RED contract cases**

Enable retry exhaustion, transient retry success, explicit skip, stop, fatal, after-commit success ordering, after-commit suppression on rollback, and after-commit failure. The retry projector writes an attempt row inside each failed transaction so the test also proves failed-attempt rows roll back; an external atomic counter proves the bounded invocation count is exactly initial attempt plus configured retries.

- [ ] **Step 2: Run RED and obtain independent recovery review**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test -E 'test(/(retry|skip|stop|fatal|after_commit)/)'
```

- [ ] **Step 3: Implement the per-event state machine**

On `Retry`, roll back, sleep the capped validated delay, begin a fresh transaction, reload progress, and reapply. On exhaustion return `RetryExhausted` with the last application error and attempt count. On `Skip`, roll back the failed transaction, begin a fresh transaction, reload progress, advance only progress, commit, and record one skipped event in `ProjectionRunOutcome`. On `Stop`, roll back and return `ProjectionRunOutcome::Stopped { position }`. On `Fatal`, roll back and return `Application`. Run `AfterCommit::execute` only after confirmed commit; if it fails return `AfterCommitFailed` carrying the committed position so callers never reapply it.

- [ ] **Step 4: Run GREEN and commit**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test
nix develop -c cargo fmt --all
git add eventcore-postgres/src/projections eventcore-testing/src/projection_contract.rs eventcore-postgres/tests/transactional_projection_contract_test.rs
nix develop -c git commit -S -m "feat(projections): add explicit failure policies"
```

---

### Task 6: Fenced Leadership, Batch Drain, and Continuous Catch-Up

**Files:**

- Modify: `eventcore-postgres/src/projections/store.rs`
- Modify: `eventcore-postgres/src/projections/runner.rs`
- Modify: `eventcore-postgres/src/projections/error.rs`
- Modify: `eventcore-testing/src/projection_contract.rs`
- Modify: `eventcore-postgres/tests/transactional_projection_contract_test.rs`
- Create: `eventcore-postgres/tests/projection_continuous_test.rs`

**Interfaces:**

- Produces: `LeadershipBusy`, fenced `LeadershipLost`, finite `CaughtUp { processed, skipped, through }`, continuous cancellation, and no busy loop.

- [ ] **Step 1: Add RED leadership and execution-mode cases**

Enable second-writer exclusion, forced backend termination of the leader connection, more-than-one-page batch drain, fixed high-water behavior under concurrent appends, initially empty source, unselected frontier, no-match selection, continuous delivery after catch-up, positive idle waiting, and cancellation. Leadership-loss assertions must prove no later effect or progress write occurs from the stale runner.

- [ ] **Step 2: Run RED and obtain independent concurrency review**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test -E 'test(/(leadership|batch|page)/)'
nix develop -c cargo nextest run -p eventcore-postgres --test projection_continuous_test
```

- [ ] **Step 3: Implement bounded catch-up and continuous wait**

Batch captures `high_watermark()` once; `None` returns caught up immediately. Read `Inclusive(target)` pages strictly after committed progress until one page is empty, including when the checkpoint is below a trailing unselected global event. Continuous mode repeats a fresh bounded cycle, then `tokio::select!` waits for either `sleep(poll_interval)` or `CancellationToken::cancelled()`. Never sleep between non-empty pages. Report cancellation as a normal `ProjectionRunOutcome::Cancelled`, not failure.

Any database error on the leader connection is terminal for that leadership grant. Because `close_on_drop` was set before the lock query and all writes use that connection, dropping the grant closes the session instead of returning an ambiguously lock-owning connection to the pool.

- [ ] **Step 4: Run GREEN and commit**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test projection_delivery_contract_test
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test
nix develop -c cargo nextest run -p eventcore-postgres --test projection_continuous_test
nix develop -c cargo fmt --all
git add eventcore-postgres/src/projections eventcore-testing/src/projection_contract.rs eventcore-postgres/tests/transactional_projection_contract_test.rs eventcore-postgres/tests/projection_continuous_test.rs
nix develop -c git commit -S -m "feat(projections): fence leaders and drain delivery"
```

---

### Task 7: Coordinated Reset and Replay

**Files:**

- Create: `eventcore-postgres/src/projections/reset.rs`
- Create: `eventcore-postgres/tests/projection_reset_test.rs`
- Modify: `eventcore-postgres/src/projections/projector.rs`
- Modify: `eventcore-postgres/src/projections/error.rs`
- Modify: `eventcore-postgres/src/projections/mod.rs`
- Modify: `eventcore-testing/src/projection_contract.rs`
- Modify: `eventcore-postgres/tests/transactional_projection_contract_test.rs`

**Interfaces:**

- Produces: `PostgresProjectionReset`, `reset_transactional_projection`, and `reset_and_replay_transactional_projection`.

- [ ] **Step 1: Add RED reset contracts**

Test that reset returns `Busy` while a runner owns leadership, reset callback failure preserves both old model and progress, successful reset clears both atomically, reset-and-replay retains leadership across both phases, and replay reconstructs the exact expected model. Include a legacy UUID-checkpoint adoption test that performs reset/replay instead of translating the UUID.

- [ ] **Step 2: Run RED and obtain independent recovery review**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test projection_reset_test
```

- [ ] **Step 3: Implement reset on the leader connection**

Use this callback shape:

```rust
pub trait PostgresProjectionReset: Send {
    type Error: Error + Send + Sync + 'static;

    fn reset<'a, 'c>(
        &'a mut self,
        tx: &'a mut Transaction<'c, Postgres>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a
    where
        'c: 'a;
}
```

The two public operations have these shapes:

```rust
pub async fn reset_transactional_projection<R>(
    reset: &mut R,
    projector_name: &ProjectorName,
    source_id: &DeliverySourceId,
    selection_id: &ProjectionSelectionId,
    store: &PostgresProjectionStore,
) -> Result<(), ProjectionResetError>
where
    R: PostgresProjectionReset;

pub async fn reset_and_replay_transactional_projection<P, R, S>(
    projector: P,
    reset: &mut R,
    source: &S,
    store: &PostgresProjectionStore,
    config: PostgresProjectionConfig,
) -> Result<ProjectionRunOutcome, ProjectionResetAndReplayError>
where
    P: PostgresProjector,
    R: PostgresProjectionReset,
    S: ProjectionSource;
```

Acquire the same named advisory lock non-blockingly. In one transaction invoke the callback and delete the matching progress row only after validating any existing source/selection identity; commit both or neither. `reset_and_replay_transactional_projection` keeps the internal `ProjectionLeader` value and calls the same internal catch-up loop without reacquiring or releasing leadership between phases.

- [ ] **Step 4: Run GREEN and commit**

```bash
nix develop -c cargo nextest run -p eventcore-postgres --test projection_reset_test
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test
nix develop -c cargo fmt --all
git add eventcore-postgres/src/projections eventcore-postgres/tests/projection_reset_test.rs eventcore-testing/src/projection_contract.rs eventcore-postgres/tests/transactional_projection_contract_test.rs
nix develop -c git commit -S -m "feat(projections): add coordinated reset and replay"
```

---

### Task 8: Facade API, Documentation, Example, and Semver Evidence

**Files:**

- Create: `eventcore-postgres/README.md`
- Create: `eventcore/tests/postgres_transactional_projection_api_test.rs`
- Modify: `eventcore/src/lib.rs`
- Modify: `eventcore-postgres/src/lib.rs`
- Modify: `docs/manual/01-introduction/04-architecture.md`
- Modify: `docs/manual/02-getting-started/04-projections.md`
- Modify: relevant crate `CHANGELOG.md` files

**Interfaces:**

- Produces: `eventcore::postgres::projections::*` facade path and a compilable end-to-end user example.

- [ ] **Step 1: Write a failing facade/API example test**

The test imports only the promised facade path, defines a `PostgresProjector` that updates a table through `&mut Transaction<Postgres>`, creates distinct source/store pools, migrates both, runs batch, and asserts the read model. It must also compile legacy `Projector` plus `run_projection` unchanged.

- [ ] **Step 2: Run RED**

```bash
nix develop -c cargo nextest run -p eventcore --features postgres --test postgres_transactional_projection_api_test
```

- [ ] **Step 3: Add re-exports and user documentation**

Re-export the `eventcore-postgres` crate as the existing `eventcore::postgres` module path and ensure its `projections` module is public. Re-export compatible `sqlx::{Postgres, Transaction}` from `eventcore_postgres::projections` so application signatures do not guess a SQLx version.

Update architecture documentation to remove the existing unqualified “exactly-once” and UUID cross-stream guarantees. Document legacy projections as non-atomic mechanics and transactional PostgreSQL projections as effect-plus-progress exactly once within one read-model database commit. The guide must show source migration, destination migration, stable source/projector/selection IDs, event-type selection, batch and continuous modes, explicit retry/skip/stop/fatal behavior, outbox guidance, reset/replay downtime, mixed databases, and the session-pooling restriction.

- [ ] **Step 4: Run docs/API GREEN and commit**

```bash
nix develop -c cargo nextest run -p eventcore --features postgres --test postgres_transactional_projection_api_test
nix develop -c cargo test --doc --workspace --all-features
nix develop -c cargo fmt --all
git add eventcore/src/lib.rs eventcore-postgres/src/lib.rs eventcore-postgres/README.md eventcore/tests/postgres_transactional_projection_api_test.rs docs/manual eventcore*/CHANGELOG.md
nix develop -c git commit -S -m "docs(projections): document transactional postgres usage"
```

---

### Task 9: Full Verification, CI Partitioning, Independent Review, and Pull Request

**Files:**

- Modify only if measurement requires: `.github/workflows/ci.yml`
- Create: `docs/development-archive/transactional-postgres-projections-release-readiness.md`

**Interfaces:**

- Produces: complete verification evidence, reviewed PR, and exact Foundry version recommendation without publication.

- [ ] **Step 1: Run focused fault suite and measure wall time**

```bash
time nix develop -c cargo nextest run -p eventcore-postgres --test projection_delivery_contract_test --test transactional_projection_contract_test --test projection_continuous_test --test projection_reset_test
```

If ordinary workspace CI would exceed roughly ten minutes, add a cached `postgres-projection-faults` job containing only deterministic connection/commit/leadership fault cases while leaving ordinary contract behavior in the standard test job. Do not weaken or skip either set.

- [ ] **Step 2: Run all local gates from a clean database state**

```bash
nix develop -c cargo fmt --all -- --check
nix develop -c cargo clippy --all-targets --all-features -- -D warnings
nix develop -c cargo nextest run --workspace --all-features
nix develop -c cargo test --doc --workspace --all-features
nix develop -c cargo build --workspace --all-features
nix develop -c cargo audit --db .cargo-advisory-db
```

Expected: every command exits zero; record exact test counts and elapsed time.

- [ ] **Step 3: Run mutation testing for the new state machines**

```bash
nix develop -c cargo mutants -p eventcore-types --file eventcore-types/src/projection_delivery.rs
nix develop -c cargo mutants -p eventcore-postgres --file eventcore-postgres/src/projections/runner.rs --file eventcore-postgres/src/projections/store.rs --file eventcore-postgres/src/projections/reset.rs
```

Expected: zero surviving mutants. Add focused tests for any survivor before continuing.

- [ ] **Step 4: Perform independent final reviews**

Dispatch independent reviewers for architecture conformance, transaction ownership, concurrency/fencing, recovery/fault truthfulness, API compatibility/semver, test quality, and documentation accuracy. Fix every blocking finding with TDD, rerun affected gates, and obtain re-review with no blockers.

- [ ] **Step 5: Write release-readiness report**

The report must map all ten original findings and all sixteen required contracts to implementation files and passing tests, list public API additions and unchanged legacy APIs, report CI/mutation evidence, document migration/operational constraints, and state: “Foundry should consume exact version `=2.1.0` with feature `postgres` after merge and separately approved publication.” It must explicitly state that publication has not occurred and remains unauthorized.

- [ ] **Step 6: Commit final evidence**

```bash
nix develop -c cargo fmt --all
git add .github/workflows/ci.yml docs/development-archive/transactional-postgres-projections-release-readiness.md
nix develop -c git commit -S -m "chore(projections): record release readiness"
```

- [ ] **Step 7: Push and open the pull request**

Push `feat/transactional-postgres-projections` using the repository's SSH remote and ordinary host SSH configuration. Open a PR against `main` whose body lists the ADRs, compatibility decision, migration plan, contract matrix, local evidence, and “No crate publication is authorized by this PR.” Link the active Tiber task.

- [ ] **Step 8: Monitor CI and review through completion**

Wait for every required check and independent review. If CI fails, use the repository CI-recovery workflow to classify and repair or rerun the exact SHA, then obtain green replacement evidence. Address review comments with signed commits and rerun the relevant full gates. The delivery is complete only when the PR is open, green, independently reviewed with no unresolved blockers, and ready for owner merge; do not merge or publish without explicit authorization.
