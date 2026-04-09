# Design System Review Before UI Implementation

Every slice that includes user-facing UI interaction must undergo a design
system review BEFORE Phase 4 (Implementation) begins. This is a gate, not a
suggestion.

## When This Applies

Any slice where the user interacts with the application through a browser:
filling forms, viewing status, navigating between pages, receiving feedback.

This does NOT apply to CLI-only slices or pure backend/event-sourcing slices
with no UI component.

## What the Review Must Answer

1. **Which sm_ui components are needed** for this slice's screens?
2. **Do those components already exist** in sm_ui at the required Atomic
   Design level (atom, molecule, organism, template, page)?
3. **What new components must be created**, and at which level?
4. **Are design tokens sufficient** for the needed styling, or do new tokens
   need to be added to `quarks/tokens.rs`?
5. **Does the page template exist**, or does a new template need to be created?

## Recording the Review

Record the review output as:

- A comment on the GitHub Issue for the slice, OR
- A section in the technical plan if Phase 3 produces one

## Timing

This gate applies between Phase 3 and Phase 4. If Phase 3 is skipped (with
user confirmation), the design system review still must happen before
implementation begins.

## No Ad-Hoc UI in sm_app

Route handlers in sm_app MUST compose pages from sm_ui components. All visual
design, markup structure, and styling decisions live in sm_ui. Handlers in
sm_app are responsible for:

- Parsing requests
- Running domain logic
- Selecting which sm_ui page/component to render
- Passing domain data to sm_ui components

They are NOT responsible for HTML structure, CSS classes, or layout decisions.

## Why

Without this gate, implementers create ad-hoc HTML in route handlers, bypass
the design system, and produce inconsistent UI. The design system exists to
prevent this; the gate ensures it is consulted before code is written.
