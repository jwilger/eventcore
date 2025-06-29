//! Error types for EventCore.
//!
//! This module provides comprehensive error types for all failure scenarios in the
//! event sourcing system. The error design follows these principles:
//!
//! - **Rich error information**: Include context to help diagnose issues
//! - **Type safety**: Different error types for different subsystems
//! - **Actionable**: Users can determine how to handle each error
//! - **Composable**: Errors can be converted between layers
//! - **Enhanced Diagnostics**: Uses [`miette`] for rich error reporting with helpful hints
//!
//! # Error Categories
//!
//! - **CommandError**: Business logic and command execution failures
//! - **EventStoreError**: Storage and persistence layer failures
//! - **ProjectionError**: Event processing and projection failures
//! - **ValidationError**: Input validation failures (rare due to type-driven design)
//!
//! # Enhanced Error Reporting
//!
//! EventCore uses the [`miette`] crate to provide rich, user-friendly error messages
//! with actionable help text, error codes, and links to documentation.
//!
//! ```rust,ignore
//! use eventcore::errors::{CommandError, CommandResult};
//! use eventcore::miette::{Diagnostic, Report};
//!
//! async fn handle_command_with_diagnostics() -> Result<(), Box<dyn std::error::Error>> {
//!     match execute_command().await {
//!         Ok(result) => Ok(()),
//!         Err(CommandError::ConcurrencyConflict { streams }) => {
//!             // Enhanced error with helpful suggestions
//!             let report = Report::new(CommandError::ConcurrencyConflict { streams });
//!             eprintln!("{:?}", report);
//!             Err(report.into())
//!         }
//!         Err(e) => Err(e.into())
//!     }
//! }
//! ```
//!
//! # Error Handling Patterns
//!
//! ## Command Error Handling
//!
//! ```rust,ignore
//! use eventcore::errors::{CommandError, CommandResult};
//!
//! async fn transfer_money(amount: Money) -> CommandResult<()> {
//!     if amount > available_balance {
//!         return Err(CommandError::BusinessRuleViolation(
//!             "Insufficient funds".to_string()
//!         ));
//!     }
//!     // ... transfer logic
//!     Ok(())
//! }
//!
//! // Handling errors with retry logic
//! async fn transfer_with_retry(amount: Money) -> CommandResult<()> {
//!     let mut attempts = 0;
//!     let max_attempts = 3;
//!     
//!     loop {
//!         match transfer_money(amount).await {
//!             Ok(result) => return Ok(result),
//!             Err(CommandError::ConcurrencyConflict { .. }) if attempts < max_attempts => {
//!                 attempts += 1;
//!                 let delay = std::time::Duration::from_millis(100 * 2_u64.pow(attempts));
//!                 tokio::time::sleep(delay).await;
//!                 continue;
//!             }
//!             Err(e) => return Err(e),
//!         }
//!     }
//! }
//! ```
//!
//! ## Stream Access Error Handling
//!
//! ```rust,ignore
//! use eventcore::errors::CommandError;
//!
//! // Handle stream access violations
//! match command_result {
//!     Err(CommandError::InvalidStreamAccess { stream, declared_streams }) => {
//!         eprintln!("Error: Attempted to access stream '{}' which was not declared", stream);
//!         eprintln!("Declared streams: {:?}", declared_streams);
//!         eprintln!("Fix: Add '{}' to your command's read_streams() method", stream);
//!     }
//!     Err(CommandError::StreamNotDeclared { stream, command_type }) => {
//!         eprintln!("Error: Stream '{}' not declared in {}", stream, command_type);
//!         eprintln!("Fix: Add stream to read_streams() method to enable write access");
//!     }
//!     _ => {}
//! }
//! ```
//!
//! ## Type Mismatch Error Handling
//!
//! ```rust,ignore
//! use eventcore::errors::CommandError;
//!
//! match event_processing_result {
//!     Err(CommandError::TypeMismatch { expected, actual, context }) => {
//!         eprintln!("Type mismatch: expected {}, found {}", expected, actual);
//!         if let Some(ctx) = context {
//!             eprintln!("Context: {}", ctx);
//!         }
//!         eprintln!("This may indicate a schema migration is needed");
//!     }
//!     _ => {}
//! }
//! ```
//!
//! ## Event Store Error Patterns
//!
//! ```rust,ignore
//! use eventcore::errors::{EventStoreError, EventStoreResult};
//! use std::time::Duration;
//!
//! async fn write_events_with_retry<E>(
//!     store: &impl EventStore<Event = E>,
//!     events: Vec<StreamEvents<E>>,
//! ) -> EventStoreResult<()> {
//!     let mut retries = 3;
//!     
//!     loop {
//!         match store.write_events_multi(events.clone()).await {
//!             Ok(_) => return Ok(()),
//!             Err(EventStoreError::VersionConflict { stream, expected, current }) => {
//!                 if retries > 0 {
//!                     retries -= 1;
//!                     eprintln!("Version conflict on stream '{}': expected {}, found {}",
//!                              stream, expected, current);
//!                     eprintln!("Retrying in 100ms... ({} attempts remaining)", retries);
//!                     tokio::time::sleep(Duration::from_millis(100)).await;
//!                     continue;
//!                 } else {
//!                     return Err(EventStoreError::VersionConflict { stream, expected, current });
//!                 }
//!             }
//!             Err(EventStoreError::ConnectionFailed(msg)) => {
//!                 eprintln!("Connection failed: {}", msg);
//!                 eprintln!("Consider checking network connectivity and database health");
//!                 return Err(EventStoreError::ConnectionFailed(msg));
//!             }
//!             Err(e) => return Err(e),
//!         }
//!     }
//! }
//! ```
//!
//! ## Projection Error Recovery
//!
//! ```rust,ignore
//! use eventcore::errors::{ProjectionError, ProjectionResult};
//!
//! async fn start_projection_with_recovery(
//!     projection: &mut impl Projection
//! ) -> ProjectionResult<()> {
//!     match projection.start().await {
//!         Ok(_) => {
//!             println!("Projection started successfully");
//!             Ok(())
//!         }
//!         Err(ProjectionError::AlreadyRunning(name)) => {
//!             println!("Projection '{}' is already running", name);
//!             Ok(()) // This is often acceptable
//!         }
//!         Err(ProjectionError::CheckpointLoadFailed(reason)) => {
//!             println!("Failed to load checkpoint: {}", reason);
//!             println!("Starting projection from beginning...");
//!             projection.reset().await?;
//!             projection.start().await
//!         }
//!         Err(e) => {
//!             eprintln!("Projection start failed: {}", e);
//!             Err(e)
//!         }
//!     }
//! }
//! ```
//!
//! ## Error Conversion and Propagation
//!
//! ```rust,ignore
//! use eventcore::errors::{CommandError, EventStoreError};
//!
//! // EventStoreError automatically converts to CommandError
//! fn handle_event_store_error() -> Result<(), CommandError> {
//!     let event_store_error = EventStoreError::ConnectionFailed("timeout".to_string());
//!     Err(event_store_error.into()) // Converts to CommandError::EventStore
//! }
//!
//! // Version conflicts become concurrency conflicts
//! fn handle_version_conflict() -> Result<(), CommandError> {
//!     let stream_id = StreamId::try_new("test").unwrap();
//!     let version_conflict = EventStoreError::VersionConflict {
//!         stream: stream_id.clone(),
//!         expected: EventVersion::try_new(1).unwrap(),
//!         current: EventVersion::try_new(2).unwrap(),
//!     };
//!     Err(version_conflict.into()) // Becomes CommandError::ConcurrencyConflict
//! }
//! ```
//!
//! ## Best Practices
//!
//! 1. **Use Specific Error Types**: Choose the most specific error variant that describes the failure
//! 2. **Include Context**: Provide relevant information like stream IDs, expected vs actual values
//! 3. **Handle Retryable Errors**: Implement retry logic for `ConcurrencyConflict` and transient failures
//! 4. **Log with Correlation**: Include correlation IDs in error messages for tracing
//! 5. **Use Diagnostics**: Leverage `miette` features for user-friendly error reporting
//! 6. **Fail Fast**: Don't retry business rule violations or validation errors
//! 7. **Graceful Degradation**: Handle projection failures without stopping the entire system

