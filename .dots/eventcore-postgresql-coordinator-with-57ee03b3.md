---
title: PostgreSQL Coordinator with Contract Tests (TDD)
status: open
priority: 1
issue-type: task
created-at: "2026-02-17T07:49:26.937354-08:00"
---

[migrated from bead eventcore-che, type: feature]

Implement PostgreSQL Coordinator using advisory locks on dedicated connections per ADR-026. Follows the simple EventStore/Commanded pattern: subscriptions table tracks checkpoints, advisory locks on dedicated connections provide coordination.

## Core Pattern (ADR-026)

**Subscriptions Table** - Checkpoint tracking:
- Schema: (subscription_name TEXT PRIMARY KEY, last_position BIGINT, updated_at TIMESTAMPTZ)
- Purpose: WHERE did we process up to?
- Updated transactionally with projection writes

**Advisory Locks on Dedicated Connections** - Coordination:
- Each projector owns a dedicated connection (NOT from pool)
- Advisory lock acquired via pg_advisory_lock(hash(subscription_name))
- Lock held for session duration (connection open)
- Released automatically when connection closes
- NO heartbeat table needed
- NO validity checking needed

## Implementation Components

- PostgresCoordinator implementing ProjectorCoordinator trait
- Dedicated connection creation (not from pool)
- Advisory lock acquisition/release
- RAII guard for crash-safe leadership
- Subscriptions table migration

## Key Differences from ADR-023

- ✅ NO heartbeat table
- ✅ NO guard.heartbeat() method
- ✅ NO guard.is_valid() checks in projection loop
- ✅ Connection lifecycle = lock lifecycle = process lifecycle
- ✅ Simpler: One table, one lock, automatic release

## Acceptance Criteria

Scenario: First instance acquires leadership
  Given PostgresCoordinator with dedicated connection
  When coordinator.acquire("test-projection") is called
  Then leadership is granted
  And advisory lock is held on dedicated connection

Scenario: Second instance waits for leadership
  Given first instance holds leadership
  When second instance calls coordinator.acquire()
  Then second instance blocks until leadership available

Scenario: Crash releases leadership automatically
  Given first instance holds leadership
  When first instance connection closes (crash/shutdown)
  Then advisory lock released automatically (session-scoped)
  And second instance can acquire leadership

Scenario: Subscriptions table tracks checkpoints
  Given projector processing events
  When projector updates checkpoint in same transaction as projection
  Then subscriptions table row updated
  And updated_at timestamp reflects processing progress

Scenario: Different projectors have independent coordination
  Given coordinator for "balance-projection"
  And coordinator for "notification-projection"
  When both acquire leadership
  Then both succeed (different subscription names, different locks)
