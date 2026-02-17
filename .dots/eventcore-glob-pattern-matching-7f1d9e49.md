---
title: Glob Pattern Matching for Subscriptions
status: open
priority: 3
issue-type: task
created-at: "2026-02-17T07:49:26.960686-08:00"
---

[migrated from bead eventcore-ihm, type: feature]

Implements POSIX glob pattern matching for SubscriptionQuery per ADR-017 and ARCHITECTURE.md v1.6. Enables filtering subscriptions using wildcard patterns like `account-*` or `order-[0-9]*`.

**Core Components:**
- StreamPattern type that permits glob metacharacters (*, ?, [, ])
- SubscriptionQuery::filter_stream_pattern(StreamPattern) method
- POSIX glob matching via `glob` crate (or similar)

**Design Decisions (from ARCHITECTURE.md):**
- Distinct type: StreamPattern vs StreamPrefix (literals) - type system makes intent explicit
- Reserved characters in StreamId/StreamPrefix enable unambiguous pattern matching
- POSIX glob over regex: simpler, sufficient, safer (no catastrophic backtracking)

**Depends on:**
- [deleted:eventcore-017] (subscription foundation) - must be complete
- StreamId/StreamPrefix character restrictions - must be implemented first

## Acceptance Criteria
Feature: Developer filters subscriptions with glob patterns

Scenario: Developer creates StreamPattern with wildcards
  Given developer imports StreamPattern
  When developer writes StreamPattern::new("account-*")
  Then pattern is created successfully
  And pattern contains the wildcard character

Scenario: Developer filters subscription by glob pattern
  Given InMemoryEventStore with streams: account-123, account-456, order-789
  When developer calls subscribe(SubscriptionQuery::all().filter_stream_pattern(StreamPattern::new("account-*")))
  Then only events from account-123 and account-456 are delivered
  And events from order-789 are filtered out

Scenario: Developer uses single-character wildcard
  Given streams: user-a, user-b, user-ab, admin-x
  When developer filters with StreamPattern::new("user-?")
  Then only user-a and user-b match (single char after prefix)
  And user-ab does not match (two chars)

Scenario: Developer uses character class
  Given streams: order-1, order-2, order-a, order-b
  When developer filters with StreamPattern::new("order-[0-9]")
  Then order-1 and order-2 match
  And order-a and order-b do not match

Scenario: Developer distinguishes pattern from prefix
  Given StreamPrefix for literal matching
  And StreamPattern for wildcard matching
  When developer attempts StreamPrefix::new("account-*")
  Then error is returned (glob chars forbidden in prefix)
  When developer uses StreamPattern::new("account-*")
  Then pattern is created successfully
