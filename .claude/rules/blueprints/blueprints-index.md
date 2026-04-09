# Blueprints

Technical documentation for this project's architecture and systems.

## When to Consult Blueprints

Before modifying system architecture, use Glob and Read on the `blueprints/`
directory to understand:

- Current design decisions and rationale
- Integration points and dependencies
- Established patterns to follow

## Key Triggers

Consult blueprints when working on:

- Event store backend implementations
- Command execution and retry logic
- Projection system and coordination
- Macro codegen (`#[derive(Command)]`, `require!`, `emit!`)
- Trait design (`CommandLogic`, `EventStore`, `Projector`)
- Testing infrastructure and contract tests

## After Modifications

Update blueprints using the Write tool on `blueprints/{name}.md` when you:

- Add new systems or major features
- Change architectural patterns
- Discover undocumented conventions

## Available Blueprints

<!-- AUTO-GENERATED INDEX - DO NOT EDIT BELOW THIS LINE -->

_No blueprints generated yet. Use `blueprints:generate-blueprints` to create
initial documentation._
