# Transactional PostgreSQL projections release readiness

## Decision and scope

The committed implementation on branch `feat/transactional-postgres-projections` at
`c79be41a0d04aab83cae92cb22d5e4478c5f18f4` is locally ready for delivery as an additive,
lockstep EventCore 2.1.0 release candidate. This conclusion covers the source, public contracts,
PostgreSQL integration behavior, documentation, and local verification evidence at that exact
commit. The release-readiness report itself is a post-verification documentation artifact and is
not part of that source SHA.

The reviewed and verified commit range is
`68c38b469b3b8b300cf4ca6400cb9abe8187bce9..c79be41a0d04aab83cae92cb22d5e4478c5f18f4`.

Remote delivery is not complete. The branch has not yet been pushed for this delivery, no pull
request URL is claimed here, actual remote pull-request checks and hosted review state are not yet
available, and the result must not be described as remotely green. Merge and publication both
remain unauthorized. The seven reviews recorded below are independent pre-PR reviews, not hosted
pull-request reviews.

This report was checked against the original request, the committed source and tests, and the
controller's staged Task 9 verification and projection-fixture-lifecycle evidence. Those two
controller artifacts were consulted as ephemeral evidence and are deliberately not linked as
repository files.

## Original EventCore 2.0.1 finding disposition

The released legacy path remains source-compatible, including its pre-existing semantics. The
new facility resolves the findings on a separate transactional PostgreSQL path; it does not
silently change what an existing 2.0.1 `Projector` or `run_projection` call means.

