# Task 5 RED Report — Explicit Failure Policies

## Result

`RED_READY`. The backend-neutral projection contract and PostgreSQL fixture now cover every
Task 5 policy without implementing the runner state machine. The focused expression compiles and
finishes with 6 passes and 7 intentional behavioral failures. Of the eight Task 5 cases,
after-commit ordering/rollback suppression already passes; the other seven remain RED for the
missing public variants or state-machine branches.

## Contract scenarios

- Retry exhaustion configures two retries and requires exactly three application invocations,
  `RetryExhausted { position, attempts: 3 }`, no effect/progress/hook, and zero committed attempt
  rows. Its fixture script contains three explicit `Retry` decisions, so a correct runner cannot
  accidentally succeed by exhausting a one-entry script.
- Transient retry fails once and then succeeds. Every invocation inserts an attempt row through
  the supplied runner transaction; only the successful transaction's row may remain. The exact
  external atomic invocation count is two. Each invocation also reads PostgreSQL's exact
  top-level transaction ID through the supplied transaction and records it outside rollback
  state; the two IDs must differ. Exhaustion similarly requires three pairwise-distinct IDs,
  rejecting savepoint reuse inside one transaction.
- Retry progress reload commits the pending progress through an independent fixture connection
  during the first failing application attempt. A correct retry must roll back, start fresh,
  reload progress, suppress a second invocation, and report zero processed.
- Retry delay capping uses a one-hour initial delay with a 50 ms maximum and an injected recording
  sleeper that resolves immediately. The contract requires exactly one requested sleep of 50 ms.
  This deterministically fails both omitted sleeping and omitted capping without wall-clock
  thresholds, assertion sleeps, or Tokio paused-time interaction with live PostgreSQL I/O. The
  recording occurs inside the returned async future when awaited, so constructing and dropping an
  unpolled future records nothing; an implementation cannot satisfy the contract by merely calling
  `sleep` and discarding its future.
- Skip applies both the attempt row and non-idempotent increment before returning its explicit
  decision. Both must roll back, then a fresh transaction advances only progress and reports
  exactly one skip and zero processed.
- Stop runs after one committed apply and one committed skip. It must leave the third position
  pending while preserving exact totals `processed: 1, skipped: 1`, with no hook for the stopped
  attempt.
- Fatal requires the intended typed application failure at the exact pending position, with its
  attempted transaction rolled back and no hook.
- Successful after-commit work queries the read model through an independent connection and logs
  success only after it observes both the transaction-scoped attempt row and matching durable
  progress. An apply-then-fail rollback does not invoke it.
- Failing after-commit work first observes committed state, then fails. The public classification
  must carry `committed_position` and the exact stable `fixture after-commit sentinel` source
  evidence; a later invocation must suppress application and must neither retry nor replay the
  failed hook.

`eventcore-testing` remains free of SQLx/PostgreSQL. Its fixture boundary exposes only behavior,
outcomes, durable progress/effect counts, transaction-owned attempt-row counts, opaque transaction
tokens, requested retry sleeps, application invocation counts, and hook observations.
PostgreSQL SQL remains inside the backend fixture.

The smallest production seam required to compile this RED is public
`ProjectionRetrySleeper: Debug + Send + Sync`. `PostgresProjectionConfig` stores it behind `Arc`,
retains derived `Clone`/`Debug`, defaults to documented `TokioProjectionRetrySleeper` behavior via
`tokio::time::sleep`, and exposes a generic builder plus read-only accessor. The runner does not
yet consult this seam; that remains GREEN state-machine work. A focused API test proves cloned
configuration routes two distinct awaited durations to an injected sleeper while a constructed
and dropped future records nothing.

## RED evidence

```text
nix develop -c cargo fmt --all
nix develop -c cargo check -p eventcore-testing -p eventcore-postgres --tests
```

Both passed.

```text
nix develop -c cargo nextest run -p eventcore-postgres \
  --test transactional_projection_contract_test \
  -E 'test(/(retry|skip|stop|fatal|after_commit)/)'
```

Run twice with the same result: 13 selected by the requested expression, 6 passed and 7 failed.
The expression also
selects three earlier tests because their names contain `stop` or `retry`; those remain green.
Intentional Task 5 failures are:

