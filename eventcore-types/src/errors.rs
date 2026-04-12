use thiserror::Error;

use crate::store::EventStoreError;

/// String-based error for business rule violations created from string messages.
///
/// This type is used internally by the `require!` macro and the `From<String>`/`From<&str>`
/// implementations on `CommandError`. It is public because the `require!` macro generates
/// code at call sites that needs access to it, but it should be considered an implementation
/// detail.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct BusinessRuleMessage(String);

/// Error type for command execution failures.
///
/// Represents all possible failure modes during command execution.
/// Commands report these errors to the executor, which uses the error
/// classification to determine retry behavior.
#[derive(Error, Debug)]
pub enum CommandError {
    /// Business rule violation detected in command logic.
    ///
    /// This error wraps the original typed error, preserving the Rust error
    /// chain convention. Use `Box::new(e)` to wrap typed errors, or use the
    /// `From<String>`/`From<&str>` impls for string-based errors.
    #[error(transparent)]
    BusinessRuleViolation(Box<dyn std::error::Error + Send + Sync>),

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
    EventStoreError(EventStoreError),

    /// Invalid command state detected during execution.
    ///
    /// This error indicates the command entered an invalid state during
    /// execution (e.g., state reconstruction failed, required stream missing).
    /// These errors are permanent and indicate logic errors.
    #[error("validation error: {0}")]
    ValidationError(String),
}

impl CommandError {
    /// Create a business rule violation from any error type.
    pub fn business_rule_violated(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        CommandError::BusinessRuleViolation(Box::new(error))
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        CommandError::BusinessRuleViolation(Box::new(BusinessRuleMessage(message)))
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        CommandError::BusinessRuleViolation(Box::new(BusinessRuleMessage(message.to_string())))
    }
}
