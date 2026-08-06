---
name: macro-codegen
summary: Procedural macros generating CommandStreams implementations and business rule validation helpers.
---

## Experimental modeled macro registration

When `experimental-model-check` is enabled, modeled derives and `mapping!`
emit `inventory::submit!` through an EventCore-owned hidden macro. Without
that feature the same hook expands to nothing, so production modeling builds
contain no checker registration metadata.

# Macro Codegen

Compile-time code generation that eliminates command boilerplate. The `#[derive(Command)]` macro generates `CommandStreams` implementations from struct field annotations, while `require!` provides ergonomic business rule validation.

## Overview

The macro system separates infrastructure concerns (stream declarations) from domain logic (apply/handle). Developers annotate command struct fields with `#[stream]` and the macro generates the `CommandStreams` trait implementation automatically.

## Architecture

### `#[derive(Command)]`

**Input:** A struct with `#[stream]`-annotated `StreamId` fields.

**Output:** A `CommandStreams` implementation that returns `StreamDeclarations` from the annotated fields.

```rust
#[derive(Command)]
struct TransferMoney {
    #[stream]
    source_account: StreamId,
    #[stream]
    dest_account: StreamId,
    amount: u64,
}

// Generates:
impl CommandStreams for TransferMoney {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::try_from_streams(vec![
            self.source_account.clone(),
            self.dest_account.clone(),
        ]).expect("stream declarations validated at construction")
    }
}
```

**Validation (compile-time errors):**

- At least one `#[stream]` field required
- `#[stream]` fields must be `StreamId` type (supports qualified paths)
- Only named structs (no tuple or unit structs)
- `#[stream]` attribute takes no parameters

### `require!` Macro

Ergonomic early-return for business rule validation in `handle()` methods:

```rust
require!(balance >= amount, "insufficient-funds");
// Expands to:
if !(balance >= amount) {
    return Err(Into::<CommandError>::into("insufficient-funds"));
}
```

The error argument is anything implementing `Into<CommandError>`. Typed error
enums (with a `From<MyError> for CommandError` impl) are preferred over string
literals.

Supports format strings: `require!(cond, "need {}, have {}", need, have)`

Events are produced by returning `NewEvents` from `handle()`; there is no
`emit!` macro.

## Files

| File                                                  | Description                                    |
| ----------------------------------------------------- | ---------------------------------------------- |
| `eventcore-macros/src/lib.rs`                         | `#[derive(Command)]` proc macro implementation |
| `eventcore/src/lib.rs`                                | `require!` macro definition                    |
| `eventcore-macros/tests/trybuild.rs`                  | Compile-time error test harness                |
| `eventcore-macros/tests/command_macro.rs`             | Compile-time coverage tests                    |
| `eventcore-macros/tests/command_derive_macro_test.rs` | Runtime integration tests                      |
| `eventcore-macros/tests/require_macro_test.rs`        | require! macro tests                           |

## Related Systems

- [command-execution](command-execution.md) — Commands that macros generate code for
- ADR-006: Command macro design (infrastructure separate from domain logic)
