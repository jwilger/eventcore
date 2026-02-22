# Mob Review Protocol

Full-team review process for the PR phase. Triggers after a pair completes their
TDD cycle and the code is ready for merge. Integrates the three-stage code review
(from the parent skill) with consent-based decision-making from Robert's Rules.

## Participants

The full team reviews every PR. The same consent-by-default Robert's Rules protocol
used in planning applies here. Members who lack relevant expertise for a given stage
should **stand aside** -- this is appropriate deference, not disapproval. The
accessibility specialist does not need to opine on database internals; the domain
SME does not need to review CSS.

## Stage Leadership

Each review stage has designated leads. Other team members may contribute to any
stage but are not required to.

| Stage                 | Focus                              | Led By                             |
| --------------------- | ---------------------------------- | ---------------------------------- |
| 1 -- Spec Compliance  | Does the code do what was asked?   | PM and domain SME                  |
| 2 -- Code Quality     | Is it clear, tested, maintainable? | Engineers NOT in the original pair |
| 3 -- Domain Integrity | Are domain boundaries respected?   | Domain SME with input from all     |

Stages run sequentially. A failure in an earlier stage blocks later stages, per the
parent skill's rules.

## Decision Categories

Feedback items are classified the same way as planning decisions:

| Category | Examples                        | Max Rounds | Quorum                      |
| -------- | ------------------------------- | ---------- | --------------------------- |
| Trivial  | Naming, formatting, minor style | 1          | 5 of 8 (consent-by-default) |
| Standard | Architecture, API changes       | 3          | 6 of 8                      |
| Critical | Security, breaking changes      | 5          | 8 of 8 (full quorum)        |

The facilitator classifies each feedback item. Trivial items adopt automatically
if no objection is raised within the round.

## Feedback Resolution

The original pair addresses all feedback using their existing ping-pong process.
If feedback requires significant rework, pair rotation rules still apply -- the
pairing history constraint (no repeat of last 2 pairings) may assign a new pair.

## Re-review

After feedback is addressed, only affected reviewers re-review their specific
flagged concerns. Full re-review of all stages is not required unless any change
was classified as critical-category.

## Context Budget

During mob review, team members use their **compressed active-context persona**
(<500 tokens), not full profiles. Full profiles are loaded only for the engineers
doing the fix work. This keeps the review phase lightweight on context while
preserving persona-consistent judgment.

## Verification

After completing a mob review, verify:

- [ ] All three stages were performed sequentially with designated leads
- [ ] Stand-asides were recorded, not treated as blocks
- [ ] Each feedback item was classified by decision category
- [ ] Trivial items used consent-by-default (1 round)
- [ ] Critical items achieved full quorum before adopting
- [ ] Original pair addressed feedback via ping-pong process
- [ ] Re-review was scoped to flagged concerns only (unless critical)
- [ ] Team members used compressed persona forms during review