|   # | Original finding                                                                                                                                                                                           | Disposition and implementation                                                                                                                                                                                                                                                          | Passing public-boundary evidence                                                                                                                                                                                                                                                                                                                          |
| --: | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|   1 | **Projector::apply is synchronous.**                                                                                                                                                                       | Confirmed for the retained legacy API. The additive `PostgresProjector::apply` is asynchronous and receives the runner-owned `&mut Transaction<'_, Postgres>` in `eventcore-postgres/src/projections/projector.rs`; execution is in `eventcore-postgres/src/projections/runner.rs`.     | `effect_and_progress_commit_atomically` in `eventcore-postgres/tests/transactional_projection_contract_test.rs`; `facade_runs_transactional_projection_across_distinct_pools_and_schemas` in `eventcore/tests/postgres_transactional_projection_api_test.rs`.                                                                                             |
|   2 | **The projection runner applies an event and saves its checkpoint as separate operations.**                                                                                                                | Confirmed for the retained legacy runner. `process_envelope` applies the mutation, advances progress, and commits one transaction in `eventcore-postgres/src/projections/runner.rs`.                                                                                                    | `effect_and_progress_commit_atomically`, `application_mutation_failure_rolls_back_effect_and_progress`, and `progress_failure_rolls_back_effect_and_progress` in `eventcore-postgres/tests/transactional_projection_contract_test.rs`.                                                                                                                    |
|   3 | **PostgreSQL checkpoint saving uses its own pool rather than the transaction containing application read-model changes.**                                                                                  | Confirmed for the retained legacy store. `ProjectionLeader` in `eventcore-postgres/src/projections/store.rs` owns the one destination connection; its transactions are passed to both application code and progress operations. Source and destination pools remain separate by design. | `facade_runs_transactional_projection_across_distinct_pools_and_schemas` in `eventcore/tests/postgres_transactional_projection_api_test.rs`; `effect_and_progress_commit_atomically` in `eventcore-postgres/tests/transactional_projection_contract_test.rs`.                                                                                             |
|   4 | **Checkpoint-save failure can be logged while processing continues.**                                                                                                                                      | Confirmed in the retained legacy pipeline. The new runner returns positioned `TransactionalProjectionError::Progress`, rolls the transaction back, and stops; see `eventcore-postgres/src/projections/runner.rs`, `store.rs`, and `error.rs`.                                           | `progress_failure_rolls_back_effect_and_progress` and `failure_context_and_retry_exhaustion_retain_exact_position_attempt_and_error` in `eventcore-postgres/tests/transactional_projection_contract_test.rs`.                                                                                                                                             |
|   5 | **PostgreSQL EventReader advances with `event_id > cursor ORDER BY event_id`, but UUID event IDs are allocated before transaction commit and therefore are not a safe cross-transaction commit frontier.** | Confirmed for legacy `EventReader`. The projection source uses the transactional frontier, immutable event-to-position mapping, and trigger in `eventcore-postgres/projection-migrations/0001_delivery_source.sql`, read by `eventcore-postgres/src/projections/source.rs`.             | `lower_uuid_committed_after_a_consumed_higher_uuid_receives_a_later_delivery_position` and `concurrent_transactions_deliver_every_committed_event_without_waiting_for_blocked_insert` in `eventcore-postgres/tests/projection_delivery_contract_test.rs`.                                                                                                 |
|   6 | **PostgreSQL EventReader silently discards deserialization failures through filter_map/ok behavior.**                                                                                                      | Confirmed for legacy `EventReader`. `PostgresProjectionSource` constructs a complete opaque envelope or returns an explicit error, and `PostgresProjector::decode` fails terminally without progress; see `source.rs`, `projector.rs`, and `runner.rs`.                                 | `malformed_selected_input_returns_decode_and_does_not_advance` in `eventcore-postgres/tests/transactional_projection_contract_test.rs`; `storage_valid_but_application_malformed_payload_remains_visible_as_raw_json` and `delivered_envelopes_preserve_every_public_persisted_field` in `eventcore-postgres/tests/projection_delivery_contract_test.rs`. |
|   7 | **Batch mode appears to stop after one fixed-size page rather than draining all available events.**                                                                                                        | Confirmed for the retained legacy batch pipeline. The new `drain_cycle` loops over bounded selected pages through one captured high-water mark in `eventcore-postgres/src/projections/runner.rs`.                                                                                       | `batch_drains_more_than_one_page`, `batch_captures_high_watermark_once_despite_concurrent_append`, and `batch_catches_up_through_a_trailing_unselected_frontier` in `eventcore-postgres/tests/transactional_projection_contract_test.rs`.                                                                                                                 |
|   8 | **CheckpointStore has no explicit reset/replay contract.**                                                                                                                                                 | Confirmed for the retained legacy trait. `PostgresProjectionReset`, `reset_transactional_projection`, and `reset_and_replay_transactional_projection` in `projector.rs` and `reset.rs` add a coordinated PostgreSQL contract under the same named leadership.                           | `successful_reset_commits_model_and_progress_deletion`, `reset_and_replay_reconstructs_exact_model`, and `reset_and_replay_retains_leadership_between_phases` in `eventcore-postgres/tests/projection_reset_test.rs`.                                                                                                                                     |
|   9 | **Existing checkpoint resumption suppresses already-checkpointed input, but it cannot guarantee atomic read-model effects plus progress. A crash between those operations can reapply effects.**           | Confirmed for the retained legacy contract. The new destination transaction atomically commits effect and progress, reloads durable progress on restart/retry, and suppresses committed positions in `runner.rs` and `store.rs`.                                                        | `effect_and_progress_commit_atomically`, `restart_resumes_from_last_committed_position`, and `commit_acknowledgement_loss_is_indeterminate_without_after_commit_or_retry` in `eventcore-postgres/tests/transactional_projection_contract_test.rs`.                                                                                                        |
|  10 | **Existing projector coordination is useful, but its leadership-loss and fencing guarantees need to be made explicit and tested.**                                                                         | Addressed additively. `PostgresProjectionStore::acquire_leader` and `ProjectionLeader` use a session advisory lock on the same `close_on_drop` physical destination connection that owns all transactions; loss of that backend fences stale work.                                      | `leadership_rejects_overlapping_second_writer_while_leader_is_active` and `leadership_loss_of_exact_backend_fences_all_stale_writes` in `eventcore-postgres/tests/transactional_projection_contract_test.rs`; `reset_is_busy_while_runner_owns_leadership` in `eventcore-postgres/tests/projection_reset_test.rs`.                                        |

## Required behavioral contract matrix

The reusable assertions live in `eventcore-testing/src/projection_contract.rs`. PostgreSQL invokes
them through public adapter tests; raw PostgreSQL controls are confined to deterministic fault
injection and observation. In the matrix, unqualified production names such as `runner.rs` mean
`eventcore-postgres/src/projections/<name>`, and unqualified integration-test names mean the
`eventcore-postgres/tests/` file named in that cell.

