# EventCore Technical Increment Plan

**Document Version:** 4.0 (Beads Integration)
**Date:** 2025-10-22
**Project:** EventCore
**Phase:** 6 - Technical Increment Planning (Progressive Disclosure)
**Workflow:** Infrastructure Library Development

## Issue Tracking with Beads

**ALL issue tracking and increment management is done in beads (bd), NOT in this document.**

This document provides guidance on SDLC process and development philosophy. For actual work tracking:

- **View all increments**: `/beads:list` or `bd list`
- **View specific increment**: `/beads:show <issue-id>` or `bd show <issue-id>`
- **Find ready work**: `/beads:ready` or `bd ready`
- **View blocked items**: `/beads:blocked` or `bd blocked`
- **Update status**: `/beads:update <issue-id> --status <status>` or `bd update <issue-id> --status <status>`
- **Close completed work**: `/beads:close <issue-id>` or `bd close <issue-id>`
- **View project stats**: `/beads:stats` or `bd stats`

**For LLMs working with this codebase:**

- Use beads MCP tools for all issue tracking operations
- DO NOT maintain separate tracking in Markdown files
- Reference issue IDs (e.g., I-001) in commit messages and documentation
- Query beads for current status before starting work on an increment

**Version 4.0 Changes:**

- MIGRATED all increment tracking to beads issue database
- THIS FILE now contains only SDLC process guidance, NOT work items
- All 16 increments (I-001 through I-016) exist in beads with detailed design notes

## Overview

This document outlines the technical increment planning process for developing EventCore, a type-safe event sourcing library implementing multi-stream atomicity with dynamic consistency boundaries.

**CRITICAL RESTRUCTURE:** This plan has been completely restructured from horizontal layers (all types, then all storage, then all commands) to **proper vertical slices** where each increment is an end-to-end feature testable from the library consumer's perspective.

**Core Value Proposition:** EventCore eliminates the artificial constraints of traditional event sourcing by enabling atomic multi-stream operations while maintaining type safety, strong consistency guarantees, and developer ergonomics.

**Development Philosophy:**

- **Type-Driven Development:** Types enforce domain constraints at compile time FROM INCREMENT 1
- **Correctness Over Performance:** Multi-stream atomicity is non-negotiable
- **Infrastructure Neutrality:** Library, not framework - no business domain assumptions
- **Developer Ergonomics:** Minimize boilerplate while maximizing type safety
- **Vertical Slices:** Each increment provides end-to-end value developers can integrate immediately

## Vertical Slice Principles

**What Makes This Different:**

1. **End-to-End from Library Consumer POV**
   - Each increment = something a developer can actually use in their application
   - Must be testable as integration test from consumer perspective
   - Must provide real, working functionality (even if limited)

2. **Domain Types and Error Handling Are NOT Features**
   - Validated domain types (nutype) included from increment 1
   - Proper error handling (thiserror, Result types) included from increment 1
   - These are foundational discipline, not optional "features to add later"
   - We don't write with String primitives and "add types later"

3. **Simplest Possible First Slice**
   - I-001: Developer can create single-stream command with validated types, execute it with proper error handling, events written to in-memory storage
   - Includes: StreamId, EventId validated types; Event/EventMetadata; structured errors; InMemoryEventStore; single-stream executor
   - Excludes: retry logic, multi-stream, PostgreSQL, macros, dynamic discovery (all come later)

4. **Each Subsequent Slice Adds ONE Capability**
   - I-002: Add automatic retry with sensible defaults (no configuration)
   - I-003: Add retry configuration and advanced observability
   - I-004: Add multi-stream atomic commands (THE core value prop)
   - I-005: Add PostgreSQL production backend
   - I-006: Add macro ergonomics
   - I-007: Add dynamic stream discovery
   - etc.

## Increment Organization

Increments are organized as **end-to-end vertical slices** that library consumers can integrate and use. All increments (I-001 through I-016) are tracked in beads.

**To view all increments with current status:**

```bash
bd list
```

**To view a specific increment with full details (design, acceptance criteria, dependencies):**

```bash
bd show <issue-id>
# Example: bd show I-001
```

**Increment Summary (see beads for current status and full details):**

- 16 increments total (I-001 through I-016)
- Priority 1: Core functionality (I-001 through I-005)
- Priority 2: Developer experience and projections (I-006 through I-009, I-013 through I-016)
- Priority 3: Advanced testing and performance (I-010 through I-012)

**Critical Principle:** Each increment is a complete vertical slice testable from library consumer perspective. Domain types and error handling are included from I-001 (not separate increments).

Each increment:

- Provides end-to-end functionality developers can integrate immediately
- Testable as integration test from consumer perspective
- Includes all infrastructure needed (types, errors, storage, execution)
- Builds incrementally on previous work

---

## Increment Details

**All increment details (purpose, design, acceptance criteria, dependencies) are stored in beads.**

Use `/beads:show <issue-id>` or `bd show <issue-id>` to view full increment specifications.

For historical reference, the original detailed increment specifications existed in this file through version 3.0 (2025-10-14).

## Implementation Roadmap

### Phase 1: Core Functionality (Weeks 1-4)

- **I-001: Single-Stream Command End-to-End** (Week 1-2) - 🎯 COMPLETE WORKING SYSTEM (no retry)
- **I-002: Automatic Retry with Defaults** (Week 2) - Add automatic retry without configuration
- **I-003: Configurable Retry Policies** (Week 3) - Add retry configuration and observability
- **I-004: Multi-Stream Atomicity** (Week 4) - 🎯 CORE VALUE PROP DELIVERED

