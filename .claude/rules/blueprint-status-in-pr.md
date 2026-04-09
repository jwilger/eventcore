# Blueprint Status Updates Belong in the Implementing PR

When a PR implements a blueprint slice, update the slice's status marker in
the blueprint file **within that same PR** — not after merge.

## Rule

Before marking a slice PR as ready for review:

1. Update the slice's status from `<!-- status: pending -->` or
   `<!-- status: in-progress -->` to `<!-- status: implemented -->` in the
   blueprint file
2. Include that change in a commit on the PR branch

## Why

The main branch has branch protection requiring PRs. If the status update is
deferred until after merge, it requires a separate PR for a one-line change.
Worse, there's a window where the code is merged but the blueprint still shows
the wrong status.

Updating the status in the implementing PR keeps the blueprint and code in
sync atomically.