|   # | Required contract                                                                                         | Implementation                                                                                                                                                             | Exact passing test(s)                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| --: | --------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|   1 | Read-model mutation and checkpoint commit atomically.                                                     | `eventcore-postgres/src/projections/runner.rs`, `store.rs`                                                                                                                 | Shared `transactional_projection_contract`; adapter `effect_and_progress_commit_atomically` in `eventcore-postgres/tests/transactional_projection_contract_test.rs`.                                                                                                                                                                                                                                                                                  |
|   2 | Mutation failure rolls back checkpoint.                                                                   | `runner.rs`, `projector.rs`, `error.rs`                                                                                                                                    | Shared `mutation_failure_rolls_back_contract`; adapter `application_mutation_failure_rolls_back_effect_and_progress` in `transactional_projection_contract_test.rs`.                                                                                                                                                                                                                                                                                  |
|   3 | Checkpoint failure rolls back mutation.                                                                   | `runner.rs`, `store.rs`, `error.rs`                                                                                                                                        | Shared `progress_failure_rolls_back_contract`; adapter `progress_failure_rolls_back_effect_and_progress` in `transactional_projection_contract_test.rs`.                                                                                                                                                                                                                                                                                              |
|   4 | Connection loss or interrupted commit produces a truthful, recoverable outcome.                           | Positioned `CommitIndeterminate` handling in `runner.rs` and reset-specific `CommitIndeterminate` in `reset.rs`/`error.rs`.                                                | Shared `commit_acknowledgement_loss_contract`; adapter `commit_acknowledgement_loss_is_indeterminate_without_after_commit_or_retry` in `transactional_projection_contract_test.rs`. The true reset commit/lost-ACK branch is `committed_reset_with_lost_acknowledgement_is_indeterminate` in `eventcore-postgres/tests/projection_reset_test.rs`. Both faults drop the acknowledgement only after an independent connection observes committed state. |
|   5 | Restart resumes from the last committed position.                                                         | Durable identity-bound progress load in `runner.rs` and `store.rs`.                                                                                                        | Shared `restart_resumes_from_committed_position_contract`; adapter `restart_resumes_from_last_committed_position` in `transactional_projection_contract_test.rs`.                                                                                                                                                                                                                                                                                     |
|   6 | Redelivery does not duplicate already-committed effects.                                                  | `position_is_pending` and per-attempt durable progress reload in `runner.rs`.                                                                                              | Shared `transactional_projection_contract`; adapter `effect_and_progress_commit_atomically` (its second run proves suppression). The lost-ACK recovery branch is also proved by `commit_acknowledgement_loss_is_indeterminate_without_after_commit_or_retry`.                                                                                                                                                                                         |
|   7 | Concurrent event-store transactions cannot be skipped because identifier order differs from commit order. | `0001_delivery_source.sql` transactional frontier/mapping/trigger and `source.rs`.                                                                                         | `lower_uuid_committed_after_a_consumed_higher_uuid_receives_a_later_delivery_position` and `concurrent_transactions_deliver_every_committed_event_without_waiting_for_blocked_insert` in `eventcore-postgres/tests/projection_delivery_contract_test.rs`.                                                                                                                                                                                             |
|   8 | Malformed persisted events stop processing without checkpoint advancement.                                | Strict envelope conversion in `source.rs`; application decode boundary in `projector.rs`; positioned terminal error in `runner.rs`.                                        | Shared `malformed_selected_input_contract`; adapter `malformed_selected_input_returns_decode_and_does_not_advance` in `transactional_projection_contract_test.rs`; raw visibility in `storage_valid_but_application_malformed_payload_remains_visible_as_raw_json` in `projection_delivery_contract_test.rs`.                                                                                                                                         |
|   9 | Leadership excludes a second writer.                                                                      | Session advisory leadership in `store.rs`.                                                                                                                                 | Shared `overlapping_leadership_contract`; adapter `leadership_rejects_overlapping_second_writer_while_leader_is_active` in `transactional_projection_contract_test.rs`.                                                                                                                                                                                                                                                                               |
|  10 | Leadership loss prevents continued unfenced writes.                                                       | Leader-owned physical connection, transaction ownership, rollback, and `close_on_drop` in `store.rs`/`runner.rs`.                                                          | Shared `leadership_loss_fencing_contract`; adapter `leadership_loss_of_exact_backend_fences_all_stale_writes` in `transactional_projection_contract_test.rs`.                                                                                                                                                                                                                                                                                         |
|  11 | Batch mode drains more than one page.                                                                     | Bounded `drain_cycle` loop in `runner.rs`.                                                                                                                                 | Shared `multi_page_batch_drain_contract`; adapter `batch_drains_more_than_one_page` in `transactional_projection_contract_test.rs`. Boundary variants `batch_catches_up_when_source_is_initially_empty`, `batch_catches_up_when_selection_matches_nothing`, and `batch_catches_up_through_a_trailing_unselected_frontier` also pass.                                                                                                                  |
|  12 | Continuous mode catches events committed after initial catch-up.                                          | Repeated bounded cycles and idle-only `tokio::select!` wait in `runner.rs`; cancellation token in `config.rs`.                                                             | Shared `continuous_delivery_contract` and `continuous_idle_cancellation_contract`; adapters `continuous_mode_delivers_an_event_appended_after_initial_catch_up` and `continuous_mode_awaits_one_positive_idle_poll_and_cancels_normally` in `eventcore-postgres/tests/projection_continuous_test.rs`.                                                                                                                                                 |
|  13 | Retry exhaustion is bounded and reported.                                                                 | Typed decisions, one-based `AttemptNumber`, bounded/capped policy, fresh transactions, and positioned sources in `config.rs`, `projector.rs`, `runner.rs`, and `error.rs`. | Shared `retry_exhaustion_contract`; adapter `retry_exhaustion_is_bounded_and_rolls_back_every_attempt` in `transactional_projection_contract_test.rs`. `failure_context_and_retry_exhaustion_retain_exact_position_attempt_and_error`, `retry_backoff_grows_exponentially_then_caps_at_the_maximum`, and `retry_backoff_saturates_finite_multiplier_overflow_without_panicking` cover exact context and backoff.                                      |
|  14 | Skip behavior advances only when explicitly selected.                                                     | `ProjectionFailureDecision::Skip` and progress-only transaction in `projector.rs`/`runner.rs`.                                                                             | Shared `explicit_skip_contract`; adapter `skip_rolls_back_application_work_and_advances_only_progress` in `transactional_projection_contract_test.rs`. `stop_leaves_exact_position_pending_with_prior_counts` and `fatal_returns_application_failure_without_progress_or_hook` prove non-skip decisions do not advance.                                                                                                                               |
|  15 | Reset followed by replay reconstructs the expected state.                                                 | Same-leader reset/replay in `reset.rs`; application reset transaction in `projector.rs`; progress delete in `store.rs`.                                                    | Shared `reset_and_replay_reconstructs_model_contract`; adapter `reset_and_replay_reconstructs_exact_model` in `eventcore-postgres/tests/projection_reset_test.rs`. `reset_and_replay_retains_leadership_between_phases`, `legacy_uuid_checkpoint_is_adopted_through_reset_replay`, and `committed_reset_with_lost_acknowledgement_is_indeterminate` cover fencing, legacy adoption, and true reset lost-ACK recovery.                                 |
|  16 | Notifications/after-commit callbacks occur only after successful commit.                                  | `AfterCommit` value returned by `apply` and executed only after acknowledged `commit` in `projector.rs`/`runner.rs`.                                                       | Shared `after_commit_ordering_and_rollback_contract` and `after_commit_failure_contract`; adapters `after_commit_observes_committed_state_and_is_suppressed_on_rollback` and `after_commit_failure_reports_committed_position_without_replay` in `transactional_projection_contract_test.rs`. The apply lost-ACK test also proves no hook runs without acknowledgement.                                                                               |

