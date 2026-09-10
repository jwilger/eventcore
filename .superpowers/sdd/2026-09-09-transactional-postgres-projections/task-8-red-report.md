# Task 8 report: facade API, documentation, and isolated-pool example

## Files changed

- `eventcore/tests/postgres_transactional_projection_api_test.rs` — consumer-facing PostgreSQL
  projection example and legacy API compatibility exercise.
- `eventcore/Cargo.toml` and `Cargo.lock` — compatible SQLx consumer-test dependency added with
  Cargo CLI and resolved exactly to SQLx 0.8.6 by `Cargo.lock`, leaving SQLx pool/query helpers
  outside the promised facade contract.

## Breaks caught

1. A consumer of the `eventcore` facade cannot currently name the delivery vocabulary used by
   `PostgresProjectionConfig` and `PostgresProjector` without adding/importing
   `eventcore-types` directly.
2. A consumer cannot name the SQLx `Postgres`/`Transaction` pair required by the public
   `PostgresProjector::apply` signature without guessing and adding a compatible SQLx version.
3. The runtime half of the test uses separately configured source and destination pools with
   different isolated schemas in one PostgreSQL database. Independent table-presence assertions
   plus source reads and destination writes make collapsing or swapping those pool arguments
   observable without claiming separate physical databases.
4. The example appends through EventCore's public `execute` command path; it does not use raw
   event inserts or implementation-private hooks. It asserts the exact read model, outcome, and
   durable progress.
5. The same consumer target implements and invokes the unchanged legacy `Projector` and
   `run_projection` API against the public PostgreSQL backend.

The test imports `PgPool`, `PgPoolOptions`, `query`, and `query_scalar` directly from its
compatible SQLx dev-dependency, resolved exactly to 0.8.6 by `Cargo.lock`. Those are deliberately
not part of the facade requirement. Only the compatible `Postgres` and `Transaction` signature
types must come from the facade.

Dependency command:

```text
nix develop -c cargo add sqlx@0.8.6 --package eventcore --dev --no-default-features --features runtime-tokio-rustls,postgres
```

## RED evidence

Command:

```text
nix develop -c cargo nextest run -p eventcore --features postgres --test postgres_transactional_projection_api_test
```

Result: compilation failed with exit code 101 before PostgreSQL setup. Rust reported 12 missing
facade uses comprising `Postgres`, `Transaction`, and the projection vocabulary
`DeliveryPosition`, `DeliverySourceId`, `EventTypeName`, `ProjectorName`,
`ProjectionSelection`, `ProjectionSelectionId`, and `ProjectionStreamFilter`. This is the
intended public-surface failure; SQLx setup/query helpers resolved from the test dependency.

The complete test was also compile-checked successfully using temporary direct imports for only
those missing names, then restored to facade-only imports. This proves the remaining compilation
failure is confined to the intended facade exports rather than test scaffolding.

The file is gated with `#![cfg(feature = "postgres")]`. The isolated default-feature command

```text
nix develop -c cargo check -p eventcore --test postgres_transactional_projection_api_test
```

completed successfully, so the new integration target does not break default-feature consumers.

Formatting was independently checked by running `nix develop -c cargo fmt --all`.

## Legacy baseline evidence

Command:

```text
nix develop -c cargo nextest run -p eventcore --test mixed_event_type_projection_test
```

Result: 2 tests passed, including
`run_projection_processes_events_when_other_event_types_exist`. This separately proves the
pre-existing legacy trait/runner still compile and execute before the facade GREEN change.

## First GREEN edits

1. In `eventcore-postgres::projections`, publicly re-export the delivery contract types used to
   configure and implement transactional projections.
2. From the same module, publicly re-export only SQLx's compatible `Postgres` and `Transaction`
   names so projector signatures never guess an SQLx version. Pool and query helpers remain the
   application's direct SQLx responsibility.
3. Re-run the target to expose any runtime contract issue, implementing no behavior beyond what
   this already-complete transactional runner requires.

## Mutation check

- Removing a facade export makes the target fail to compile.
- Pointing both runner arguments at the source pool makes destination migration/model access
  fail; pointing both at the destination pool makes source delivery fail.
- Removing the projector's effect changes the exact model assertion.
- Omitting progress advancement changes the exact progress assertion.
- Breaking the legacy API makes its implementation or invocation fail to compile, and breaking
  legacy delivery changes the atomic total assertion.
- Setup failures after schema creation attempt bounded partial cleanup. Destination setup failure
  also cleans the source. Runtime errors, timeout, and assertion panics all flow through cleanup
  attempts for both schemas, with both cleanup outcomes retained alongside the original failure.

