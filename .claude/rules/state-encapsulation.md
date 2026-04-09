# State Types Encapsulate Their Fields

Command state types and projection types expose behavior through methods,
not through public fields.

## The Rule

- State struct fields are **private** (no `pub`)
- State behavior is accessed through methods that express domain intent
- External code never compares state fields against enum variants directly

## Why

Exposing fields forces callers to know the internal representation. When
the representation changes, every caller breaks. Methods hide the
representation and express what the caller actually wants to know.

## Pattern

```rust
// Wrong: pub fields, external comparison
pub struct MyState {
    pub setup_completed: SetupCompleted,
}

// In handle():
require!(state.setup_completed == SetupCompleted::Yes, "...");

// Right: private fields, method expresses intent
pub struct MyState {
    setup_completed: SetupCompleted,
}

impl MyState {
    pub fn is_setup_completed(&self) -> bool {
        self.setup_completed == SetupCompleted::Yes
    }
}

// In handle():
require!(state.is_setup_completed(), "...");
```

## When to Use Result-Returning Methods

When a state query is a precondition check that produces a specific error
on failure, the method can return `Result`:

```rust
impl MyState {
    pub fn require_setup_completed(&self) -> Result<(), MyError> {
        if self.setup_completed != SetupCompleted::Yes {
            return Err(MyError::SetupNotCompleted);
        }
        Ok(())
    }
}
```

## Applies To

- `CommandLogic::State` types
- Projection/read-model state types
- Any struct that represents reconstructed state from events
