# Architecture Decision Records

This directory contains Architecture Decision Records (ADRs) for this project.

## Index

| #   | Title                                                   | Status     |
| --- | ------------------------------------------------------- | ---------- |
| 001 | Multi-Stream Atomicity                                  | Accepted   |
| 002 | Event Store Trait Design                                | Accepted   |
| 003 | Type System Patterns                                    | Accepted   |
| 004 | Error Handling Hierarchy                                | Accepted   |
| 005 | Event Metadata Structure                                | Accepted   |
| 006 | Command Macro Design                                    | Accepted   |
| 007 | Optimistic Concurrency Control                          | Accepted   |
| 008 | Command Executor Retry Logic                            | Accepted   |
| 009 | Stream Resolver Design                                  | Accepted   |
| 010 | Free Function API Design                                | Accepted   |
| 011 | In-Memory Event Store Crate Location                    | Accepted   |
| 012 | Event Trait for Domain-First Design                     | Accepted   |
| 013 | EventStore Contract Testing                             | Accepted   |
| 014 | Queue-Based Stream Discovery                            | Accepted   |
| 015 | Testing Crate Scope                                     | Accepted   |
| 016 | Subscription Model                                      | Superseded |
| 017 | StreamId Reserved Characters                            | Accepted   |
| 018 | Subscription Error Handling                             | Superseded |
| 019 | Projector Trait                                         | Superseded |
| 020 | Subscribable Trait Design                               | Superseded |
| 021 | Poll-Based Projector Trait                              | Accepted   |
| 022 | Crate Reorganization for Feature Flags                  | Accepted   |
| 023 | Projector Coordination                                  | Superseded |
| 024 | Projector Configuration                                 | Superseded |
| 025 | Release Management and Versioning Policy                | Accepted   |
| 026 | Subscription Table Coordination                         | Accepted   |
| 027 | Projector Poll and Retry Configuration                  | Accepted   |
| 028 | Advisory Lock Acquisition Behavior                      | Accepted   |
| 029 | Projection Runner API Simplification                    | Accepted   |
| 030 | Layered Crate Public API Design                         | Accepted   |
| 031 | Black-Box Integration Testing via Projections           | Accepted   |
| 032 | Integration Test Crate for End-to-End Testing           | Accepted   |
| 033 | Experimental Runtime-Coupled Model Checking             | Superseded in part |
| 034 | Stable Releases for Experimental Features              | Accepted   |
| 035 | Event Schema Evolution via Enum Variants                | Accepted   |
| 036 | Continuous Polling via Projection Runner                | Superseded |
| 037 | Projection Configuration via Free Function API          | Accepted   |
| 038 | File-Based Event Store Format and Atomicity             | Accepted   |
| 039 | Read-Time Linearization and StreamVersion as Projection | Accepted   |
| 040 | File-Store Locking and Projector Coordination           | Accepted   |
| 041 | Merge Causality via Transaction DAG                     | Accepted   |
| 042 | Domain-Owned Reconciliation API                         | Accepted   |
| 043 | Projection Behavior After Structural Merge              | Accepted   |
| 044 | Replica Identity for Merge Mode                         | Accepted   |
| 045 | Merge Mode Outside the EventStore Trait                 | Accepted   |
| 046 | Git Integration Contract for the File Store             | Accepted   |
| 047 | Glob Pattern Matching for Subscriptions                 | Accepted   |
| 049 | Streaming Reads for EventStore::read_stream             | Accepted   |

## Creating an ADR

Use the `/adr` command to create a new ADR:

```
/adr new
```

## ADR Lifecycle

- **Proposed**: Initial draft, open for discussion
- **Accepted**: Approved and active
- **Superseded**: Replaced by newer ADR
- **Deprecated**: No longer recommended
