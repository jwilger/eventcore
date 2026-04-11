# No Rewriting Pushed History

Never amend, squash, or rebase commits that have already been pushed
to a remote branch unless the user explicitly asks for it.

## What This Covers

- `git commit --amend` on pushed commits
- `git reset --soft HEAD~N && git commit` (squash) on pushed commits
- `git rebase -i` on pushed commits
- Any operation that changes commit SHAs already on the remote

## What To Do Instead

If a commit needs correction after pushing:

1. **Create a new commit** with the fix
2. Push the new commit normally
3. The PR will be squash-merged anyway, so multiple commits are fine

## When Rewriting Is Allowed

Only when the user explicitly says something like:

- "squash those commits"
- "amend the last commit"
- "rebase and force push"
- "clean up the commit history"

## Why

Force-pushing rewrites shared history. Even on feature branches, it
can disrupt reviewers who have already fetched the branch, invalidate
review comments tied to specific commits, and lose context about how
the code evolved.
