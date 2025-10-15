use thiserror::Error;

/// Error type for command execution failures.
///
/// Represents all possible failure modes during command execution.
/// Commands report these errors to the executor, which uses the error
/// classification to determine retry behavior.
#[derive(Error, Debug)]
pub enum CommandError {
    /// Business rule violation detected in command logic.
    ///
    /// This error indicates the command violated a domain-specific business
    /// rule (e.g., insufficient funds, duplicate entity). These errors are
    /// permanent and will not succeed on retry with the same input.
    #[error("business rule violation: {0}")]
    BusinessRuleViolation(String),

    /// Version conflict detected during optimistic concurrency control.
    ///
    /// This error indicates another command modified the stream(s) between
    /// this command's read and write phases. The executor will automatically
    /// retry with fresh state.
    #[error("concurrency conflict: {0}")]
    ConcurrencyError(#[from] ConcurrencyError),

    /// Storage backend failure during event store operations.
    ///
    /// This error wraps failures from the event store backend (network errors,
    /// constraint violations, etc.). The error classification determines
    /// whether retry is appropriate.
    #[error("event store error: {0}")]
    EventStoreError(#[from] EventStoreError),

    /// Invalid command state detected during execution.
    ///
    /// This error indicates the command entered an invalid state during
    /// execution (e.g., state reconstruction failed, required stream missing).
    /// These errors are permanent and indicate logic errors.
    #[error("validation error: {0}")]
    ValidationError(#[from] ValidationError),
}

/// Placeholder for concurrency error details.
///
/// TODO: Implement full ConcurrencyError with expected vs actual versions.
#[derive(Error, Debug)]
#[error("version conflict")]
pub struct ConcurrencyError;

/// Placeholder for event store error details.
///
/// TODO: Implement full EventStoreError with backend-specific details.
#[derive(Error, Debug)]
#[error("event store failure")]
pub struct EventStoreError;

/// Placeholder for validation error details.
///
/// TODO: Implement full ValidationError with field-specific context.
#[derive(Error, Debug)]
#[error("validation failure")]
pub struct ValidationError;