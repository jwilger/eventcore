# ADR-034: Stable Releases for Experimental Features

## Status

Accepted

## Context

ADR-025 established generated release PRs as the review gate before crates.io
publication. ADR-033 instead used a prerelease version to communicate that the
runtime-coupled model checker was experimental. In practice, prerelease version
selection is a separate manual policy layered on top of release-plz's normal
semantic-version calculation, and `release_always = true` permits a version
already present on `main` to publish before its generated release PR is merged.

Experimental APIs already have a more direct opt-in boundary: Cargo feature
flags. A feature name can communicate its stability without changing the
release channel for every other crate and API in the lockstep workspace.

## Decision

EventCore publishes standard `X.Y.Z` semantic versions. Prerelease suffixes are
not used to distribute experimental functionality.

Public experiments must be disabled by default and exposed only through feature
flags whose names begin with `experimental-`. While a flag retains that prefix,
its public API is outside EventCore's stable compatibility guarantee: opting in
accepts that the API may change or be removed without a major version bump. A
follow-up ADR is required before removing the prefix, enabling the feature by
default, or otherwise declaring the API stable.

Set `release_always = false` in release-plz. Pushes to `main` may create or
refresh a generated release PR, but crates.io publication, tags, and GitHub
releases occur only after a `release-plz-*` release PR is merged. Stable GitHub
releases are production releases and may be marked as latest.

The release workflow exposes an optional exact workspace-version override for
manual runs. It delegates the override to `release-plz set-version` before
`release-plz release-pr`, preserving release-plz ownership of manifests,
internal dependency updates, changelogs, and release-PR metadata. Leaving the
input empty retains normal semantic-version inference.

This decision supersedes ADR-033's prerelease-only distribution decision and
clarifies ADR-025's two-phase publication policy. The already-published
`1.1.0-alpha.1` artifacts remain immutable historical releases; subsequent
EventCore releases use standard versions.

## Consequences

- Experimental and stable APIs can ship together without putting the entire
  workspace on a prerelease channel.
- Consumers must explicitly enable experimental features and cannot assume
  semver compatibility for APIs exposed only by those features.
- Every irreversible publication is previewed and gated by a generated release
  PR, even though release automation still runs after ordinary pushes to main.
- Documentation must identify experimental stability through feature names and
  warnings rather than exact prerelease pins.
