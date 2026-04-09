# Cargo Dependency Management

Dependencies are managed through `cargo add` / `cargo remove` for member
crates and `cargo-autoinherit` for workspace promotion.

## Adding a New Dependency

1. Run `cargo add <dep> --package <crate>` — this resolves the latest
   compatible version and sets feature flags correctly.
2. The pre-commit hook runs `cargo autoinherit`, which automatically promotes
   the dependency to `[workspace.dependencies]` and rewrites the member to use
   `dep.workspace = true`.
3. No manual editing of any `Cargo.toml` is needed for adding dependencies.

## Removing a Dependency

Run `cargo remove <dep> --package <crate>`. Cargo automatically cleans up
stale entries from `[workspace.dependencies]` when the last member reference
is removed.

## Updating a Workspace Dependency Version

Edit the version in `[workspace.dependencies]` in the root `Cargo.toml`
directly. This is the one case where hand-editing is required — no CLI tool
supports updating workspace dependency versions. Run `cargo check` afterward
to verify compatibility.

## Modifying Features

Use `cargo add <dep> --package <crate> --features <f>` to add features to a
member's dependency. Per-member features stay in the member's `Cargo.toml`
alongside `workspace = true`.

## Rules

1. **Do not specify a version** in `cargo add` unless there is a conflict with
   the latest available version and the human operator explicitly agrees.
2. **Never hand-edit member `Cargo.toml` dependency sections** — use
   `cargo add` / `cargo remove` exclusively.
3. **Hand-editing root `Cargo.toml`** is permitted only for updating versions
   or feature flags on existing `[workspace.dependencies]` entries.
4. **`cargo-autoinherit`** must be installed (`cargo install --locked cargo-autoinherit`).
   The pre-commit hook enforces workspace promotion automatically.

## Pre-Commit Hook

The lefthook pre-commit hook runs `cargo autoinherit` on any commit that
touches `Cargo.toml` files. This ensures all dependencies are promoted to
workspace scope. If a dependency cannot be promoted (incompatible sources
across members), the hook will leave it in the member's `Cargo.toml`.

## Why

Direct edits to member `Cargo.toml` files bypass Cargo's version resolution
and can introduce incompatible versions. `cargo-autoinherit` ensures
workspace-level deduplication happens automatically, removing the manual
coordination step.
