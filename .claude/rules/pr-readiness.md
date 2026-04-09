# PR Readiness

Before creating a pull request, ensure the branch is up to date with `main`.

## Required Steps

1. Fetch the latest `main`: `git fetch origin main`
2. Rebase onto `origin/main`: `git rebase origin/main`
3. Resolve any conflicts if they arise
4. Force-push the rebased branch (with user confirmation, per standard safety protocol)
5. Only then create the PR

## Why

Stale branches cause merge conflicts, broken CI, and wasted reviewer time.
Rebasing before opening a PR ensures the diff is clean and the branch builds
against the current state of the codebase.
