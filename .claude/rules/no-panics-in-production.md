# No Panics in Production Code

Production code must not use `expect()`, `unwrap()`, or any other
panic-inducing pattern. Use `Result` propagation instead.

## What This Covers

- `expect()` and `unwrap()` on `Option` and `Result`
- `panic!()` macro
- `unreachable!()` without proof the branch is truly unreachable
- Array indexing without bounds checking

## Acceptable Uses

- **Test code** (`#[cfg(test)]` modules): `expect()` and `unwrap()` are fine
  in tests where a failure indicates a broken test, not a production issue.
- **Static initialization**: `OnceLock::get_or_init` with hardcoded valid
  values (e.g., `StreamId::try_new("setup/session")`) where the input is
  a compile-time constant that cannot fail.
- **Process startup before serving**: If a precondition failure means the
  process cannot start at all and should exit immediately, use
  `unwrap_or_else(|e| { eprintln!(...); std::process::exit(1) })` instead
  of `expect()`. Panics produce ugly stack traces; explicit exits produce
  clear error messages.

## What to Do Instead

```rust
// Wrong: panics at runtime
let value = some_fallible_operation().expect("should succeed");

// Right: propagate the error
let value = some_fallible_operation()?;

// Right: handle with a meaningful response
let value = match some_fallible_operation() {
    Ok(v) => v,
    Err(e) => return Err(e.into()),
};
```

## Why

Panics in production code crash the process. In a self-hosted product,
this means the customer's service goes down. Every fallible operation
must have an explicit error handling path that produces a recoverable
error or a graceful shutdown — not an uncontrolled crash.