### Additional public-boundary hardening

- Envelope fidelity is tested by `persisted_envelope_exposes_exact_fields_and_preserves_raw_json`
  in `eventcore-types/src/projection_delivery.test.rs`, plus
  `delivered_envelopes_preserve_every_public_persisted_field` and
  `source_preserves_precise_jsonb_numbers_in_payload_and_metadata` in
  `eventcore-postgres/tests/projection_delivery_contract_test.rs`. Application-owned envelope
  routing and discriminator rejection are tested by
  `projector_decode_routes_by_persisted_event_type_and_rejects_unsupported_discriminator` in
  `eventcore-postgres/tests/transactional_projection_contract_test.rs`.
- Exact failure context is tested by
  `failure_context_and_retry_exhaustion_retain_exact_position_attempt_and_error`, covering the
  delivery position, one-based attempt, and original application error. Typed after-commit source
  forwarding is covered by `after_commit_error_classifier_forwards_distinct_sources`, both in
  `transactional_projection_contract_test.rs`.
- A zero transactional batch is rejected before execution by
  `transactional_projection_config_rejects_zero_batch_size` in
  `transactional_projection_contract_test.rs`; the implementation is
  `PostgresProjectionConfig::with_batch_size` in `eventcore-postgres/src/projections/config.rs`.
