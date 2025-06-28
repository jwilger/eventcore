//! Error types for the `EventCore` event sourcing library.
//!
//! This module defines all error types used throughout the library,
//! following the principle of making illegal states unrepresentable.

use crate::types::{EventId, EventVersion, StreamId};
use thiserror::Error;

/// Errors that can occur during command execution.
#[derive(Debug, Clone, Error)]
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
    ConcurrencyConflict {
        /// The streams that had version conflicts
        streams: Vec<StreamId>,
    },

    /// One or more required streams were not found.
    #[error("Stream not found: {0}")]
    StreamNotFound(StreamId),

    /// The command requires authorization that was not provided.
    #[error("Unauthorized: missing permission {0}")]
    Unauthorized(String),

    /// An error occurred in the event store while executing the command.
    #[error("Event store error: {0}")]
    EventStore(#[from] EventStoreError),

    /// An unexpected internal error occurred.
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Errors that can occur when interacting with the event store.
#[derive(Debug, Error)]
pub enum EventStoreError {
    /// The requested stream was not found.
    #[error("Stream '{0}' not found")]
    StreamNotFound(StreamId),

    /// A version conflict occurred when writing events.
    #[error(
        "Version conflict on stream '{stream}': expected {expected}, but current is {current}"
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
            Self::Io(err) => Self::Io(std::io::Error::new(err.kind(), err.to_string())),
            Self::Timeout(duration) => Self::Timeout(*duration),
            Self::Unavailable(msg) => Self::Unavailable(msg.clone()),
            Self::Internal(msg) => Self::Internal(msg.clone()),
        }
    }
}

/// Errors that can occur in the projection system.
#[derive(Debug, Clone, Error)]
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
/// These errors are typically returned by the `nutype` generated constructors
/// for our domain types.
#[derive(Debug, Clone, Error)]
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
            streams: vec![stream_id],
        };
        assert!(err.to_string().contains("Concurrency conflict"));
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
}
