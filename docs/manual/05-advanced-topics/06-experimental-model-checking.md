# Chapter 5.6: Experimental Model Checking

> This API is experimental even though it ships in standard EventCore releases.
> Enable it only in applications evaluating the approach; it may change while
> its feature flags retain the `experimental-` prefix.

Enable the runtime lane in normal code and the checker in test targets:

```toml
[dependencies]
eventcore = { version = "1.1", features = ["experimental-modeling"] }

[dev-dependencies]
eventcore = { version = "1.1", features = ["experimental-model-check"] }
```

`StreamIdentity` makes stream roles semantic newtypes. Mark external, actor,
session, or generated input fields with `#[model(origin)]` (or a descriptive
role such as `#[model(origin = actor)]`); these are the only input fields that
accept raw builder values and act as checker roots. State fields must declare
one root recipe: `#[model(default)]`, `#[model(absence)]`, or
`#[model(constant)]`. Option and collection types are otherwise atomic values,
not implicit roots.
`ModelInput`,
`ModelCommand`, `ModelEvent`, `ModelState`, `ModelReadModel`, `ModelEffect`, and
`ModelOutput` generate typestate builders. A non-input builder accepts only a
`FieldValue` for its exact occurrence, normally produced by `mapping!`.

```rust
mapping! {
    RequestAmount:
        TransferRequest.amount => Transfer.amount
        using clone;
}
```

The same mapping is executable and registered for `eventcore::model::check()`.
The checker verifies structural provenance: it follows mappings from modeled
outputs back to explicit inputs and default state roots. It rejects missing
producers, unknown sources, duplicate descriptor names, and ordinary cycles.
Diagnostics have stable codes, remediation text, dependency traces for
provenance failures, and source locations captured from derives or mappings.
Successful reports have status `Verified` or, when a named boundary is
accepted, `Assumed`; failed checks return a `CheckError` whose status is
`Incomplete`.
It does **not** prove that a formula represents the intended business rule;
continue to test command decisions and projections normally.

## Guarantee boundary

The checker establishes only structural information completeness: every
modeled downstream field has a type-correct executable provenance path to an
explicit root or an accepted named assumption. It does not prove formula
intent, meaningful use of function arguments, branch feasibility, business
invariants, authorization, liveness, concurrency behavior, serialization
fidelity, provenance of historical events loaded from storage, correctness of
custom sinks, external truth, or the absence of intentional owner bypass.
Those properties remain application and integration-test responsibilities.

The current representation supports named structs, unit event variants, and
event variants with one typed payload. Anonymous multi-field tuple variants
and generic modeled component owners are rejected at compile time. Nested
generic field values are supported; `Option` and collection fields are treated
as atomic values in this experiment.

For an intentionally opaque native persistence boundary, annotate the field
with `#[model(assumption = "descriptive-name")]`. Strict `check()` rejects
that boundary. A test may explicitly permit it with
`CheckOptions::default().allow_assumption("descriptive-name")`; the resulting
report is `Assumed`, never `Verified`.

`ModeledCommand` delegates to `execute`, preserving EventCore's stream reads,
optimistic concurrency, and atomic appends. `checked_projection` adapts a
pure `ModelProjection` plus an imperative `ProjectionSink` to the existing
`Projector` interface. `InMemoryProjectionSink` is available for executable
tests and examples. Application-owned SQL or native sinks are an assumption
boundary: annotate their affected modeled field with a descriptive
`#[model(assumption = "...")]` and permit that exact name in `CheckOptions`.
Such a check reports `Assumed`, never `Verified`.

The complete bank-transfer test is in
`eventcore-examples/tests/experimental_modeling_test.rs`. It checks the graph,
executes a command against the real in-memory store, and applies a modeled
projection effect.

Release builds that do not enable either feature contain none of this module,
its macro expansions, or the inventory registry.

## Performance checks

`eventcore-bench` includes `model_check`, which measures the real checker at
100, 1,000, and 10,000 field nodes plus modeled-wrapper versus legacy command
logic. A separate DHAT integration test compares heap allocations for the
legacy and modeled decision paths; both controls perform zero allocations for
the no-event case. The Criterion benchmark intentionally has no wall-clock CI
assertion. On the pinned local development environment, the 10,000-node linear
check is expected to remain below the 100 ms experiment target; run it with:

```sh
cargo bench -p eventcore-bench --bench model_check
```

Run the allocation control with:

```sh
cargo test -p eventcore-bench --test model_wrapper_allocations
```
