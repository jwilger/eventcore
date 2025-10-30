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
    #[error("concurrency conflict after {0} retry attempts")]
    ConcurrencyError(u32),

    /// Storage backend failure during event store operations.
    ///
    /// This error wraps failures from the event store backend (network errors,
    /// constraint violations, etc.). The error classification determines
    /// whether retry is appropriate.
    #[error("event store error: {0}")]
    EventStoreError(crate::store::EventStoreError),

    /// Invalid command state detected during execution.
    ///
    /// This error indicates the command entered an invalid state during
    /// execution (e.g., state reconstruction failed, required stream missing).
    /// These errors are permanent and indicate logic errors.
    #[error("validation error")]
    ValidationError,
}
