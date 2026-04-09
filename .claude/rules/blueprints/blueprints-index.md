# Blueprints

Technical documentation for this project's architecture and systems.

## When to Consult Blueprints

Before modifying system architecture, use Glob and Read on the `blueprints/` directory to understand:

- Current design decisions and rationale
- Integration points and dependencies
- Established patterns to follow

## Key Triggers

Consult blueprints when working on:

- GraphQL schema changes
- CLI command modifications
- MCP server integrations
- Plugin architecture changes
- Database schema updates
- Hook system modifications

## After Modifications

Update blueprints using the Write tool on `blueprints/{name}.md` when you:

- Add new systems or major features
- Change architectural patterns
- Discover undocumented conventions

## Available Blueprints

<!-- AUTO-GENERATED INDEX - DO NOT EDIT BELOW THIS LINE -->

| Blueprint                   | Summary                                                                                                                                                   |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| architectural-invariants    | The 26 enforceable architectural rules that every other blueprint references.                                                                             |
| database-encryption         | Envelope key hierarchy for SQLCipher database encryption with HKDF derivation and license-JWT-derived KEK wrapping.                                       |
| deployment-and-web-stack    | Single-tenant self-hosted web application with Tokio/Axum, Maud templates, Tailwind CSS v4, and HTMX.                                                     |
| design-system               | Dark-first Atomic Design system for server-rendered Rust UI with OKLCH tokens, Tailwind v4, and WCAG AA accessibility.                                    |
| domain-model                | Domain overview covering vendor-admin and customer-runtime boundaries, actors, invariants, and workflow inventory.                                        |
| event-sourcing              | Aggregateless event sourcing with CQRS and multi-database per-project persistence architecture.                                                           |
| functional-core-and-effects | Functional core / imperative shell architecture with algebraic effects via the trampoline pattern.                                                        |
| installation-workflow       | Customer-side workflow for SM installation, bootstrap access via setup tokens, initial setup, admin authentication, and runtime configuration management. |
| license-issuance-workflow   | Vendor-side CLI workflow for customer registration, license issuance, renewal, supersession, and operator lookup via CQRS event model.                    |
| license-management          | Ed25519-signed JWT license system with offline verification and the sm_licensing vendor-side CLI.                                                         |
| product-brief               | Product definition for stochastic_macro — a commercial, self-hosted orchestration product for supervisor-managed coding agents.                           |
| testing-strategy            | Outside-in BDD testing with proof-based drill-down discipline across acceptance, unit, and property layers.                                               |
| type-system                 | Semantic domain types with the nutype macro enforcing parse-don't-validate at IO boundaries.                                                              |
| ui-architecture             | Atomic Design component architecture split between sm_app (runtime shell) and sm_ui (presentation layer) with property testing and route-based showcase.  |
| workflow-engine             | Deterministic workflow engine using typed graph definitions with trampoline-driven execution.                                                             |
