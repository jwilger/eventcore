---
globs: docs/adrs/**
---

# Architecture Decision Records

ADRs in this directory record architectural decisions and their rationale. This includes accepted, rejected, and superseded decisions — rejected and superseded ADRs are kept with their status set appropriately so we know what has already been considered and why it was not pursued.

## Format

Each ADR follows the standard format:

- Title with sequential number (NNNN-kebab-case-title.md)
- Status (accepted, rejected, superseded), Context, Decision, Consequences sections

## Reading ADRs

- ADR 0013 supersedes earlier frontend/packaging assumptions — read it before relying on older ADRs
- ADRs are historical records; the current truth lives in `blueprints/`
- Cross-reference ADRs from blueprint "Related Systems" sections

## Creating New ADRs

When making architectural decisions:

1. Use the next sequential number
2. Record the context, decision, and consequences
3. Update related blueprints to reference the new ADR
4. Link to the ADR from the relevant blueprint's "Related Systems" section
