# ADR-033: Experimental Runtime-Coupled Model Checking

## Status

Proposed

## Context

Formal model compilers can validate an Event Modeling diagram, but the proved
model can drift from the Rust application that runs in production. Their proof
cost also makes rapid, incremental modeling impractical. EventCore already
uses Rust's type system to validate much of the piece-by-piece construction of
a command and projection.

The remaining useful check is information completeness: each non-root field
in the model needs an explicit executable provenance path. This is distinct
from proving that a business formula is intended or economically correct.

## Decision

Add an additive, experimental modeled lane behind two non-default features:

```toml
experimental-modeling = ["macros", "eventcore-macros/experimental-modeling"]
experimental-model-check = ["experimental-modeling", "dep:inventory"]
```

The first feature supplies semantic stream identities, typed modeled builders,
`mapping!`, `ModeledCommand`, and a checked projection adapter. The wrappers
delegate to EventCore's stable command executor and projector traits; stable
APIs remain unchanged.

The second feature registers metadata from those same executable artifacts and
provides `check()` / `check_with()`. It reports deterministic diagnostics for
missing producers, duplicate registrations, unresolved sources, and ordinary
cycles. It treats explicit input and modeled default state as roots. The
checker is designed for test targets and contributes no release-binary code
when neither experimental feature is enabled.

Field occurrences, rather than a matrix of conversion methods, express
provenance; semantic Rust types continue to express domain roles. Macro
registration is automatic, normalized before analysis, and isolated behind
the checker feature. A custom SQL or native projection sink is an explicit
assumption boundary: strict checks reject it, and an opted-in named assumption
can produce only `Assumed`, never `Verified`.

This is an alpha experiment, not an official modeling direction. It will be
released only as `1.1.0-alpha.1` after validation in downstream projects; it
must not be published or marked as the latest stable release automatically.

## Consequences

- Runtime behavior and checked artifacts share builders and mappings, reducing
  model/implementation drift.
- The guarantee is deliberately narrower than formal theorem proving: it does
  not establish intended business formulas or external-system truth.
- Applications opt in explicitly and may continue using only stable APIs.
- The public experimental API can change before stabilization.
- A follow-up ADR is required before this lane is stabilized, enabled by
  default, or removed; that decision must incorporate trial-project feedback
  and the alpha verification results.
