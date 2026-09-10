# Task 7 RED report: coordinated reset and replay

## Status

`GREEN_READY` — coordinated reset and reset/replay are implemented through the runner-owned
PostgreSQL leader session. All 13 focused reset tests and the full workspace verification gates
pass.

## Files changed

- `eventcore-postgres/src/projections/projector.rs` — public `PostgresProjectionReset` callback
  contract with the runner-owned SQLx transaction.
- `eventcore-postgres/src/projections/error.rs` — distinct reset and reset/replay failure types,
  including source/selection mismatch, callback, leadership, progress, and indeterminate commit.
- `eventcore-postgres/src/projections/reset.rs` — coordinated reset transaction, identity
  validation, typed failure mapping, and same-leader reset/replay orchestration.
- `eventcore-postgres/src/projections/runner.rs` — shared catch-up entrypoint accepting an already
  owned leader while preserving the existing public runner behavior.
- `eventcore-postgres/src/projections/store.rs` — progress deletion through the caller's exact
  runner-owned transaction.
- `eventcore-postgres/src/projections/mod.rs`, `eventcore-postgres/src/lib.rs` — public exports.
- `eventcore-testing/src/projection_contract.rs` — SQLx-free reset/replay fixture vocabulary and
  reusable contracts.
- `eventcore-postgres/tests/projection_reset_test.rs` — public-boundary PostgreSQL fixture and
  reset/replay contract tests.

## Nested TDD evidence

1. The first API compile failed with unresolved public imports for the callback trait, two
   operations, and two error types.
2. After adding only the compileable surface plus real non-blocking leader acquisition,
   `cargo check -p eventcore-postgres --test projection_reset_test` passed.
3. The initial callback rollback contract exposed a false-positive: the skeleton's synthetic
   callback error preserved state without invoking the callback. The contract was strengthened to
   require exactly one real callback attempt and the callback's exact public error source.
4. The retained-leadership contract establishes non-empty state first, gates the replay source's
   high-water read, independently requires reset state (`model_total = 0`, no progress) while
   gated, probes a competitor, then releases the source. This prevents a gate placed before reset
   from satisfying the test.
5. Independent review found the first RED insufficiently discriminating around progress-delete
   rollback, identity evidence, release/reacquire gaps, task cleanup, and legacy model state. The
   revised RED adds a real PostgreSQL DELETE trigger fault, preserves every public mismatch field,
   queues an exact-key advisory waiter before callback completion, converges all spawned paths
   through cleanup, and seeds legacy model state to `9999`.

## Final focused RED

Command:

```text
nix develop --command cargo nextest run -p eventcore-postgres --test projection_reset_test --no-fail-fast
```

Result on two consecutive final-cleanup runs: 13 tests run; 5 passed and 8 failed each time.

Passed:

- `coordinated_reset_api_is_public`
- `reset_is_busy_while_runner_owns_leadership` (real runner is gated inside `apply`; the basic
  reset returns typed `Busy` through the real named lock)
- `completed_error_and_panicked_handles_are_marked_joined_before_propagation`
- `cleanup_timeout_is_a_deterministic_outcome`
- `waiter_cleanup_wakes_and_joins_the_owned_task`

Intended failures:

- failed callback: got the skeleton callback source instead of the real mutating callback source;
- progress DELETE failure: got the skeleton callback failure instead of typed `Progress`; the
  GREEN implementation must run the successful callback first and roll it back with the failed
  deletion;
- successful reset: got callback failure instead of `Completed`;
- source mismatch: got callback failure instead of `SourceIdentityMismatch`;
- selection mismatch: got callback failure instead of `SelectionIdentityMismatch`;
- exact replay: reset/replay returned the skeleton reset failure;
- legacy UUID adoption: reset/replay returned the skeleton reset failure;
- inter-phase fencing: timed out waiting for the controlled high-water gate because replay never
  started.

Every async gate/task uses a three-second bound. Both runner-overlap and reset/replay orchestration
route timeout, observation error, competitor error, normal completion, and panic through gate
release, abort-if-live, bounded join, and waiter shutdown before propagating the outcome. Every
contract uses a UUID-derived schema/projector, catches panics, and drops the schema before resuming
the panic.

