---
title: Fix ARCHITECTURE.md Event trait definition
status: open
priority: 2
issue-type: task
created-at: "2026-02-17T07:49:26.946502-08:00"
---

[migrated from bead eventcore-6am, type: bug]

**Location:** `docs/ARCHITECTURE.md:374-376`

**Issue:** The Event trait documentation shows an incomplete trait definition:

```rust
pub trait Event: Clone + Send + 'static {
    fn stream_id(&self) -> &StreamId;
}
```

**Actual trait (src/command.rs:202):**
```rust
pub trait Event: Clone + Send + Serialize + DeserializeOwned + 'static {
    fn stream_id(&self) -> &StreamId;
    fn event_type_name(&self) -> EventTypeName;
    fn all_type_names() -> Vec<EventTypeName>;
}
```

**Fix:** Update ARCHITECTURE.md to show complete trait definition including:
- `Serialize + DeserializeOwned` bounds
- `event_type_name()` method
- `all_type_names()` method