use crate::types::{EventId, EventVersion, StreamId};
use miette::Diagnostic;
use thiserror::Error;

/// Errors that can occur during command execution.
///
/// `CommandError` represents failures at the business logic layer. These errors
/// help distinguish between different failure scenarios so that callers can
/// handle them appropriately.
///
/// # Error Handling Strategy
///
/// - **ValidationFailed**: Retry with corrected input
/// - **BusinessRuleViolation**: Show user-friendly error message
/// - **ConcurrencyConflict**: Retry the command with fresh state
/// - **StreamNotFound**: Check if streams need to be created first
/// - **Unauthorized**: Check permissions and authentication
/// - **EventStore**: Handle based on specific store error
/// - **Internal**: Log and investigate - indicates a bug
///
/// # Example
///
/// ```rust,ignore
/// match command.execute(input).await {
///     Ok(result) => process_result(result),
///     Err(CommandError::BusinessRuleViolation(msg)) => {
///         // Show error to user
///         display_error(&msg);
///     }
///     Err(CommandError::ConcurrencyConflict { streams }) => {
///         // Retry with exponential backoff
///         retry_command(command, input).await?;
///     }
///     Err(e) => return Err(e),
/// }
/// ```
#[derive(Debug, Clone, Error, Diagnostic)]
pub enum CommandError {
    /// The command input validation failed.
    /// This should be rare as validation should happen at type construction.
    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    /// A business rule was violated during command execution.
    #[error("Business rule violation: {0}")]
    BusinessRuleViolation(String),

