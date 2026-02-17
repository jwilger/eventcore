---
title: "Investigate: StreamVersion vs EventId for optimistic concurrency"
status: open
priority: 4
issue-type: task
created-at: "2026-02-17T07:49:26.963024-08:00"
---

[migrated from bead eventcore-aet, type: task]

Currently StreamVersion is a monotonic counter (0, 1, 2...) used for optimistic concurrency. However, since EventId is UUIDv7 (time-ordered), we could potentially use the last event's EventId as the 'version' for conflict detection.

**Questions to investigate:**
1. Would comparing EventIds provide equivalent conflict detection to comparing StreamVersions?
2. What are the trade-offs? (storage, comparison performance, semantics)
3. Could this simplify the API by removing StreamVersion entirely?
4. How do other event stores handle this? (EventStoreDB, Marten, etc.)

**Context:**
- StreamVersion currently starts at 0 for empty streams
- Each append increments version by 1
- EventId is UUIDv7 with embedded timestamp + randomness
- Conflict detection compares expected vs actual version
