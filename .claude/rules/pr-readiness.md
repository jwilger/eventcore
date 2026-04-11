# PR Readiness

Before creating a pull request, ensure the branch is up to date with `main`.

## Required Steps

1. Fetch the latest `main`: `git fetch origin main`
2. Merge `origin/main` into your branch: `git merge origin/main`
3. Resolve any conflicts if they arise
4. Push normally (no force push needed)
5. Only then create the PR

## Updating an Existing PR

When `main` has moved ahead of an open PR branch:

1. Fetch the latest `main`: `git fetch origin main`
2. Merge `origin/main` into the PR branch: `git merge origin/main`
3. Resolve any conflicts
4. Push normally

Do **not** rebase and force-push. Merge commits keep the history
intact, avoid invalidating review comments, and don't require force
pushes. The PR will be squash-merged anyway, so merge commits on
the branch don't affect the final history on `main`.

## Why

Stale branches cause merge conflicts, broken CI, and wasted reviewer
time. Merging `main` before opening (or updating) a PR ensures the
branch builds against the current state of the codebase without
rewriting history.