    /// Optimistic concurrency control detected conflicting updates.
    #[error("Concurrency conflict on streams: {streams:?}")]
    #[diagnostic(
        code(eventcore::concurrency_conflict),
        help("This error occurs when multiple commands modify the same streams simultaneously. Consider implementing retry logic with exponential backoff."),
        url("https://docs.rs/eventcore/latest/eventcore/errors/enum.CommandError.html#variant.ConcurrencyConflict")
    )]
    ConcurrencyConflict {
        /// The streams that had version conflicts
        streams: Vec<StreamId>,
    },

    /// One or more required streams were not found.
    #[error("Stream not found: {0}")]
    StreamNotFound(StreamId),

    /// Attempted to access a stream that wasn't declared in the command's read_streams().
    #[error("Invalid stream access: stream '{stream}' was not declared in read_streams()")]
    #[diagnostic(
        code(eventcore::invalid_stream_access),
        help("Commands can only write to streams they declare in their read_streams() method. Add '{stream}' to your command's read_streams() method."),
        url("https://docs.rs/eventcore/latest/eventcore/errors/enum.CommandError.html#variant.InvalidStreamAccess")
    )]
    InvalidStreamAccess {
        /// The stream that was accessed without being declared
        stream: StreamId,
        /// The streams that were actually declared
        declared_streams: Vec<StreamId>,
    },

    /// Attempted to write to a stream that wasn't declared for read access.
    #[error("Stream not declared: stream '{stream}' must be added to read_streams()")]
    #[diagnostic(
        code(eventcore::stream_not_declared),
        help("Add '{stream}' to your command's read_streams() method to enable write access. Commands must declare all streams they intend to modify."),
        url("https://docs.rs/eventcore/latest/eventcore/errors/enum.CommandError.html#variant.StreamNotDeclared")
    )]
    StreamNotDeclared {
        /// The stream that wasn't declared
        stream: StreamId,
        /// The command type that attempted the access
        command_type: String,
    },

    /// A type mismatch occurred during event processing.
    #[error("Type mismatch: expected {expected}, found {actual}")]
    #[diagnostic(
        code(eventcore::type_mismatch),
        help("Ensure your event types match the expected schema. Consider implementing proper event migration if schema has changed."),
        url("https://docs.rs/eventcore/latest/eventcore/errors/enum.CommandError.html#variant.TypeMismatch")
    )]
    TypeMismatch {
        /// The expected type
        expected: String,
        /// The actual type found
        actual: String,
        /// Context about where the mismatch occurred
        context: Option<String>,
    },

    /// The command requires authorization that was not provided.
    #[error("Unauthorized: missing permission {0}")]
    Unauthorized(String),

    /// An error occurred in the event store while executing the command.
    #[error("Event store error: {0}")]
    EventStore(EventStoreError),

    /// An unexpected internal error occurred.
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Errors that can occur when interacting with the event store.
///
/// `EventStoreError` represents failures at the persistence layer. These errors
/// indicate issues with storing, retrieving, or managing events.
///
/// # Common Scenarios
///
/// - **StreamNotFound**: Normal for new streams, create with `ExpectedVersion::New`
/// - **VersionConflict**: Another process modified the stream, retry needed
/// - **DuplicateEventId**: EventId collision (extremely rare with UUIDv7)
/// - **ConnectionFailed**: Network or database issues
/// - **Timeout**: Operation took too long, may need to retry
///
/// # Retry Strategy
///
/// ```rust,ignore
/// async fn write_with_retry(
///     store: &impl EventStore,
///     events: Vec<StreamEvents>,
/// ) -> Result<(), EventStoreError> {
///     let mut retries = 3;
///     loop {
///         match store.write_events_multi(events.clone()).await {
///             Ok(_) => return Ok(()),
///             Err(EventStoreError::VersionConflict { .. }) if retries > 0 => {
///                 retries -= 1;
///                 tokio::time::sleep(Duration::from_millis(100)).await;
///                 continue;
///             }
///             Err(e) => return Err(e),
///         }
///     }
/// }
/// ```
#[derive(Debug, Error, Diagnostic)]
pub enum EventStoreError {
    /// The requested stream was not found.
    #[error("Stream '{0}' not found")]
    StreamNotFound(StreamId),

