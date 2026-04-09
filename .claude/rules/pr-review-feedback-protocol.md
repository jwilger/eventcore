# PR Review Feedback Protocol

When processing PR review feedback from a human reviewer, classify each piece of
feedback before acting on it.

## Classification

For each review comment, determine its category:

### Category A: Guardrail Gap

The feedback reveals a systemic issue — either:

- An existing guardrail (in CLAUDE.md, `.claude/rules/`, the per-project
  Claude Code memory directory, REVIEW.md, or han plugin hooks) was not
  followed, OR
- No guardrail exists but one should, because the mistake reflects a repeatable
  pattern that the AI would likely make again in a different context

**Indicators:** reviewer says "you should always...", "this violates our...",
"we discussed this before", "check the rules about...", or the feedback matches
a documented invariant/convention that was missed.

### Category B: One-Off Fix

The feedback is specific to this implementation with no broader pattern —
a better algorithm choice, a naming preference, a missed edge case unique to
this feature, or a subjective style suggestion.

**Indicators:** the fix is localized, would not generalize to other code, and
no rule could reasonably prevent it.

## Protocol for Category A (Guardrail Gap)

Do NOT immediately fix the code. Instead, follow these steps in order:

### Step 1: Self-Reflect

Read all AI guardrails to understand why the mistake was made:

1. `CLAUDE.md` (project root)
2. All files in `.claude/rules/` (and subdirectories)
3. All memory files in the per-project Claude Code memory directory
4. `REVIEW.md` (project root)

Identify which guardrail was missed or which guardrail is missing. Write a brief
analysis (in the conversation, not in a file) explaining:

- What the reviewer caught
- Which existing guardrail should have prevented it (or that none exists)
- Why the AI did not follow it or why the gap exists
- What change to guardrails would prevent this category of mistake

### Step 2: Plan Guardrail Updates

Determine which files need to change. Possible targets:

- **New or updated `.claude/rules/` file** — for project-wide conventions
- **New or updated memory file** — for feedback-driven behavioral corrections
- **Updated `CLAUDE.md`** — for workflow or top-level rule changes
- **Updated `REVIEW.md`** — for review checklist additions

Present the plan to the user and get confirmation before proceeding.

### Step 3: Create Guardrails PR

1. Create a new branch based on `main` (name: `chore/guardrail-update-<short-description>`)
2. Implement the guardrail/memory updates from Step 2
3. Commit and push the changes
4. Create a PR for the guardrail changes
5. Tell the user the PR is ready for review

### Step 4: Iterate on Guardrails PR

If the user gives feedback on the guardrails PR, address it directly on that
branch. Do not return to the original PR until the guardrails PR is merged.

### Step 5: Prompt Restart

Once the guardrails PR is merged, tell the user:

> The guardrail updates have been merged. Please restart Claude Code so the new
> rules take effect before we address the original PR feedback.

Do not proceed until the user confirms they have restarted.

### Step 6: Fix the Original PR

After restart, return to the original PR branch and address the review feedback.
The updated guardrails now guide the fix and prevent the same category of
mistake.

## Protocol for Category B (One-Off Fix)

Fix the issue directly on the current PR branch. No guardrail update is needed.

## Mixed Feedback

When a review contains both Category A and Category B items, process all
Category A items first (they may result in a single combined guardrails PR),
then fix Category B items alongside the original PR fixes in Step 6.

## Replying to Review Threads

When a PR review contains inline comments (review threads), reply to EVERY
thread after addressing the feedback. This allows the reviewer to read the
response and resolve the thread.

- Use `get_review_comments` to retrieve threads and their comment IDs.
- Use `add_reply_to_pull_request_comment` (GitHub MCP tool) with the numeric
  comment ID to post replies to inline review threads.
- The reply should briefly explain what was changed, or acknowledge the feedback
  if it led to a guardrail update rather than an immediate code fix.
- Do NOT leave review threads without a reply — silent fixes force the reviewer
  to re-read the diff to confirm the issue was addressed.

## Ambiguous Cases

If unsure whether feedback is Category A or B, default to Category A. It is
better to over-invest in guardrail improvement than to repeat mistakes.