1. transient retry: existing runner returned `ApplicationFatal` instead of completing once;
2. after-commit failure: legacy public failure classified as
   `LegacyAfterCommitFailed { committed_position, source }` instead of the intended
   `AfterCommitFailed { committed_position, source }`;
3. stop after prior apply/skip: existing runner failed fatally at the skip position instead of
   reaching a stopped outcome at the third position with both counters retained;
4. progress reload: existing runner returned `ApplicationFatal` instead of suppressing reapply;
5. fatal: legacy public failure classified as `Other` instead of the intended `Application`;
6. skip: existing runner returned `ApplicationFatal` instead of a caught-up one-skip outcome;
7. retry exhaustion: existing runner returned `ApplicationFatal` instead of
   `RetryExhausted { attempts: 3 }`.

The already-correct after-commit ordering and rollback-suppression contract passed.

```text
nix develop -c cargo nextest run -p eventcore-postgres \
  --test transactional_projection_contract_test \
  -E 'test(/(config|retry_sleeper|classifier_forwards)/)'
```

Passed: 3/3. In addition to config defaults and sleeper cloning/routing, the provenance regression
constructs two legacy after-commit failures with distinct sources and proves the adapter forwards
each actual `source.to_string()` into its legacy observation. This prevents satisfying the shared
sentinel assertion by hardcoding the sentinel in the classifier.

```text
nix develop -c cargo nextest run -p eventcore-postgres \
  --test transactional_projection_contract_test \
  -E 'test(/retry_sleeper/)'
```

Passed: 1/1. It explicitly proves an unpolled/dropped sleeper future records nothing and two
awaited futures record their distinct durations exactly once through cloned configuration.

```text
nix develop -c cargo nextest run -p eventcore-postgres \
  --test transactional_projection_contract_test \
  -E 'test(/^(effect_and_progress|application_mutation|progress_failure|restart_resumes|malformed_selected|source_identity|selection_identity|commit_acknowledgement)/)'
```

Passed: 8/8 exact Task 3/4 recovery contracts.

```text
nix develop -c cargo nextest run -p eventcore-testing
```

Passed: 43/43.

## Mutation claims

- `max_retries` off by one changes the exact external invocation count and terminal attempt field.
- Omitting rollback commits extra attempt rows/effects on transient success. Reusing one
  transaction or savepoints repeats the externally observed top-level transaction token instead
  of producing pairwise-distinct IDs.
- Omitting sleep or dropping its unawaited future leaves the recording sleeper empty; ignoring the
  cap records one hour instead of the exact expected 50 ms.
- Omitting durable progress reload invokes application twice in the externally-advanced retry
  case and duplicates its transaction-owned work.
- Committing the failed skip transaction exposes its increment/attempt row; failing to advance in
  a fresh transaction leaves progress absent; miscounting changes the literal outcome.
- Advancing stop/fatal changes the exact public progress position; discarding prior totals changes
  the literal stopped outcome.
- Calling after-commit before commit makes its independent query fail to observe the attempt row
  and progress. Calling it on rollback or replay changes the hook attempt count/log.
- Treating hook failure as transaction failure duplicates the non-idempotent effect or invokes the
  hook again on the recovery run. Dropping or replacing the hook's source loses the exact sentinel
  required by the public failure observation.

## Cleanup and concerns

Every contract run remains bounded by `RUN_TIMEOUT`. The existing panic-safe wrapper catches
contract panics, shuts down any proxy task with a bounded abort/join, drops runner resources, drops
the isolated schema, and then resumes the original panic. Hook database reads execute inside the
same bounded runner future. No new detached task, wall-clock threshold, paused Tokio clock, or
assertion sleep was introduced; retry-delay evidence is the exact duration passed to an injected
immediately-ready sleeper.

At RED, the PostgreSQL fixture intentionally mapped the legacy `ApplicationFatal` result for the
explicit Fatal scenario to `Other` and the legacy `AfterCommit` result to a distinct legacy
observation. GREEN had to introduce the Task 5 public `Application` and
`AfterCommitFailed { committed_position, source }` variants and update the fixture classifier;
otherwise the intended public surface would not be tested despite equivalent legacy payload
fields. The existing Task 4 mutation-failure classification remained green.

## GREEN result