    /// A version conflict occurred when writing events.
    #[error(
        "Version conflict on stream '{stream}': expected {expected}, but current is {current}"
    )]
    #[diagnostic(
        code(eventcore::version_conflict),
        help("Another process has modified this stream since you read it. Re-read the stream and retry your operation with the latest version."),
        url("https://docs.rs/eventcore/latest/eventcore/errors/enum.EventStoreError.html#variant.VersionConflict")
    )]
    VersionConflict {
        /// The stream with the version conflict
        stream: StreamId,
        /// The version that was expected
        expected: EventVersion,
        /// The actual current version
        current: EventVersion,
    },

    /// An event with the given ID already exists.
    #[error("Duplicate event ID: {0}")]
    DuplicateEventId(EventId),

    /// The connection to the event store failed.
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// A transaction was rolled back.
    #[error("Transaction rolled back: {0}")]
    TransactionRollback(String),

    /// Serialization of an event failed.
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    /// Deserialization of an event failed.
    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Deserialization error.
    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    /// Schema evolution error.
    #[error("Schema evolution error: {0}")]
    SchemaEvolutionError(String),

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A timeout occurred while waiting for the operation.
    #[error("Operation timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// The event store is temporarily unavailable.
    #[error("Event store unavailable: {0}")]
    Unavailable(String),

    /// An unexpected internal error occurred.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl Clone for EventStoreError {
    fn clone(&self) -> Self {
        match self {
            Self::StreamNotFound(stream_id) => Self::StreamNotFound(stream_id.clone()),
            Self::VersionConflict {
                stream,
                expected,
                current,
            } => Self::VersionConflict {
                stream: stream.clone(),
                expected: *expected,
                current: *current,
            },
            Self::DuplicateEventId(event_id) => Self::DuplicateEventId(*event_id),
            Self::ConnectionFailed(msg) => Self::ConnectionFailed(msg.clone()),
            Self::Configuration(msg) => Self::Configuration(msg.clone()),
            Self::TransactionRollback(msg) => Self::TransactionRollback(msg.clone()),
            Self::SerializationFailed(msg) => Self::SerializationFailed(msg.clone()),
            Self::DeserializationFailed(msg) => Self::DeserializationFailed(msg.clone()),
            Self::SerializationError(msg) => Self::SerializationError(msg.clone()),
            Self::DeserializationError(msg) => Self::DeserializationError(msg.clone()),
            Self::SchemaEvolutionError(msg) => Self::SchemaEvolutionError(msg.clone()),
            Self::Io(err) => Self::Io(std::io::Error::new(err.kind(), err.to_string())),
            Self::Timeout(duration) => Self::Timeout(*duration),
            Self::Unavailable(msg) => Self::Unavailable(msg.clone()),
            Self::Internal(msg) => Self::Internal(msg.clone()),
        }
    }
}

/// Errors that can occur in the projection system.
///
/// `ProjectionError` represents failures when processing events to build
/// read models or derive state from the event stream.
///
/// # Error Scenarios
///
/// - **EventProcessingFailed**: A specific event couldn't be processed
/// - **CheckpointLoadFailed**: Can't resume from last position
/// - **SubscriptionFailed**: Lost connection to event stream
/// - **InvalidStateTransition**: Projection in wrong state for operation
/// - **AlreadyRunning/NotRunning**: State management errors
///
/// # Recovery Strategies
///
/// ```rust,ignore
/// match projection.start().await {
///     Ok(_) => info!("Projection started"),
///     Err(ProjectionError::AlreadyRunning(_)) => {
///         // Already running is often OK
///         debug!("Projection was already running");
///     }
///     Err(ProjectionError::CheckpointLoadFailed(_)) => {
///         // May need to rebuild from beginning
///         warn!("Starting projection from beginning");
///         projection.reset().await?;
///         projection.start().await?;
///     }
///     Err(e) => return Err(e),
/// }
/// ```
#[derive(Debug, Clone, Error, Diagnostic)]
pub enum ProjectionError {
    /// The projection failed to process an event.
    #[error("Failed to process event {event_id}: {reason}")]
    EventProcessingFailed {
        /// The ID of the event that failed to process
        event_id: EventId,
        /// The reason for the failure
        reason: String,
    },

