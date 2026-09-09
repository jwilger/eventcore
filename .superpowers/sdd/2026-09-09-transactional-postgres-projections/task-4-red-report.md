# Task 4 RED report — recovery contracts

## Scope

This checkpoint changes only the reusable contract and PostgreSQL test fixture.
It deliberately does not change `runner.rs`, `store.rs`, or `error.rs`
production recovery behavior.

## Scenarios added or enabled

- `ApplyThenFail` executes the non-idempotent mutation through the supplied
  transaction and then returns an application error; both the effect and
  progress must roll back at the exact pending position.
- A progress-write trigger failure rolls back the already-applied mutation,
  leaves progress absent, and must report the exact pending position.
- A new batch invocation resumes after the last committed position, without
  duplicating the first effect.
- The pre-existing atomicity contract continues to prove redelivery suppression
  of a non-idempotent effect.
- Malformed selected input returns `Decode` at its exact delivery position,
  leaves effect/progress absent, and does not enter application code.
- Distinct source and selection identity mismatches are required to stop before
  application code runs.
- A test-local TCP PostgreSQL transport proxy forwards the exact simple-query
  frontend `COMMIT` frame. It detects PostgreSQL's backend `CommandComplete`
  frame for `COMMIT`, then uses an independent direct database connection to
  confirm the matching effect and progress row have committed atomically before
  dropping the runner transport without forwarding the acknowledgement. This
  proves the committed/lost-ACK branch rather than a pre-commit disconnect.
  The run uses a nonzero retry configuration but must have exactly one apply
  attempt, no after-commit hook, and a positioned `CommitIndeterminate`.

The fixture observes normal behavior only through public runner outcomes,
public progress, read-model state, and hook/application observations. Raw SQL
is limited to deterministic fixture setup, durable-state observation, and the
progress-failure trigger. The commit fault has no production hook or SQL
trigger dependency.

## Commands and results

```text
nix develop -c cargo fmt --all
nix develop -c cargo check -p eventcore-testing -p eventcore-postgres --tests
```

Both completed successfully.

```text
nix develop -c cargo nextest run -p eventcore-testing
```

Passed: 43/43 tests.

```text
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test -E 'test(/(rolls_back|restart|redelivery|malformed|identity|commit)/)'
```

Result: 5 passed, 4 failed. The only failures are intentional behavioral RED
failures:

1. `progress_failure_rolls_back_effect_and_progress` expected
   `Failed(Progress { position: DeliveryPosition(1) })` but received
   `Failed(ProgressWithoutPosition)`.
2. `source_identity_mismatch_stops_before_application_code` expected
   `Failed(SourceIdentityMismatch)` but received
   `Failed(UndifferentiatedIdentityMismatch)`.
3. `selection_identity_mismatch_stops_before_application_code` expected
   `Failed(SelectionIdentityMismatch)` but received
   `Failed(UndifferentiatedIdentityMismatch)`.
4. `commit_acknowledgement_loss_is_indeterminate_without_after_commit_or_retry`
   expected `Failed(CommitIndeterminate { position: DeliveryPosition(1) })`
   but received `Failed(CommitIndeterminateWithoutPosition)`.

The fixture classifies the existing public
`TransactionalProjectionError::IdentityMismatch` as
`UndifferentiatedIdentityMismatch`; it intentionally does not pretend the
generic old error identifies which persisted binding was wrong. It likewise
classifies the existing positionless public `CommitIndeterminate` variant as
`CommitIndeterminateWithoutPosition`, and it classifies the existing
positionless `Progress` variant as `ProgressWithoutPosition`. GREEN must add
and return distinct public source- and selection-identity error variants before
application code is invoked, and include the pending position in `Progress` and
`CommitIndeterminate`.

`classify_runner_error` in
`eventcore-postgres/tests/transactional_projection_contract_test.rs` is a
Task 4-owned PostgreSQL fixture adapter, not shared contract behavior. GREEN
will replace its legacy arms with arms for the new production variants and
position fields; the backend-neutral `eventcore-testing` assertions and their
expected observations remain unchanged.

