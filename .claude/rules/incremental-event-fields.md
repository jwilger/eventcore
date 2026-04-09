# Incremental Event Field Addition

Event variants are built incrementally as tests demand fields, not all at once
from the target design. This rule governs how fields are added over time.

## Adding Fields to Existing Events

When a new feature needs a field on an event variant that was created by an
earlier change:

1. **Check if a serde default is reasonable.** Can the application replay old
   events without this field and behave correctly? If yes, add the field with
   `#[serde(default)]`.
2. **If no reasonable default exists**, the change is non-backwards-compatible.
   This is blocked on eventcore upcasting support (see ADR 0021). Flag it to
   the user.

## What "Reasonable Default" Means

- `Option<T>` defaulting to `None` — acceptable when `None` means "unknown at
  recording time"
- `Vec<T>` defaulting to empty — acceptable when empty means "none existed yet"
- A required value with no meaningful absence state — **not acceptable**, needs
  event versioning

## New Event Variants

New variants can be added freely — they don't affect deserialization of existing
events. Old events simply never match the new variant.

## Do Not Pre-Add Fields

The target API design describes the complete event model. Implementation is
incremental. Only add fields that current non-test code reads. The design is
the target, not the current schema.

## Reference

- ADR 0021: Event Schema Evolution and Upcasting