- Fixture lifecycle cleanup is bounded across initialization, body, cleanup, task ownership, and
  two-resource cases. The seven helper tests in
  `eventcore-postgres/tests/common/fixture_lifecycle.rs` are
  `initialization_error_after_resource_ownership_still_cleans_up`,
  `initialization_panic_after_resource_ownership_still_cleans_up`,
  `initialization_timeout_after_resource_ownership_still_cleans_up`,
  `body_panic_and_timeout_each_clean_up`,
  `cleanup_failure_does_not_replace_primary_failure`,
  `two_owned_resources_are_both_cleaned_after_partial_setup`, and
  `pending_first_cleanup_does_not_prevent_observable_second_cleanup`. PostgreSQL ownership is
  additionally observed by `failed_delivery_initialization_leaves_no_owned_schema`,
  `cancelling_transactional_proxy_task_closes_frontend_connection`,
  `dropping_continuous_runner_owner_cancels_its_task`,
  `dropping_lock_waiter_owner_terminates_task_and_releases_database_connection`, and
  `cancelling_proxy_task_closes_frontend_connection` in the four focused integration-test files.

## Public API and compatibility

### Additions by crate and facade

- `eventcore-types` adds the SQLx-free delivery vocabulary and contract in
  `eventcore-types/src/projection_delivery.rs`: `DeliveryPosition`, `DeliverySourceId`,
  `ProjectorName`, `ProjectionSelectionId`, `PersistedEventId`, `EventTypeName`,
  `DeliveryUpperBound`, `ProjectionStreamFilter`, `ProjectionSelection`,
  `PersistedEventEnvelope`, `ProjectionSource`, and their validation errors. They are re-exported
  from `eventcore-types/src/lib.rs`.
- `eventcore-postgres::projections` adds `PostgresProjectionSource`,
  `PostgresProjectionSourceError`, `PostgresProjectionStore`, `ProjectionProgress`,
  `PostgresProjectionConfig`, `PostgresProjectionMode`, `ProjectionRetryPolicy`,
  `ProjectionConfigurationError`, `ProjectionRetrySleeper`, `ProjectionPollSleeper`,
  `TokioProjectionRetrySleeper`, `TokioProjectionPollSleeper`, `PostgresProjector`,
  `PostgresProjectionReset`, `AfterCommit`, `NoopAfterCommit`, `ProjectionFailureContext`,
  `ProjectionFailureDecision`, `ProjectionRunOutcome`, `BoxedProjectionError`,
  `TransactionalProjectionError`, `ProjectionResetError`, `ProjectionResetAndReplayError`, and
  the three free functions `run_transactional_projection`, `reset_transactional_projection`, and
  `reset_and_replay_transactional_projection`. The module also re-exports the delivery vocabulary
  and matching `sqlx::Postgres` and `sqlx::Transaction` types so downstream projector signatures
  use EventCore's SQLx version.
