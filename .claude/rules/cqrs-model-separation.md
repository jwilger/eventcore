# CQRS: Separate Read and Write Models

Read models (projections, queries) and write models (command state) must
be separate code paths, even when the current logic is identical.

## The Rule

- **Command state** (`CommandLogic::apply` + `CommandLogic::handle`) is
  the write model. It exists to validate preconditions and produce events.
- **Projections and queries** are the read model. They exist to present
  data to consumers or to inform application-level decisions.

These two concerns must not share functions, even if they currently fold
events the same way.

## Why Separation Matters

Write models and read models evolve independently:

- A write model may stop caring about a field that a read model still needs
- A read model may denormalize data that a write model keeps normalized
- Optimizing a read model (caching, indexing) must not affect write correctness

Sharing code between them creates hidden coupling. When one changes, the
other breaks.

## Applies To

- The `apply` function inside `CommandLogic` is write-model code. Do not
  expose it as a public function for consumer-level reads.
- Application code that needs to query state before executing a command must
  use a separate projection, not the command's apply.

## Example

```rust
// Wrong: application reuses the command's apply for reads
let state = events.iter().fold(State::default(), |s, e| command::apply(s, e));
let credential = state.stored_credential_for(&email);

// Right: application has its own projection
let credential = credential_projection::lookup(&events, &email);
```
