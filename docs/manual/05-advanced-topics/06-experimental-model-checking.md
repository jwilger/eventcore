# Chapter 5.6: Experimental Model Checking

> This is a pre-release experiment. Enable it only in applications evaluating
> the approach; the API may change before it becomes part of EventCore's stable
> direction.

Enable the runtime lane in normal code and the checker in test targets:

```toml
[dependencies]
eventcore = { version = "=1.1.0-alpha.1", features = ["experimental-modeling"] }

[dev-dependencies]
eventcore = { version = "=1.1.0-alpha.1", features = ["experimental-model-check"] }
```

`StreamIdentity` makes stream roles semantic newtypes. Mark external, actor,
session, or generated input fields with `#[model(origin)]`; these are the only
input fields that accept raw builder values and act as checker roots.
`ModelInput`,
`ModelCommand`, `ModelEvent`, `ModelState`, `ModelReadModel`, and
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
It does **not** prove that a formula represents the intended business rule;
continue to test command decisions and projections normally.

For an intentionally opaque native persistence boundary, annotate the field
with `#[model(assumption = "descriptive-name")]`. Strict `check()` rejects
that boundary. A test may explicitly permit it with
`CheckOptions::default().allow_assumption("descriptive-name")`; the resulting
report is `Assumed`, never `Verified`.

`ModeledCommand` delegates to `execute`, preserving EventCore's stream reads,
optimistic concurrency, and atomic appends. `checked_projection` adapts a
pure `ModelProjection` plus an imperative `ProjectionSink` to the existing
`Projector` interface. `InMemoryProjectionSink` is available for executable
tests and examples.

The complete bank-transfer test is in
`eventcore-examples/tests/experimental_modeling_test.rs`. It checks the graph,
executes a command against the real in-memory store, and applies a modeled
projection effect.

Release builds that do not enable either feature contain none of this module,
its macro expansions, or the inventory registry.

## Performance checks

`eventcore-bench` includes `model_check`, which measures the real checker at
100, 1,000, and 10,000 field nodes plus modeled-wrapper versus legacy command
logic. It intentionally has no wall-clock CI assertion. On the pinned local
development environment, the 10,000-node linear check is expected to remain
well below the alpha target of 100 ms; run it with:

```sh
cargo bench -p eventcore-bench --bench model_check
```