- `eventcore-testing::projection_contract` adds the SQLx-free fixture traits
  `TransactionalProjectionFixture`, `TransactionalProjectionExecutionFixture`,
  `TransactionalProjectionContinuousFixture`, and `TransactionalProjectionResetFixture`; typed
  application behavior, attempt/failure/run/reset observations, progress, leadership,
  high-water-mark, continuous-idle, and reset-state observations; and reusable public assertion
  functions for every contract named in the 16-row matrix, including the reset, identity,
  empty/no-match, after-commit failure, retry backoff, and fixed-frontier variants. All are
  re-exported from `eventcore-testing/src/lib.rs`.
- With feature `postgres`, the main `eventcore` facade exposes the complete adapter at
  `eventcore::postgres::projections`. The compiled public-facade proof is
  `eventcore/tests/postgres_transactional_projection_api_test.rs`.

### Compatibility and semver decision

The legacy `Projector`, `run_projection`, `EventReader`, `CheckpointStore`,
`ProjectorCoordinator`, UUID-backed `StreamPosition`, `PostgresCheckpointStore`,
`PostgresProjectorCoordinator`, their batch/continuous modes and failure strategies, and the
legacy PostgreSQL event/checkpoint tables remain unchanged. Existing applications receive none of
the new transactional semantics unless they opt into the new API.

`eventcore-types` and `eventcore-testing` remain SQLx-free; SQLx appears only in the PostgreSQL
adapter boundary where the application must share a concrete destination transaction. The default
`PostgresProjector::decode` retains payload-only `serde_json` decoding, so a simple existing
payload-shaped projector need not override it; envelope-aware multi-type projectors may override
it to inspect type, metadata, stream, and identity fields.

Exactly nine new extensible error/control enums are marked `#[non_exhaustive]`:
`ProjectionSelectionError`, `PostgresProjectionSourceError`, `PostgresProjectionMode`,
`ProjectionConfigurationError`, `ProjectionFailureDecision`, `ProjectionRunOutcome`,
`TransactionalProjectionError`, `ProjectionResetError`, and `ProjectionResetAndReplayError`.
This permits compatible future variants without requiring downstream exhaustive matching.

The change is additive and therefore targets the next lockstep minor release, 2.1.0, rather than a
breaking 3.0.0. The current manifests still describe the unpublished workspace state; publication
must perform the repository's normal lockstep versioning only after separate authorization.

## Migration and operating constraints

- Projection source and destination migrations use
  `eventcore_projection_schema_versions`, a component/version ledger separate from SQLx's
  `_sqlx_migrations`. `delivery-source` version 1 and `projection-destination` version 2 can be
  applied independently. `projection_migration_keeps_the_legacy_event_store_migrator_compatible`
  proves that the legacy migrator still runs afterward.
- The first source migration takes `ACCESS EXCLUSIVE` on `eventcore_events`, backfills the entire
  history in deterministic `(stream_id, stream_version, event_id)` order, installs the immutable
  event-to-delivery-position mapping, transactional frontier, and statement trigger, and can block
  reads/writes. It requires measured maintenance downtime for a large store. Historical backfill
  order is deterministic, not reconstructed commit order; new writes receive commit-safe frontier
  positions inside their event-store transaction.
- Source and destination are separate arguments and may be distinct pools, schemas, or PostgreSQL
  databases. The source owns delivery ordering; the destination owns the live read model,
  projector progress, and leadership. `DeliverySourceId` and `ProjectionSelectionId` bind the
  persisted ordering and selection semantics, and `ProjectorName` binds the destination model.
  Change an identity only when its meaning changes and use the documented reset protocol.
- Leadership is a session advisory lock on the same physical destination connection that owns all
  effect/progress transactions. The connection is marked `close_on_drop`; transaction-pooling
  proxies are unsupported because they cannot preserve session fencing.
- An acknowledged commit contains both read-model effect and progress. A connection failure while
  awaiting `COMMIT` yields `CommitIndeterminate`; operators must inspect durable progress,
  read-model, and any transactional outbox state, then restart with the same identities. Neither
  apply nor reset automatically retries an indeterminate commit.
- Batch captures one high-water mark and drains all selected pages through it. Continuous mode
  repeats bounded catch-up cycles and waits only after an empty bounded page. Cancellation is an
  idle boundary: it does not interrupt `apply`, retry delay, commit, or after-commit work already
  in progress.
