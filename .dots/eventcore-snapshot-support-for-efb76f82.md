---
title: Snapshot Support for Performance
status: open
priority: 3
issue-type: task
created-at: "2026-02-17T07:49:26.955705-08:00"
---

[migrated from bead eventcore-012, type: feature]

Optimize state reconstruction for long-lived streams by periodically saving snapshots and starting reconstruction from snapshot instead of version 0. Comes AFTER eventcore-011 because we need performance data to determine if snapshots are necessary and what snapshot frequency makes sense.

## Acceptance Criteria
Feature: Developer optimizes long-lived streams with snapshots

Scenario: Developer uses benchmark data to decide on snapshots
  Given developer reviews I-011 benchmark results
  When stream reconstruction time exceeds threshold (e.g., 100ms for 10k events)
  Then developer enables snapshot support
  And chooses snapshot frequency based on performance data

Scenario: Developer creates snapshot of stream
  Given account stream has 10,000 events
  When snapshot is saved at version 10,000
  Then snapshot stores complete account state
  And snapshot size is documented

Scenario: Developer loads state from snapshot
  Given snapshot exists at version 10,000
  When command reads account stream
  Then executor loads snapshot as starting state
  And applies only events 10,001+ (incremental)
  And state reconstruction is dramatically faster

Scenario: Developer configures snapshot frequency
  Given developer sets snapshot interval to 1000 events (from benchmark guidance)
  When events are appended
  Then snapshots are created automatically at 1000, 2000, 3000...
  And reconstruction remains fast even for very old streams

Scenario: Developer measures snapshot impact
  Given developer runs benchmarks with and without snapshots
  When comparing reconstruction time for 50k event stream
  Then snapshot-enabled reconstruction is significantly faster
  And benchmark documents improvement (e.g., 500ms → 50ms)