The orchestrator join helper marks a completed `JoinHandle` consumed before propagating either its
nested task result or `JoinError`, so unconditional cleanup cannot repoll a completed handle. State
and waiter database reads after the replay gate are individually bounded. The outer contract
wrapper also bounds pool close/schema drop, reports cleanup timeout/failure alongside a normal
contract error, and preserves the original panic payload if cleanup independently fails, times
out, or panics.

Three focused fixture regressions verify that nested task `Err` and panicked handles are marked
joined before propagation, a pending cleanup becomes deterministic `TimedOut`, and waiter cleanup
wakes, joins, removes its owned task, then permits bounded schema cleanup.

## GREEN implementation and focused evidence

- Reset acquires the same named non-blocking leader used by the runner, begins a transaction on
  that session, locks existing progress, and validates source identity before selection identity.
- The callback receives the exact mutable transaction. Successful callback mutation and matching
  progress deletion commit once; load/callback/delete failures explicitly roll back where
  possible. A rollback/session failure maps to `LeadershipLost`, while a commit failure maps to
  `CommitIndeterminate` without a false rollback claim.
- Reset/replay acquires leadership once, commits reset, then enters the existing catch-up loop with
  the same owned `ProjectionLeader`. The retained-leadership fixture proves the queued exact-key
  waiter never acquires between phases, the public competitor remains excluded, and the reset and
  replay callbacks observe the same backend PID.
- Legacy UUID checkpoint adoption rebuilds the stale model through reset/replay and leaves the
  opaque UUID checkpoint unchanged; no cursor translation or shadow generation is used.
- `cargo nextest run -p eventcore-postgres --test projection_reset_test --no-fail-fast`: 13/13
  passed.

## Regression and static evidence

- `cargo nextest run -p eventcore-testing`: 43/43 passed.
- `cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test`: 36/36
  passed.
- `cargo nextest run -p eventcore-postgres --test projection_continuous_test`: 4/4 passed.
- `cargo nextest run -p eventcore-postgres --test projection_delivery_contract_test`: 18/18
  passed.
- `cargo fmt --all`: passed.
- `cargo build --workspace`: passed.
- `cargo nextest run --workspace`: 415/415 passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo doc --workspace --no-deps`: passed.
- `cargo audit --db /home/jwilger/projects/eventcore/.cargo-advisory-db`: passed with the five
  repository-allowed warnings.
- `git diff --check`: passed.

## Contract details for review

- Callback mutation and progress deletion are asserted only through state read on an independent
  pooled connection; the callback receives only `&mut Transaction<'c, Postgres>`.
- The progress-deletion fault is a real `BEFORE DELETE` trigger in an isolated schema. It starts
  from exact model `68` and matching progress, lets the successful callback clear through its
  supplied transaction, then requires typed `Progress` plus exact old model/progress. Splitting
  callback mutation from progress deletion cannot pass.
- Mismatch observations carry the exact projector, persisted identity, and configured identity
  forwarded by the public reset error. Source-only and selection-only contracts preserve exact
  model/progress and require zero additional callback attempts.
- The replay model is a non-idempotent sum of literal values (`2 + 3 + 5 = 10`), and the legacy
  adoption model is `23 + 29 = 52`; expected values are not derived with production helpers.
- The legacy fixture seeds/reads `PostgresCheckpointStore` through the public `CheckpointStore`
  API, overwrites the model with stale value `9999`, and requires exact rebuilt model `52`, exact
  new global progress, and unchanged opaque UUID. Omitting reset cannot pass.
- During the reset callback, the fixture records `pg_backend_pid()`, derives the exact already-held
  advisory key from that backend's granted `pg_locks` row (without copying production hashing),
  and starts a dedicated blocking waiter. It DB-observes the waiter in PostgreSQL `Lock` wait
  before the callback may finish. At the post-reset high-water gate it requires the same waiter to
  remain queued, probes the public competing runner for `LeadershipBusy`, and later requires the
  replay projector's backend PID to equal the reset callback PID. PostgreSQL lock queue ordering
  makes a release/reacquire gap fail deterministically.
- ADR-0050's reset section and Task 7 checklist do not assign a reset-specific lost-commit-
  acknowledgement injection test. The public reset error still distinguishes
  `CommitIndeterminate`, so GREEN cannot truthfully map a commit error to rollback/callback failure.

## Historical RED boundary

Before implementation, the final approved RED ran 13 tests with the five fixture/API tests passing
and all eight absent reset/replay behaviors failing for their intended reasons. The unchanged
contracts now pass against the coordinated implementation described above.