- Reset takes the same named leadership, atomically resets the live model and deletes matching
  progress, and reset-and-replay retains that leadership between phases. Schedule downtime and
  stop other writers. Legacy UUID checkpoints cannot be translated into global delivery
  positions; leave the opaque legacy checkpoint unchanged, reset the live model, choose stable
  transactional identities, and replay. This release intentionally has no shadow generations or
  uninterrupted generation handoff.

The operational source of truth is the [PostgreSQL adapter README](../../eventcore-postgres/README.md),
with architectural context in the [manual architecture chapter](../manual/01-introduction/04-architecture.md)
and usage in the [projection chapter](../manual/02-getting-started/04-projections.md).

## Local verification evidence

All evidence in this section was collected on 2026-09-09 America/Los_Angeles (2026-09-10 UTC)
against committed source at `c79be41a0d04aab83cae92cb22d5e4478c5f18f4`, in the pinned Nix
development environment with PostgreSQL 17.10.

| Gate                                                                   | Exact result                                                                                                                                                                  |
| ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cargo fmt --all -- --check`                                           | Pass; exit 0, no formatting diff.                                                                                                                                             |
| `cargo clippy --all-targets --all-features -- -D warnings`             | Pass; exit 0, no Rust warnings or errors.                                                                                                                                     |
| `cargo nextest run --workspace --all-features`                         | **500/500 passed**, 0 skipped.                                                                                                                                                |
| `cargo test --doc --workspace --all-features`                          | **35 passed**, 0 failed, **18 ignored**.                                                                                                                                      |
| `cargo build --workspace --all-features`                               | Pass; exit 0.                                                                                                                                                                 |
| `cargo check -p eventcore-postgres --lib`                              | Standalone library check passed.                                                                                                                                              |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p eventcore-postgres --no-deps` | Standalone warning-denied rustdoc passed.                                                                                                                                     |
| Focused four-binary PostgreSQL projection suite                        | **110/110 passed**, 0 skipped: delivery 27, transactional 47, continuous 12, reset 24.                                                                                        |
| Scoped mutation runs                                                   | **92 outcomes**: **31 caught + 61 unviable**, 0 missed/survivors, 0 timeouts. The 61 unviable outcomes are compile-invalid or otherwise unrunnable and are not called killed. |
| Commit policy over merge base through `c79be41a`                       | **31/31** commits had good signatures and Conventional Commit subjects; 0 signature failures, 0 subject failures.                                                             |
| Final PostgreSQL lifecycle state                                       | **0** other client sessions and **0** exact test-owned projection schemas after guarded cleanup.                                                                              |

Mutation scope was `eventcore-types/src/projection_delivery.rs` (18 total: 4 caught, 14
unviable) and `eventcore-postgres/src/projections/{runner,store,reset}.rs` (74 total: 27 caught,
47 unviable). No mutant survived or timed out, and source diffs were clean after the runs.

The pinned-database command `cargo audit --db .cargo-advisory-db` exited zero with exactly the five
allowed warnings below:

| Kind     | Crate                  | Advisory/status                                                               |
| -------- | ---------------------- | ----------------------------------------------------------------------------- |
| Advisory | `anyhow 1.0.102`       | `RUSTSEC-2026-0190`: unsoundness in `Error::downcast_mut()`                   |
| Advisory | `event-listener 5.4.1` | `RUSTSEC-2026-0221`: `!Send` tags can cross thread boundaries via `StackSlot` |
| Advisory | `rand 0.8.5`           | `RUSTSEC-2026-0097`: unsound with a custom logger using `rand::rng()`         |
| Yanked   | `chacha20 0.10.0`      | yanked version                                                                |
| Yanked   | `spin 0.9.8`           | yanked version                                                                |

The repository's separate `RUSTSEC-2023-0071` ignore covers SQLx's unused MySQL-backend RSA path
and did not add a sixth warning.

## Existing CI and timing decision

The existing `.github/workflows/ci.yml` already provides PostgreSQL 17 for all-feature workspace
nextest and doctests, caches Rust dependencies, and runs all-target/all-feature Clippy under the
workflow-wide `RUSTFLAGS=-D warnings`. Its release-label mutation job also uses PostgreSQL 17,
caching, nextest, mutation artifact upload, and an explicit survivor gate.

