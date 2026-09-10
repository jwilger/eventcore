# Task 6 delivery report: fenced leadership, finite drain, and continuous catch-up

## Status

`GREEN_COMPLETE`

The Task 6 contracts and all prior behavior are green. The runner now holds one close-on-drop PostgreSQL leadership session for its full lifetime, drains fixed-frontier finite cycles, validates saved identity before an empty result can complete, classifies proven connection loss as `LeadershipLost`, and implements idle-only continuous polling with responsive cancellation.

## Files changed

- `eventcore-postgres/src/projections/config.rs` — adds the minimal cloneable `ProjectionPollSleeper` configuration seam, Tokio default, builder, and accessor needed to observe actual awaited continuous idle delays without pausing database I/O.
- `eventcore-postgres/src/projections/mod.rs` and `eventcore-postgres/src/lib.rs` — export the new poll-sleeper API.
- `eventcore-postgres/src/projections/runner.rs` — acquires and retains leadership before progress validation, resumes from durable progress, factors fixed-frontier drain cycles, distinguishes rollback-confirmed application/progress failures from actual leadership-session loss, and implements cumulative continuous delivery with idle-only poll/cancel selection.
- `eventcore-testing/src/projection_contract.rs` — adds backend-neutral finite execution, fencing, and continuous-mode fixture contracts and public observations.
- `eventcore-postgres/tests/transactional_projection_contract_test.rs` — adapts the PostgreSQL fixture for finite paging, empty/no-match identity validation, overlapping leadership, exact leader-session loss, and fixed high-water behavior. Two-phase application gates and bounded abort/join cleanup prevent concurrency-test hangs.
- `eventcore-postgres/tests/projection_continuous_test.rs` — adds a real PostgreSQL destination fixture with a controllable backend-neutral source and an awaited-sleeper observer for multi-page post-catch-up delivery, positive non-busy idle waiting, and normal cancellation.

## Contract coverage

- More than one page: five selected events with page size two; effect count and progress must reach the fifth position.
- Fixed finite frontier: append a second event only after the first run entered application code; the first run must stop at its original high-water mark and the next run must process the append.
- Empty and pure no-match completion: both `None` high-water and a global frontier containing only an unselected event must return finite `CaughtUp` without manufactured progress.
- Trailing unselected frontier: append selected position N followed by unselected N+1; require effect count one, progress exactly N, and `CaughtUp.through` exactly N+1.
- Carried identity validation: saved source and selection are simultaneously incompatible for empty and no-match runs, so source mismatch must be reported first. Separate selection-only mismatch cases require `SelectionIdentityMismatch`; every path proves zero application attempts and effects.
- Leadership exclusion: a first runner is held inside its transaction after acquiring leadership; an overlapping writer for the same projector must receive `LeadershipBusy`.
- Exact session-loss fencing: the fixture reads `pg_backend_pid()` through the runner-supplied transaction, waits until all pre-gate SQL has completed and application code is blocked, terminates that exact backend, releases application work, bounded-joins the stale runner, and independently verifies no effect or progress committed. The required public classification is `LeadershipLost`.
- Continuous catch-up: begin with five selected events at page size two. The first actual idle sleep captures independently committed effect/progress and must show all five events drained before sleep. Append three more events, release the first sleep, require the second idle boundary to show all eight events committed, then cancel and require `Cancelled { processed: 8, skipped: 0 }` with matching final progress.
- No inter-page sleeping: the two exact idle-boundary observations must be `(effect=5, progress=5)` and `(effect=8, progress=8)`; an awaited sleep between any populated pages fails at the boundary where it occurs.
- Exact polling and cancellation: both observed delivery-cycle sleeps must request exactly 17ms. The empty runner must hold exactly one awaited 17ms sleep pending, cancel it, and return normal `Cancelled { processed: 0, skipped: 0 }`. The standalone poll-sleeper API test also proves constructing and dropping an unpolled sleep future records nothing.
- Every spawned concurrency task uses bounded rendezvous, bounded completion, and timeout-bounded abort-and-join cleanup.
- Idle-observation failure and channel-closure regressions use a deliberately pending runner and prove both paths return their exact fixture error only after the runner is aborted and joined. Completed-runner results are carried as explicit state so cleanup never polls a consumed `JoinHandle` twice.

## Verification evidence

`nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test -E 'test(/(leadership|batch|page)/)' --no-fail-fast`

- PASS: 13/13.

`nix develop -c cargo nextest run -p eventcore-postgres --test projection_continuous_test`

- PASS: 4/4, including both observation-failure cleanup regressions.

`nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test --no-fail-fast`

- PASS: 36/36.

`nix develop -c cargo nextest run -p eventcore-postgres --test projection_delivery_contract_test`

- PASS: 18/18.

`nix develop -c cargo nextest run -p eventcore-testing`

- PASS: 43/43.

Task 3–5 regression command:

`nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test -E 'not test(/(leadership|batch|page|poll_sleeper)/)'`

- PASS: 22/22.

`nix develop -c cargo fmt --all --check`

- PASS.

`nix develop -c cargo clippy --all-targets --all-features -- -D warnings`

- PASS.

`nix develop -c cargo nextest run --workspace --no-fail-fast`

- PASS: 402/402.

`nix develop -c cargo build --workspace`

- PASS.

`nix develop -c cargo test --doc --workspace --all-features`

- PASS: all workspace doctests (35 passed, 18 ignored).

## Production behavior

The runner acquires leadership before reading durable progress and validates source identity before selection identity. Each drain cycle captures one inclusive high-water mark and reads selected pages from the durable cursor until an empty page certifies catch-up; its outcome reports the global frontier while progress remains at the last selected event. Continuous mode retains the same leader, starts a fresh bounded cycle after each configured idle wait, accumulates counts, and returns normal `Cancelled` when cancellation wins the idle selection.

Transactional error classification remains specific: ordinary application and progress errors retain their typed Task 4 variants when rollback confirms that the transaction and connection remain usable. A failed rollback after the exact leader backend is terminated is truthful evidence that the leader session was lost, so the runner returns `LeadershipLost` and performs no stale write.

## Concerns

- The poll-sleeper seam is intentionally public and parallels the existing retry-sleeper seam. Its default is Tokio-backed; custom sleepers make actual awaited idle boundaries deterministic without pausing database I/O.
- No known correctness or verification concerns remain for Task 6.
