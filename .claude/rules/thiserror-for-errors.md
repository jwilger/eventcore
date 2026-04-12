# Use thiserror for Error Types

All error types in the domain and command layers must use `thiserror::Error`
derive instead of manual `Display` implementations.

## Pattern

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MyError {
    #[error("kebab-case-error-id")]
    VariantName,
}
```

## Command Business Rule Errors

Each command must define a typed error enum for its business rule
violations. Do not use string literals with the `require!` macro or
construct `CommandError::BusinessRuleViolation(String)` directly.

```rust
// Wrong: stringly-typed errors
require!(state.is_setup_completed(), "setup-not-completed");

// Wrong: manually constructing CommandError
Err(CommandError::BusinessRuleViolation("setup-not-completed".to_string()))

// Right: typed error enum with From impl
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthenticateAdminError {
    #[error("setup-not-completed")]
    SetupNotCompleted,
    #[error("invalid-credentials")]
    InvalidCredentials,
}

impl From<AuthenticateAdminError> for CommandError {
    fn from(e: AuthenticateAdminError) -> Self {
        CommandError::BusinessRuleViolation(Box::new(e))
    }
}

// In handle():
state.require_setup_completed().map_err(AuthenticateAdminError::from)?;
```

## Rules

- Derive `thiserror::Error` on all error enums
- Do not manually implement `Display` for error types
- Error messages use kebab-case machine-readable identifiers
- `thiserror` is a workspace dependency — use it via the workspace
- Command error enums implement `From<...> for CommandError`
- `From` impls wrap the original error with `Box::new(e)`, preserving the error chain
- Do not stringify errors with `.to_string()` — this discards the error source chain
- Test assertion helpers accept typed errors via `Into<CommandError>`,
  not raw strings — this matches `require!` macro usage

## Reference

ADR 0022: Use thiserror for Domain Error Types