## Mutation claims

- A mutation outside the runner-owned transaction fails the post-mutation
  application-error rollback assertion.
- A non-transactional progress write fails the exact-position progress-failure
  assertion.
- Resuming from an in-memory/source cursor rather than durable progress fails
  restart and redelivery assertions.
- Advancing on malformed selected input fails its exact-position `Decode`
  assertion.
- Conflating source and selection bindings leaves both identity cases RED.
- Retrying or invoking hooks after the proxied committed/lost-ACK fault fails
  the application-attempt/hook assertions. The proxy itself refuses to close
  the runner transport until its independent direct connection observes the
  committed effect and matching progress row.

## Cleanup and concerns

Each contract case uses a panic-safe helper that catches contract assertions,
runs schema cleanup, then resumes the original panic. Every runner and proxy
confirmation is bounded by a two-second timeout; proxy tasks are aborted and
joined during cleanup. The proxy accepts one runner connection and its direct
observer is the only post-COMMIT synchronization point—there are no timing
sleeps. If PostgreSQL's `CommandComplete(COMMIT)` becomes readable before the
frontend forwarding task signals the simple-query `COMMIT`, the proxy holds
that completion under the same bound until it consumes the signal; it never
forwards a candidate commit acknowledgement due to scheduler ordering. The
focused RED expression was run twice with the same 5-pass/4-fail
result. A subsequent intentional identity panic left the pre-existing count
of `eventcore_transactional_test_%` schemas unchanged at 15, demonstrating the
new panic-safe helper does not leak schemas.

The current RED suite demonstrated that much of the existing minimal runner
already had correct transactional rollback behavior. Its pending production
work is resolved by the GREEN evidence below.

## GREEN evidence

The production runner now wraps progress storage failures with the envelope's
pending `DeliveryPosition`, while positionless public store APIs retain a
separate `ProgressStore` error. Source and selection bindings now produce
separate public variants with the projector and persisted/configured identities;
source mismatch is checked first. Commit acknowledgement loss carries the
pending position and still bypasses retries and after-commit work.

The test-local classifier now maps those production variants and fields to the
unchanged shared observations. The proxy destination is used only for the run;
subsequent fixture observations keep using the direct destination store, so a
one-connection proxy cannot block cleanup or post-error durable-state checks.

```text
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test
```

Passed: 12/12 tests, including the direct-observed committed/lost-ACK branch.

```text
nix develop -c cargo nextest run -p eventcore-testing
nix develop -c cargo clippy --all-targets --all-features -- -D warnings
```

Passed: 43/43 `eventcore-testing` tests; clippy completed with warnings denied.

## Final fixture review evidence

The proxy backend now reads each complete PostgreSQL frame sequentially. It
never places a partial-frame read in `tokio::select!`; only after an entire
`CommandComplete(COMMIT)` frame is decoded does it boundedly wait for the
frontend `COMMIT` signal, independently observe durable state, and close the
transport without forwarding the acknowledgement.

The backend-neutral fixture contract now requires a fresh normal-destination
recovery invocation after the committed/lost-ACK result. It proves that durable
progress suppresses application code and the non-idempotent effect, retains the
exact identity/position binding, and does not replay the after-commit hook that
was skipped because acknowledgement was unknown. The PostgreSQL fixture tears
down the one-connection proxy and uses its direct store for that invocation.

```text
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test -E 'test(commit_acknowledgement_loss)'
```

Passed three consecutive times (1/1 each; 1.564 s, 1.560 s, and 1.886 s).

```text
nix develop -c cargo nextest run -p eventcore-postgres --test transactional_projection_contract_test
nix develop -c cargo nextest run -p eventcore-testing
nix develop -c cargo clippy --all-targets --all-features -- -D warnings
```

Passed: 12/12 transactional PostgreSQL tests, 43/43 testing-crate tests, and
warnings-denied clippy.