    /// The projection's checkpoint could not be loaded.
    #[error("Failed to load checkpoint: {0}")]
    CheckpointLoadFailed(String),

    /// The projection's checkpoint could not be saved.
    #[error("Failed to save checkpoint: {0}")]
    CheckpointSaveFailed(String),

    /// The projection subscription failed.
    #[error("Subscription failed: {0}")]
    SubscriptionFailed(String),

    /// The projection was not found.
    #[error("Projection not found: {0}")]
    NotFound(String),

    /// Invalid state transition for projection.
    #[error("Invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition {
        /// The current state
        from: crate::projection::ProjectionStatus,
        /// The attempted state
        to: crate::projection::ProjectionStatus,
    },

    /// The projection was already running.
    #[error("Projection '{0}' is already running")]
    AlreadyRunning(String),

    /// The projection was not running.
    #[error("Projection '{0}' is not running")]
    NotRunning(String),

    /// An error occurred in the event store.
    #[error("Event store error: {0}")]
    EventStore(#[from] EventStoreError),

    /// An unexpected internal error occurred.
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Errors that can occur during validation of smart constructor inputs.
///
/// `ValidationError` represents failures when constructing domain types from
/// raw input. These should be rare in practice because validation happens at
/// system boundaries when parsing user input into domain types.
///
/// # Design Philosophy
///
/// In a type-driven system, validation errors only occur at the edges where
/// unstructured data enters the system. Once data is parsed into domain types,
/// those types guarantee validity throughout the program.
///
/// # Example Usage
///
/// ```rust,ignore
/// use eventcore::types::StreamId;
/// use eventcore::errors::ValidationError;
///
/// match StreamId::try_new(user_input) {
///     Ok(stream_id) => {
///         // stream_id is guaranteed valid from here on
///         process_stream(stream_id).await?;
///     }
///     Err(validation_error) => {
///         // Show error to user and ask for corrected input
///         match validation_error {
///             ValidationError::Empty => {
///                 println!("Stream ID cannot be empty");
///             }
///             ValidationError::TooLong { max, actual } => {
///                 println!("Stream ID too long: {actual} chars (max: {max})");
///             }
///             _ => println!("Invalid stream ID: {}", validation_error),
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone, Error, Diagnostic)]
pub enum ValidationError {
    /// The input was empty when a non-empty value was required.
    #[error("Value cannot be empty")]
    Empty,

    /// The input exceeded the maximum allowed length.
    #[error("Value exceeds maximum length of {max} characters (was {actual})")]
    TooLong {
        /// Maximum allowed length
        max: usize,
        /// Actual length provided
        actual: usize,
    },

    /// The input did not meet the minimum required length.
    #[error("Value must be at least {min} characters (was {actual})")]
    TooShort {
        /// Minimum required length
        min: usize,
        /// Actual length provided
        actual: usize,
    },

    /// The input contained invalid characters or format.
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// The input value was out of the allowed range.
    #[error("Value out of range: {0}")]
    OutOfRange(String),

    /// A custom validation rule failed.
    #[error("Validation failed: {0}")]
    Custom(String),
}

/// Type alias for command results.
pub type CommandResult<T> = Result<T, CommandError>;

/// Type alias for event store results.
pub type EventStoreResult<T> = Result<T, EventStoreError>;

/// Type alias for projection results.
pub type ProjectionResult<T> = Result<T, ProjectionError>;