## GREEN implementation

- `eventcore-postgres::projections` now re-exports the complete new delivery vocabulary and only
  SQLx's compatible `Postgres` and `Transaction` signature types. It does not re-export SQLx pool
  options or query helpers.
- The EventCore facade continues to expose the unchanged `eventcore::postgres` crate alias, so
  the compiled consumer path is `eventcore::postgres::projections::*`.
- `eventcore-postgres/README.md`, the architecture overview, projection guide, and crate-level
  documentation now distinguish legacy non-atomic checkpoints from acknowledged atomic
  effect-plus-progress commits in one PostgreSQL read-model database.
- Documentation covers separate source/destination migrations and pools, stable identities,
  event selection and decode failures, batch/continuous modes, bounded Retry/Skip/Stop/Fatal
  decisions, after-commit/outbox behavior, indeterminate commits, named restart progress,
  session-lock pooling constraints, and coordinated reset/replay without shadow generations.
- `eventcore`, `eventcore-postgres`, `eventcore-types`, and `eventcore-testing` changelogs record
  the additive 2.1.0 surface and legacy 2.0.1 source compatibility. Documentation states that no
  crate has been published and that Foundry should use
  `eventcore = { version = "=2.1.0", features = ["postgres"] }` only after explicitly approved
  publication.

## Fresh GREEN evidence

```text
nix develop -c cargo fmt --all -- --check
# exit 0

nix develop -c cargo nextest run -p eventcore --features postgres --test postgres_transactional_projection_api_test
# 1 passed, 0 failed

nix develop -c cargo check -p eventcore --test postgres_transactional_projection_api_test
# exit 0 with the postgres-only target cfg-disabled

nix develop -c cargo nextest run -p eventcore --test mixed_event_type_projection_test
# 2 passed, 0 failed, including the legacy run_projection scenario

nix develop -c cargo test --doc --workspace --all-features
# all workspace doctests passed (35 passed, 18 ignored)

nix develop -c cargo clippy --all-targets --all-features -- -D warnings
# exit 0, no warnings
```

The repository's pre-commit `prettier` hook formatted the four staged changelogs on its first
pass. Those hook edits were retained and restaged for the successful commit pass.

GREEN_READY

## Post-GREEN documentation review fixes

- Replaced the unconditional intra-doc link to the feature-gated PostgreSQL facade with a plain
  code path; default/no-feature rustdoc now succeeds with warnings denied.
- Documented the source migration's `ACCESS EXCLUSIVE` lock, full-history backfill, maintenance
  impact, deterministic historical `(stream_id, stream_version, event_id)` ordering, and the
  separate commit-safe frontier used by subsequent appends.
- Clarified that caller-supplied IDs are the only semantic binding. Selection/source semantic
  changes require new IDs, while reset validation must first use the old IDs persisted in
  progress. A new projector name is unsafe for a populated non-idempotent model unless the model
  is separately new or empty.
- Documented that database rollback does not restore projector `&mut self` fields and that
  retry-sensitive state belongs in the supplied transaction or must be explicitly rollback-safe.
- Documented cooperative cancellation after a bounded catch-up cycle/during the idle select; it
  does not interrupt apply, retry delay, commit, or after-commit work.
- Corrected the explicitly post-publication Foundry recommendation to
  `eventcore = { version = "=2.1.0", features = ["postgres"] }` while retaining the unpublished
  and approval-gated warning.

Fresh review-fix evidence:

```text
nix develop -c cargo fmt --all
nix develop -c pre-commit run prettier --files docs/manual/02-getting-started/04-projections.md eventcore-postgres/README.md .superpowers/sdd/2026-09-09-transactional-postgres-projections/task-8-red-report.md
# prettier passed

nix develop -c cargo check -p eventcore --test postgres_transactional_projection_api_test
nix develop -c env RUSTDOCFLAGS=-Dwarnings cargo doc -p eventcore --no-default-features --no-deps
# both exit 0

nix develop -c cargo nextest run -p eventcore --features postgres --test postgres_transactional_projection_api_test
# 1 passed, 0 failed

nix develop -c cargo nextest run -p eventcore --test mixed_event_type_projection_test
# 2 passed, 0 failed

nix develop -c cargo test --doc --workspace --all-features
# all workspace doctests passed (35 passed, 18 ignored)

nix develop -c env RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --all-features --no-deps
nix develop -c cargo clippy --all-targets --all-features -- -D warnings
# both exit 0 with no warnings
```

REVIEW_FIX_GREEN