The measured local all-feature nextest suite completed in 13.411 seconds wall time and the focused
110-test projection suite in 8.307 seconds wall time, far below the approximately ten-minute
partitioning threshold. No CI workflow modification is warranted for this increment. These local
measurements do not predict or substitute for actual hosted checks: remote CI results are not yet
available and remain a delivery gate.

## Independent final reviews

Seven independent Astra/high read-only reviews inspected the final `c79be41a` state. All prior
review findings were remediated and re-reviewed. Each final review reported zero Critical, zero
Important, and zero Minor findings.

| Review                                                    | Final outcome and independent evidence                                                                                         |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Architecture conformance (`architecture_review`)          | **APPROVED / PR-ready**; independently reran 110/110.                                                                          |
| Transaction ownership (`task2_red_transaction_review`)    | **APPROVED / PR-ready**; independently reran 110/110.                                                                          |
| Concurrency and fencing (`task2_red_concurrency_review`)  | **APPROVED / PR-ready**; independently reran 110/110.                                                                          |
| Recovery and fault truthfulness (`task4_delivery_review`) | **APPROVED / PR-ready**; inspected the final evidence, with no separate rerun.                                                 |
| API compatibility and semver (`api_compat_review`)        | **APPROVED / PR-ready**; fresh standalone library, rustdoc, facade, legacy, configuration, and decode checks.                  |
| Test quality (`final_test_quality_review`)                | **APPROVED / PR-ready**; source/evidence inspection, including the supplied protected-CI excerpt, with no separate test rerun. |
| Documentation accuracy (`task8_docs_review`)              | **APPROVED / PR-ready subject to this report** after evidence corrections; accepted ADR hashes confirmed.                      |

These approvals are local independent review evidence only. Hosted PR review requirements still
must be satisfied after the pull request exists.

## Architecture and documentation references

- [ADR-0050: Transactional PostgreSQL projections](../adr/ADR-0050-transactional-postgresql-projections.md)
  defines transaction ownership, leadership, reset, and compatibility.
- [ADR-0051: Lossless global projection delivery](../adr/ADR-0051-lossless-global-projection-delivery.md)
  defines the source-scoped frontier and envelope contract.
- The accepted ADR blobs are unchanged since acceptance:
  ADR-0050 is `086aa077da612d033d23e061391f302b76839c2c`; ADR-0051 is
  `30b8e54a250e0c3983d08b6968a76d09eb59ebbd`.
- Current narrative and usage documentation is in the
  [architecture manual](../manual/01-introduction/04-architecture.md),
  [projection manual](../manual/02-getting-started/04-projections.md), and
  [PostgreSQL README](../../eventcore-postgres/README.md).
- Public release notes are staged under Unreleased in the
  [`eventcore` changelog](../../eventcore/CHANGELOG.md),
  [`eventcore-types` changelog](../../eventcore-types/CHANGELOG.md),
  [`eventcore-postgres` changelog](../../eventcore-postgres/CHANGELOG.md), and
  [`eventcore-testing` changelog](../../eventcore-testing/CHANGELOG.md).

## Foundry consumption

Foundry should consume exact version `=2.1.0` with feature `postgres` after merge and separately approved publication.

```toml
eventcore = { version = "=2.1.0", features = ["postgres"] }
```

Publication has not occurred and remains unauthorized. Merge has not occurred and also remains
unauthorized. The version recommendation is therefore prospective, not currently consumable.

## Remaining delivery gates

1. Commit this report with the repository's signed Conventional Commit policy.
2. Push `feat/transactional-postgres-projections` through the repository's normal SSH remote.
3. Open the pull request against `main` and record its actual URL.
4. Wait for every required hosted CI check at the pushed SHA, including any release-label mutation
   requirement, and repair/rerun failures until the replacement evidence is green.
5. Satisfy hosted pull-request review requirements and resolve all comments. The seven independent
   approvals above do not replace this gate.
6. Obtain explicit owner authorization before merge. Do not merge as part of release readiness.
7. After merge, obtain separate explicit owner authorization for the lockstep 2.1.0 publication.
   Do not publish any crate before that authorization.
