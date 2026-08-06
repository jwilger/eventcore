# EventCore Architecture

EventCore keeps its stable runtime intentionally small: commands implement
`CommandLogic`, `execute` coordinates stream reads and atomic appends, and
projectors implement `Projector`. Store adapters live behind that boundary.

The optional experimental modeling lane is additive. With
`experimental-modeling`, derives create typed field occurrences, semantic
stream identities, builders, and runtime wrappers. A modeled command still
enters the unchanged `execute` function through `ModeledCommand`; a modeled
projection still enters the unchanged `Projector` contract through
`checked_projection`.

`experimental-model-check` is a test-only companion feature. It registers
metadata emitted by the same derives and `mapping!` invocations used at
runtime, then checks whether every modeled non-root field has an executable
provenance path. The checker is in-process, deterministic, has no I/O, and is
not included unless the feature is enabled.

This is information-completeness checking, not a proof of arbitrary business
formulas. Rust types and executable mappings establish the runtime coupling;
business policies remain ordinary tested command and projection logic.

The decision record is [ADR-033](docs/adr/ADR-033-experimental-runtime-coupled-model-checking.md).
