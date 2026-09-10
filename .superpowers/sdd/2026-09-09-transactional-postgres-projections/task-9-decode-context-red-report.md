# Task 9 final-review remediation RED/GREEN report

## Scope

This TDD increment addresses the two blocking final-review findings:

- PostgresProjector::decode now exposes the complete PersistedEventEnvelope to
  application code, with a source-compatible default that deserializes payload
  as JSON.
- Public PostgreSQL coverage specifies application-owned event-type routing,
  unsupported-discriminator decode failures, exact failure callback context, and
  terminal retry source retention.

The RED phase retained the runner's direct payload deserialization. The GREEN
phase now calls the application-owned decoder after duplicate suppression and
before application, retaining its boxed source as the terminal Decode failure.

## Owned files

- eventcore-postgres/src/projections/projector.rs
- eventcore-postgres/src/projections/runner.rs
- eventcore-postgres/tests/transactional_projection_contract_test.rs
- eventcore-postgres/README.md
- eventcore-postgres/CHANGELOG.md
- docs/manual/02-getting-started/04-projections.md
- docs/adr/ADR-0050-transactional-postgresql-projections.md
- docs/adr/ADR-0051-lossless-global-projection-delivery.md
- .superpowers/sdd/2026-09-09-transactional-postgres-projections/task-9-decode-context-red-report.md

The pre-existing staged task-9-verification-report.md is preserved and is not
owned by or included in this increment's commit.

## Public API skeleton

The additive method accepts a borrowed PersistedEventEnvelope and returns
Result<Self::Event, BoxedProjectionError>. The default retains the existing
Event: DeserializeOwned payload-only behavior. Its documentation assigns
envelope interpretation to the application and states that failures are
terminal TransactionalProjectionError::Decode failures.

## Intentional RED evidence

Command:

    nix develop --command cargo nextest run -p eventcore-postgres \
      --test transactional_projection_contract_test \
      -E 'test(projector_decode_routes_by_persisted_event_type_and_rejects_unsupported_discriminator)' \
      --no-capture

Result on two consecutive runs: expected failure each time, 0 passed; 1
failed; 37 skipped. Both runs produced the same exact assertion below and both
completed their bounded schema cleanup path.

Exact assertion:

    assertion left == right failed
      left: DeliveryPosition(1)
     right: DeliveryPosition(3)

The three events were appended through the public EventStore API. Credit,
debit, and unsupported types have the same JSON shape. The application decode
override routes the first two to opposite typed effects and rejects the third.
Because the runner bypasses the hook, its payload-only enum decode fails on the
first event instead of returning the application-owned unsupported-type failure
at position 3.

The test also requires, once GREEN:

- exact model total 7 - 2 = 5;
- exactly two application invocations;
- exactly zero application failure-policy invocations;
- exactly zero retry-sleeper requests despite a configured nonzero retry budget;
- decode position 3 and source downcast to the application error;
- progress fixed at the previously committed debit position 2, with exact source
  and selection identities.

The custom projector's on_error implementation increments an external counter,
returns Retry, and runs with two allowed retries plus a recording retry sleeper.
Consequently a decode failure accidentally routed through application policy is
observable as a policy call, a sleep request, and an application retry; terminal
Decode must produce none of them.

## Failure-context contract

Command:

    nix develop --command cargo nextest run -p eventcore-postgres \
      --test transactional_projection_contract_test \
      -E 'test(failure_context_and_retry_exhaustion_retain_exact_position_attempt_and_error)' \
      --no-capture

Result: pass.

The test appends two events through the public EventCore API. Event 1 fails once
and then commits. Event 2 fails with distinct initial and final sentinels and
exhausts one retry. It asserts the exact ordered callback observations:

1. position 1, attempt 1, FirstTransient, matching message;
2. position 2, attempt 1, SecondInitial, matching message;
3. position 2, attempt 2, SecondFinal, matching message.

It further requires public RetryExhausted position 2, attempts == 2, a source
downcast to SecondFinal, the final source message, read-model total 1, and
progress remaining at position 1 with exact source and selection identities.
This proves failed transactional effects roll back while the prior event remains
committed.

## Mutation sensitivity

Each mutation was applied temporarily to production runner code, the exact
failure-context test was run, and the mutation was then restored.

### Attempt forced to one

Mutation: construct every callback context with attempt 1.

Result: expected failure. The projector's exact-context branch rejected the
mutated third callback, so the runner returned Application at position 2 with
SecondFinal instead of the required RetryExhausted result.

### Position replaced with the first position

Mutation: construct every callback context with delivery position 1.

Result: expected failure. The projector's exact-context branch rejected the
mutated second-event callback, so the runner returned Application at position 2
with SecondInitial instead of the required RetryExhausted result.

### Terminal retry source replaced

Mutation: replace the RetryExhausted source with an unrelated I/O error.

Result: expected failure:

    assertion left == right failed
      left: None
     right: Some(SecondFinal)

All three mutations are restored. The only runner diff that remains is the
intended GREEN change from direct payload deserialization to the projector-owned
decode call.

## Timeout and cleanup hygiene

The new isolated-database harness separately bounds database setup, the complete
contract body (including all post-run observations), and each cleanup attempt.
Cleanup retries once after an error or timeout. A contract panic or contract
timeout remains the primary failure: cleanup is still attempted twice when
necessary, but cleanup failure cannot replace the original panic/timeout. A
cleanup failure remains terminal when the contract itself succeeded.

## GREEN evidence

The runner now calls projector.decode(&envelope), keeps the returned application
event owned for apply and retry, and maps the returned BoxedProjectionError
directly into TransactionalProjectionError::Decode. It does not clone envelope
metadata or route decode failures through application failure policy.

Focused GREEN:

- envelope-aware decode contract: 1 passed, 37 skipped;
- failure-context contract: 1 passed, 37 skipped;
- complete transactional projection test target: 38 passed;
- delivery target: 18 passed;
- continuous target: 4 passed;
- reset target: 13 passed;
- facade target: 1 passed;
- eventcore-testing: 43 passed.

The isolated-database setup now drops its newly created schema if destination
pool construction fails. Normal cleanup uses DROP SCHEMA IF EXISTS, remains
bounded, and can safely retry.

## Full verification

Passing:

- nix develop --command cargo fmt --all
- nix develop --command cargo check --workspace --all-targets --all-features
- transactional projection regressions excluding only the intentional decode RED
- nix develop --command cargo nextest run -p eventcore-postgres --test projection_continuous_test
- nix develop --command cargo nextest run -p eventcore-postgres --test projection_reset_test
- nix develop --command cargo nextest run -p eventcore-testing
- git diff --check

Additional fresh release-gate evidence:

- cargo fmt --all -- --check: pass;
- cargo check --workspace --all-targets --all-features: pass;
- cargo clippy --all-targets --all-features -- -D warnings: pass;
- cargo nextest run --workspace: 420 passed;
- cargo test --doc --workspace --all-features: pass;
- cargo build --workspace: pass;
- cargo audit: pass, 371 dependencies scanned with 5 explicitly allowed warnings.

Before GREEN, the intentional decode RED was executed twice consecutively after
the timeout, policy-counter, and retry-sleeper assertions were added; both runs
failed at the same position 1 versus position 3 assertion and exited in under
one second.

After GREEN, all focused and full verification commands above pass.