impl From<EventStoreError> for CommandError {
    fn from(err: EventStoreError) -> Self {
        match err {
            EventStoreError::VersionConflict { stream, .. } => Self::ConcurrencyConflict {
                streams: vec![stream],
            },
            other => Self::EventStore(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_error_messages_are_descriptive() {
        let err = CommandError::ValidationFailed("test validation".to_string());
        assert_eq!(err.to_string(), "Validation failed: test validation");

        let err = CommandError::BusinessRuleViolation("insufficient funds".to_string());
        assert_eq!(
            err.to_string(),
            "Business rule violation: insufficient funds"
        );

        let stream_id = StreamId::try_new("test-stream").unwrap();
        let err = CommandError::StreamNotFound(stream_id.clone());
        assert_eq!(err.to_string(), "Stream not found: test-stream");

        let err = CommandError::ConcurrencyConflict {
            streams: vec![stream_id.clone()],
        };
        assert!(err.to_string().contains("Concurrency conflict"));

        let err = CommandError::InvalidStreamAccess {
            stream: stream_id.clone(),
            declared_streams: vec![StreamId::try_new("other-stream").unwrap()],
        };
        assert!(err.to_string().contains("Invalid stream access"));
        assert!(err.to_string().contains("test-stream"));

        let err = CommandError::StreamNotDeclared {
            stream: stream_id,
            command_type: "TestCommand".to_string(),
        };
        assert!(err.to_string().contains("Stream not declared"));
        assert!(err.to_string().contains("test-stream"));

        let err = CommandError::TypeMismatch {
            expected: "AccountEvent".to_string(),
            actual: "OrderEvent".to_string(),
            context: Some("event deserialization".to_string()),
        };
        assert!(err.to_string().contains("Type mismatch"));
        assert!(err.to_string().contains("AccountEvent"));
        assert!(err.to_string().contains("OrderEvent"));
    }

    #[test]
    fn event_store_error_messages_are_descriptive() {
        let stream_id = StreamId::try_new("test-stream").unwrap();
        let err = EventStoreError::StreamNotFound(stream_id.clone());
        assert_eq!(err.to_string(), "Stream 'test-stream' not found");

        let err = EventStoreError::VersionConflict {
            stream: stream_id,
            expected: EventVersion::try_new(5).unwrap(),
            current: EventVersion::try_new(7).unwrap(),
        };
        assert_eq!(
            err.to_string(),
            "Version conflict on stream 'test-stream': expected 5, but current is 7"
        );

        let event_id = EventId::new();
        let err = EventStoreError::DuplicateEventId(event_id);
        assert!(err.to_string().contains("Duplicate event ID"));
    }

    #[test]
    fn projection_error_messages_are_descriptive() {
        let event_id = EventId::new();
        let err = ProjectionError::EventProcessingFailed {
            event_id,
            reason: "invalid data".to_string(),
        };
        assert!(err.to_string().contains("Failed to process event"));
        assert!(err.to_string().contains("invalid data"));

        let err = ProjectionError::AlreadyRunning("test-projection".to_string());
        assert_eq!(
            err.to_string(),
            "Projection 'test-projection' is already running"
        );
    }

    #[test]
    fn validation_error_messages_are_descriptive() {
        let err = ValidationError::Empty;
        assert_eq!(err.to_string(), "Value cannot be empty");

        let err = ValidationError::TooLong {
            max: 255,
            actual: 300,
        };
        assert_eq!(
            err.to_string(),
            "Value exceeds maximum length of 255 characters (was 300)"
        );

        let err = ValidationError::TooShort { min: 5, actual: 3 };
        assert_eq!(
            err.to_string(),
            "Value must be at least 5 characters (was 3)"
        );
    }

    #[test]
    fn error_conversion_from_event_store_to_command_error() {
        let stream_id = StreamId::try_new("test").unwrap();
        let event_store_err = EventStoreError::StreamNotFound(stream_id);
        let command_err: CommandError = event_store_err.into();

        match command_err {
            CommandError::EventStore(EventStoreError::StreamNotFound(_)) => {}
            _ => panic!("Expected CommandError::EventStore variant"),
        }
    }

    #[test]
    fn error_conversion_version_conflict_to_concurrency_conflict() {
        let stream_id = StreamId::try_new("test").unwrap();
        let event_store_err = EventStoreError::VersionConflict {
            stream: stream_id.clone(),
            expected: EventVersion::try_new(1).unwrap(),
            current: EventVersion::try_new(2).unwrap(),
        };
        let command_err: CommandError = event_store_err.into();

        match command_err {
            CommandError::ConcurrencyConflict { streams } => {
                assert_eq!(streams.len(), 1);
                assert_eq!(streams[0], stream_id);
            }
            _ => panic!("Expected CommandError::ConcurrencyConflict variant"),
        }
    }

    #[test]
    fn error_conversion_from_event_store_to_projection_error() {
        let stream_id = StreamId::try_new("test").unwrap();
        let event_store_err = EventStoreError::StreamNotFound(stream_id);
        let projection_err: ProjectionError = event_store_err.into();

        match projection_err {
            ProjectionError::EventStore(EventStoreError::StreamNotFound(_)) => {}
            _ => panic!("Expected ProjectionError::EventStore variant"),
        }
    }

    #[test]
    fn error_conversion_from_io_error() {
        use std::io::{Error as IoError, ErrorKind};

        let io_err = IoError::new(ErrorKind::NotFound, "file not found");
        let event_store_err: EventStoreError = io_err.into();

        match event_store_err {
            EventStoreError::Io(_) => {}
            _ => panic!("Expected EventStoreError::Io variant"),
        }
    }

    #[test]
    fn result_type_aliases_work() {
        fn command_fn() -> CommandResult<()> {
            Err(CommandError::ValidationFailed("test".to_string()))
        }

        #[allow(clippy::unnecessary_wraps)]
        fn event_store_fn() -> EventStoreResult<()> {
            Ok(())
        }

        fn projection_fn() -> ProjectionResult<()> {
            Err(ProjectionError::NotRunning("test".to_string()))
        }

        assert!(command_fn().is_err());
        assert!(event_store_fn().is_ok());
        assert!(projection_fn().is_err());
    }

    #[test]
    fn diagnostic_attributes_are_present() {
        use miette::Diagnostic;

        let stream_id = StreamId::try_new("test-stream").unwrap();

        // Test ConcurrencyConflict diagnostic
        let err = CommandError::ConcurrencyConflict {
            streams: vec![stream_id.clone()],
        };
        assert_eq!(
            err.code().unwrap().to_string(),
            "eventcore::concurrency_conflict"
        );
        assert!(err.help().is_some());
        assert!(err.url().is_some());

        // Test InvalidStreamAccess diagnostic
        let err = CommandError::InvalidStreamAccess {
            stream: stream_id.clone(),
            declared_streams: vec![],
        };
        assert_eq!(
            err.code().unwrap().to_string(),
            "eventcore::invalid_stream_access"
        );
        assert!(err.help().is_some());

        // Test StreamNotDeclared diagnostic
        let err = CommandError::StreamNotDeclared {
            stream: stream_id.clone(),
            command_type: "TestCommand".to_string(),
        };
        assert_eq!(
            err.code().unwrap().to_string(),
            "eventcore::stream_not_declared"
        );
        assert!(err.help().is_some());

        // Test TypeMismatch diagnostic
        let err = CommandError::TypeMismatch {
            expected: "String".to_string(),
            actual: "Integer".to_string(),
            context: None,
        };
        assert_eq!(err.code().unwrap().to_string(), "eventcore::type_mismatch");
        assert!(err.help().is_some());

        // Test VersionConflict diagnostic
        let err = EventStoreError::VersionConflict {
            stream: stream_id,
            expected: EventVersion::try_new(1).unwrap(),
            current: EventVersion::try_new(2).unwrap(),
        };
        assert_eq!(
            err.code().unwrap().to_string(),
            "eventcore::version_conflict"
        );
        assert!(err.help().is_some());
    }

    #[test]
    fn diagnostic_help_messages_are_useful() {
        use miette::Diagnostic;

        let stream_id1 = StreamId::try_new("test-stream-1").unwrap();
        let stream_id2 = StreamId::try_new("test-stream-2").unwrap();
        let stream_id3 = StreamId::try_new("test-stream-3").unwrap();

        let err = CommandError::ConcurrencyConflict {
            streams: vec![stream_id1],
        };
        let help = err.help().unwrap();
        assert!(help.to_string().contains("retry"));
        assert!(help.to_string().contains("exponential backoff"));

        let err = CommandError::InvalidStreamAccess {
            stream: stream_id2,
            declared_streams: vec![],
        };
        let help = err.help().unwrap();
        assert!(help.to_string().contains("read_streams"));
        assert!(help.to_string().contains("declare"));

        let err = CommandError::StreamNotDeclared {
            stream: stream_id3,
            command_type: "TestCommand".to_string(),
        };
        let help = err.help().unwrap();
        assert!(help.to_string().contains("read_streams"));
        assert!(help.to_string().contains("Add"));
    }
}