**Milestone:** Core value proposition (multi-stream atomicity) fully implemented with in-memory backend.

### Phase 2: Production Readiness (Weeks 5-7)

- **I-005: PostgreSQL Production Backend** (Week 5-6) - 🎯 PRODUCTION READY
- **I-006: Command Derive Macro** (Week 7) - Developer ergonomics
- **I-007: Dynamic Stream Discovery** (Week 7) - Advanced workflows

**Milestone:** Library ready for production use with excellent ergonomics.

### Phase 3: Advanced Features (Weeks 8-11)

- **I-008: Basic Event Subscriptions** (Week 8) - Subscribe and process events (no checkpointing)
- **I-009: Checkpointing for Subscriptions** (Week 9) - Add checkpoint/resume capability
- **I-010: Chaos Testing Infrastructure** (Week 9) - Failure injection for robust testing
- **I-011: Performance Benchmarking** (Week 10) - Establish baselines and track regressions
- **I-012: Snapshot Support** (Week 11) - Optimize based on benchmark data

**Milestone:** Advanced capabilities for complex scenarios with data-driven optimization.

### Phase 4: Developer Experience Polish (Weeks 12-14)

- **I-013: require! Macro** (Week 12) - Ergonomic business rule validation
- **I-014: emit! Macro** (Week 12) - Type-safe event emission with phantom types
- **I-015: Documentation Completeness Audit** (Week 13) - Ensure doc quality across increments
- **I-016: Error Message Consistency Audit** (Week 14) - Ensure error quality across increments

**Milestone:** Library ready for public release with excellent developer experience and consistent quality.

---

## Success Criteria

### Developer Experience

- ✅ New developer implements first command in under 30 minutes
- ✅ Typical command requires fewer than 50 lines of code (with macro)
- ✅ Type errors provide clear guidance on fixes
- ✅ Documentation examples are copy-paste ready

### Correctness

- ✅ Multi-stream atomicity verified via concurrent integration tests
- ✅ Version conflicts detected 100% of the time
- ✅ No data corruption possible under any failure scenario
- ✅ Retry logic eventually succeeds or fails clearly

### Performance

- ✅ Single-stream throughput adequate for business operations (50+ ops/sec)
- ✅ Multi-stream operations maintain correctness at scale
- ✅ Memory usage remains bounded under load
- ✅ No memory leaks under sustained operation

### Adoption

- ✅ API intuitive to Rust developers familiar with async
- ✅ Examples cover common use cases (banking, e-commerce)
- ✅ Community contributions feasible via clear extension points
- ✅ Error messages enable self-service problem resolution

---

## References

### Core Documentation

- **REQUIREMENTS_ANALYSIS.md:** Functional and non-functional requirements (FR-1 through FR-6, NFR-1 through NFR-5)
- **ARCHITECTURE.md:** System design and component interactions
- **CLAUDE.md:** Project philosophy and development patterns

### Architectural Decision Records

- **ADR-001:** Multi-Stream Atomicity Implementation Strategy
- **ADR-002:** Event Store Trait Design
- **ADR-003:** Type System Patterns for Domain Safety
- **ADR-004:** Error Handling Hierarchy
- **ADR-005:** Event Metadata Structure
- **ADR-006:** Command Macro Design
- **ADR-007:** Optimistic Concurrency Control Strategy
- **ADR-008:** Command Executor and Retry Logic
- **ADR-009:** Stream Resolver Design for Dynamic Discovery

### Process Documentation

- **~/.claude/processes/STORY_PLANNING.md:** Planning methodology (adapted for library development)
- **~/.claude/processes/DOCUMENTATION_PHILOSOPHY.md:** WHAT/WHY not HOW principles
- **~/.claude/processes/INTEGRATION_VALIDATION.md:** Testing and verification requirements

---

## Key Principles for Implementation

### Vertical Slice Discipline

1. **Each increment must be independently valuable** - Provides complete, usable functionality
2. **Integration tests are mandatory** - Test from library consumer perspective
3. **Type-driven development throughout** - Invalid states unrepresentable at compile time
4. **No horizontal layering** - Don't build "all types" then "all storage" then "all commands"
5. **Include infrastructure from day 1** - Types, errors, storage in I-001 (not separate increments)

### Testing Strategy

- **Integration tests:** Complete command execution from consumer perspective
- **Property tests:** Invariant verification across random inputs
- **Concurrent tests:** Multi-stream atomicity under concurrent load (NO partial state)
- **Chaos tests:** Failure injection to verify error handling
- **Performance tests:** Benchmarks establishing baselines
- **Real backends:** PostgreSQL tests via Docker (not mocks)

### Common Pitfalls to Avoid

- **Premature abstraction:** Start concrete (I-001), abstract later when patterns emerge
- **Horizontal layering:** Each increment must be end-to-end testable
- **Deferring types/errors:** Include from increment 1 (not "add later")
- **Skipping integration tests:** Must test from consumer perspective
- **Missing manual verification:** Document how developer would actually use this

---

**Document Status:** Version 3.0 - Progressive Disclosure Restructure Complete
**Key Improvements:**

- Subscription complexity split for learning curve (I-008 basic → I-009 checkpointing)
- Snapshot optimization data-driven (after I-011 benchmarks)
- Macro complexity progressive (I-013 simple require! → I-014 complex emit!)
- Documentation/error quality built-in from start, audited at end

**Next Steps:** Begin I-001 implementation with complete end-to-end single-stream command execution
