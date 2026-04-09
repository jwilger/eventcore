---
globs: crates/**/core/**/*.rs,crates/**/shell/**/*.rs
---

# Commands Must Use eventcore Traits

All commands in the system MUST implement the `eventcore` crate's traits.
Standalone `apply`/`handle` functions that mimic the pattern without trait
implementation are not acceptable.

## Required Trait Implementations

Every command module must:

1. **Implement `CommandLogic`** with associated types for `State` and `Event`
   - `apply()` folds events into command-local state (pure)
   - `handle()` validates preconditions and produces new events (pure)

2. **Implement `CommandStreams`** to declare consistency boundaries
   - `stream_declarations()` returns the event streams this command reads

## Shell Must Use eventcore::execute()

The imperative shell MUST call `eventcore::execute()` to run commands. The
shell must NOT:

- Manually fold events with `.iter().fold(State::default(), apply)`
- Manually append events with `.push()` or `.extend()`
- Bypass optimistic concurrency by managing the event store directly

## Canonical Example

Refer to the `eventcore` crate documentation (`cargo doc -p eventcore --open`)
for trait definitions and usage examples on `CommandLogic`, `CommandStreams`,
and `execute()`.

## Why

Manual fold/apply/handle functions look similar to CommandLogic but:

- Skip stream declarations (no type-safe consistency boundaries)
- Skip optimistic concurrency control
- Cannot be wired to `eventcore::execute()` or the event store
- Create inconsistency with crates that do use the traits (sm_licensing)
- Require rewriting when persistence is added

## All Events Must Originate From Commands

Every event appended to the event store must be produced by a command's
`handle()` method and appended via `eventcore::execute()`. The shell must
not directly construct events and append them to the store.

This includes startup events. If the system needs to record that the
server started, create a command for it (e.g., `StartSetupServer`) whose
`handle()` produces the `SetupServerStarted` event.

## No Shared Apply Functions

The `apply()` method inside `CommandLogic` is part of the write model.
Do not expose it as a public function for shell-level reads. See
`cqrs-model-separation.md` for details.