`GREEN_READY`. The runner now evaluates every application failure through `on_error` with the
exact pending position, borrowed original error, and one-based `AttemptNumber`.

- Retry explicitly rolls back each failed transaction, requests the overflow-safe exponentially
  scaled delay capped by `maximum_delay`, awaits the configured sleeper, begins a fresh top-level
  transaction, and reloads source-first identity/progress before reapplying. Exhaustion returns
  `RetryExhausted { position, attempts, source }` after exactly the initial invocation plus the
  configured retries.
- Skip rolls back the failed application transaction, reloads progress in a fresh transaction,
  advances progress alone, commits, and increments only the skipped count. Externally advanced
  progress suppresses the skip rather than overwriting or recounting it.
- Stop returns the exact pending position with prior processed/skipped counts after rollback.
- Fatal returns `Application { position, source }` after rollback.
- Confirmed commits invoke after-commit work once. Hook failure returns
  `AfterCommitFailed { committed_position, source }`; durable progress suppresses both application
  and hook replay on later invocations.
- Positioned progress, identity, and `CommitIndeterminate` behavior remains unchanged. Duplicate
  suppression is checked before decoding or invoking application code.

The retry-policy constructor now rejects `u32::MAX` retries because the one-based initial attempt
would not fit `AttemptNumber`; all accepted policies can represent their exact terminal attempt.
Delay calculation short-circuits zero/capped initial durations, treats non-finite scaled results as
the maximum, and calls `Duration::from_secs_f64` only for a finite value below the representable
configured maximum.

## GREEN verification

```text
nix develop -c cargo nextest run -p eventcore-postgres \
  --test transactional_projection_contract_test
```

Passed: 22/22.

```text
nix develop -c cargo nextest run -p eventcore-postgres \
  --test transactional_projection_contract_test \
  -E 'test(/^(effect_and_progress|application_mutation|progress_failure|restart_resumes|malformed_selected|source_identity|selection_identity|commit_acknowledgement)/)'
```

Passed: 8/8 Task 3/4 regression slice.

```text
nix develop -c cargo nextest run -p eventcore-testing
```

Passed: 43/43.

```text
nix develop -c cargo fmt --all
nix develop -c cargo clippy --all-targets --all-features -- -D warnings
nix develop -c cargo build --workspace
nix develop -c cargo nextest run --workspace
```

All passed; the workspace run completed 384/384 tests.

The RED-only legacy after-commit observation was removed. The provenance regression now constructs
two public `AfterCommitFailed` values and proves the PostgreSQL adapter forwards each distinct
actual source string.

## Final review follow-up: exponential backoff coverage

The shared backend-neutral contract now drives two additional public-runner scenarios through the
PostgreSQL recording sleeper:

- Three `Retry` decisions followed by `Apply`, with initial 10 ms, multiplier 2.0, and maximum
  25 ms, must request exactly `[10 ms, 20 ms, 25 ms]`. Four pairwise-distinct top-level transaction
  tokens, one committed attempt row/effect/progress, and one hook prove normal retry semantics.
  This rejects flattening every retry to `initial.min(maximum)`.
- The same script with finite `f64::MAX` multiplier must request exactly
  `[10 ms, 25 ms, 25 ms]`, complete without panic or hang, and retain the same transaction/effect
  invariants. This proves finite multiplication or `powf` overflow saturates at the configured
  maximum while infinity itself remains invalid configuration.

No production code changed for this follow-up; both tests pass against the Task 5 implementation.

```text
nix develop -c cargo nextest run -p eventcore-postgres \
  --test transactional_projection_contract_test -E 'test(/retry_backoff/)'
```

Passed: 2/2.

```text
nix develop -c cargo nextest run -p eventcore-postgres \
  --test transactional_projection_contract_test \
  -E 'test(/(retry|skip|stop|fatal|after_commit)/)'
```

Passed: 15/15.

```text
nix develop -c cargo nextest run -p eventcore-postgres \
  --test transactional_projection_contract_test
nix develop -c cargo nextest run -p eventcore-testing
nix develop -c cargo check -p eventcore-testing -p eventcore-postgres --tests
nix develop -c cargo clippy --all-targets --all-features -- -D warnings
```

Passed: 24/24 transactional tests, 43/43 `eventcore-testing` tests, compile check, and warnings-denied
clippy.
