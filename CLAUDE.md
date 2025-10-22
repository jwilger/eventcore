# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

EventCore is a type-safe event sourcing library implementing **multi-stream event sourcing** with dynamic consistency boundaries. Unlike traditional event sourcing that forces rigid aggregate boundaries, EventCore allows commands to atomically read from and write to multiple event streams.

This is an **infrastructure/library project** - the consumers are developers who will use this library in their applications.

## Key Design Philosophy

**Type-Driven Development:** EventCore follows strict type-driven development principles:

1. **Types first** - Design types that make illegal states unrepresentable
2. **Parse, don't validate** - Use smart constructors with `nutype`
3. **No primitive obsession** - Wrap primitives in domain types
4. **Total functions** - Handle all cases explicitly

Example pattern:

```rust
// Good: Domain type with validation
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
    derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Deref, Serialize, Deserialize)
)]
pub struct StreamId(String);
```

## Architecture

EventCore is structured as a workspace with the core library in the repository root:

- **eventcore/** (this workspace root) - Core library with traits, types, and patterns
- External packages (referenced but not in this repo):
  - `eventcore-postgres` - PostgreSQL adapter for production
  - `eventcore-memory` - In-memory adapter for testing
  - `eventcore-examples` - Complete working examples
  - `eventcore-macros` - Procedural macros (especially `#[derive(Command)]`)

### Core Concepts

1. **Commands**: Business operations that read from multiple streams and produce events
   - Use `#[derive(Command)]` macro to auto-generate boilerplate from `#[stream]` fields
   - Implement `CommandLogic` trait with `apply()` and `handle()` methods

2. **Events**: Immutable facts stored in streams
   - Commands produce events via `StreamWrite<StreamSet, Event>`
   - Use `emit!` macro for type-safe event emission

3. **Streams**: Ordered sequences of events identified by `StreamId`
   - Multi-stream commands can read/write multiple streams atomically
   - Dynamic stream discovery via `StreamResolver`

4. **State**: Reconstructed from events via `apply()` method
   - Each command defines its own state type
   - State accumulates events from all relevant streams

## Development Environment

### Setup

```bash
# Enter Nix development environment (installs all tools)
nix develop

# Start PostgreSQL (for integration tests if using postgres adapter)
docker-compose up -d
```

The Nix environment includes:

- Rust toolchain (version from rust-toolchain.toml)
- cargo-nextest (fast parallel test runner)
- cargo-mcp (MCP protocol support)
- PostgreSQL client tools
- Pre-commit hooks

### Testing

```bash
# Fast parallel tests (preferred)
cargo nextest run

# Standard test runner
cargo test

# Run specific test
cargo nextest run test_name
# or
cargo test test_name

# Run with output
cargo test -- --nocapture
```

### Code Quality

```bash
# Format code
cargo fmt

# Lint with clippy (must pass with no warnings)
cargo clippy --workspace --all-targets -- -D warnings
```

## Commit Standards

**GPG commit signing is REQUIRED.** All commits must be signed.

Commit messages should:

- Be under 50 chars for the summary line
- Include a detailed explanation focusing on WHY (not just what)
- Follow this format:

```
Short summary (max 50 chars)

Detailed explanation wrapped at 72 characters.
Focus on WHY the change was made, not just what changed.

Include any breaking changes, performance implications, or other
important notes.
```

See CONTRIBUTING.md for GPG setup instructions.

## Type Safety Patterns

### Handle Errors Explicitly

- Use `Result` types throughout
- Avoid `unwrap()` and `expect()` in library code
- Use `CommandError` for command failures
- Use `require!` macro for business rule validation

### Domain Types Over Primitives

Always wrap primitives in validated domain types:

- `StreamId` instead of `String`
- Custom newtypes for domain concepts
- Use `nutype` for validation at construction time

### Composition Over Classes

- Small, focused functions that compose
- Traits define behavior contracts
- No aggregate classes - just commands and events

## Security Checklist

Before submitting code:

- No hardcoded secrets or credentials
- All user input validated using `nutype` types
- SQL queries use parameterized statements (via `sqlx`)
- Error messages don't leak sensitive information
- No test credentials or PII in tests

## Documentation

- All public APIs must have doc comments
- Include examples in doc comments where helpful
- Explain "why" not just "what"
- Document invariants and assumptions
- User-facing docs go in `/docs`
- Example code goes in separate example crates

## Performance Notes

Current benchmarks (PostgreSQL backend):

- Single-stream commands: ~86 ops/sec
- Multi-stream commands: ~25-50 ops/sec (with full atomicity)
- Batch event writes: 9,000+ events/sec
- P95 latency: ~14ms

Optimized for correctness and multi-stream atomicity over raw throughput.
