---
globs: docs/adr/**
---

# Architecture Decision Records

ADRs in this directory record architectural decisions and their rationale. This
includes accepted, rejected, and superseded decisions — rejected and superseded
ADRs are kept with their status set appropriately so we know what has already
been considered and why it was not pursued.

## Format

Each ADR follows the standard format:

- Title with sequential number (NNNN-kebab-case-title.md)
- Status (accepted, rejected, superseded), Context, Decision, Consequences
  sections

## Reading ADRs

- ADRs are historical records; the current truth lives in the code and
  blueprints (if they exist)
- When blueprints exist, cross-reference ADRs from blueprint "Related Systems"
  sections

## Creating New ADRs

When making architectural decisions:

1. Use the next sequential number
2. Record the context, decision, and consequences
3. If relevant blueprints exist, update them to reference the new ADR
