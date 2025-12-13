use crate::command::{Event, EventTypeName};
use futures::Stream;
use nutype::nutype;
use std::future::Future;
use std::pin::Pin;

/// Validation predicate: reject glob metacharacters in StreamPrefix.
///
/// Per ADR-017, StreamPrefix reserves glob metacharacters (*, ?, [, ]) to enable
/// future pattern matching without ambiguity or escaping complexity.
fn no_glob_metacharacters(s: &str) -> bool {
    !s.contains(['*', '?', '[', ']'])
}

/// Stream prefix for filtering subscriptions.
///
/// Uses nutype for compile-time validation ensuring all stream prefixes are:
/// - Non-empty (trimmed strings with at least 1 character)
/// - Within reasonable length (max 255 characters)
/// - Sanitized (leading/trailing whitespace removed)
/// - Free of glob metacharacters (*, ?, [, ]) per ADR-017
///
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255, predicate = no_glob_metacharacters),
    derive(Debug, Clone, AsRef)
)]
pub struct StreamPrefix(String);

/// Query builder for event subscriptions.
///
/// SubscriptionQuery provides a composable API for filtering events
/// in projections and read models. Queries are type-safe infrastructure
/// types separate from domain StreamId.
///
/// # Examples
///
/// ```rust,ignore
/// use eventcore::SubscriptionQuery;
///
/// // Subscribe to all events
/// let query = SubscriptionQuery::all();
///
/// // Composable filters (future)
/// let query = SubscriptionQuery::all()
///     .filter_stream_prefix("account-")
///     .filter_event_type::<MoneyDeposited>();
/// ```
#[derive(Debug, Clone)]
pub struct SubscriptionQuery {
    stream_prefix: Option<StreamPrefix>,
    event_type_name: Option<EventTypeName>,
}

impl SubscriptionQuery {
    /// Create a query that subscribes to all events.
    ///
    /// Returns events from all streams in EventId (UUIDv7) order.
    pub fn all() -> Self {
        Self {
            stream_prefix: None,
            event_type_name: None,
        }
    }

    /// Filter events to only those from streams matching the given prefix.
    ///
    /// Returns a composable query that can be chained with additional filters.
    pub fn filter_stream_prefix(self, prefix: StreamPrefix) -> Self {
        Self {
            stream_prefix: Some(prefix),
            event_type_name: self.event_type_name,
        }
    }

    /// Filter events to only those matching the specified event type name.
    ///
    /// Returns a composable query that can be chained with additional filters.
    ///
    /// This method accepts an EventTypeName for filtering by event type name.
    /// Unlike TypeId-based filtering, this enables filtering enum variants that
    /// share the same type.
    ///
    /// # Parameters
    ///
    /// * `name` - Event type name to filter by (e.g., "Deposited", "MoneyWithdrawn")
    pub fn filter_event_type_name(self, name: EventTypeName) -> Self {
        Self {
            stream_prefix: self.stream_prefix,
            event_type_name: Some(name),
        }
    }

    /// Get the stream prefix filter, if any.
    pub fn stream_prefix(&self) -> Option<&StreamPrefix> {
        self.stream_prefix.as_ref()
    }

    /// Get the event type name filter, if any.
    pub fn event_type_name_filter(&self) -> Option<&EventTypeName> {
        self.event_type_name.as_ref()
    }
}

/// Error type for subscription operations.
///
/// Represents failures during subscription creation or event delivery.
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    #[error("subscription error: {0}")]
    Generic(String),
}

/// Event subscription trait for read models and projections.
///
/// Separate from EventStore (write side) per ADR-016 CQRS separation.
/// Provides long-lived event streams for building projections and
/// triggering side effects.
///
/// Not all backends must implement this trait - backends opt into
/// subscription support based on their capabilities.
pub trait EventSubscription {
    /// Subscribe to events matching the given query.
    ///
    /// Returns a futures::Stream that yields events in EventId (UUIDv7) order.
    /// The stream delivers events with at-least-once semantics - consumers
    /// must be idempotent.
    ///
    /// # Type Parameters
    ///
    /// * `E` - Event type implementing the Event trait
    ///
    /// # Errors
    ///
    /// Returns SubscriptionError if the subscription cannot be created.
    fn subscribe<E: Event>(
        &self,
        query: SubscriptionQuery,
    ) -> impl Future<Output = Result<Pin<Box<dyn Stream<Item = E> + Send>>, SubscriptionError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_prefix_rejects_asterisk_metacharacter() {
        // Given/When
        let result = StreamPrefix::try_new("account-*");

        // Then
        assert!(
            result.is_err(),
            "StreamPrefix should reject asterisk glob metacharacter"
        );
    }

    #[test]
    fn stream_prefix_rejects_question_mark_metacharacter() {
        // Given/When
        let result = StreamPrefix::try_new("account-?");

        // Then
        assert!(
            result.is_err(),
            "StreamPrefix should reject question mark glob metacharacter"
        );
    }

    #[test]
    fn stream_prefix_rejects_bracket_metacharacters() {
        // Given/When
        let result_open = StreamPrefix::try_new("account-[");
        let result_close = StreamPrefix::try_new("account-]");

        // Then
        assert!(
            result_open.is_err(),
            "StreamPrefix should reject open bracket glob metacharacter"
        );
        assert!(
            result_close.is_err(),
            "StreamPrefix should reject close bracket glob metacharacter"
        );
    }

    #[test]
    fn stream_prefix_rejects_empty_string() {
        // Given/When
        let result = StreamPrefix::try_new("");

        // Then
        assert!(result.is_err(), "StreamPrefix should reject empty string");
    }

    #[test]
    fn stream_prefix_rejects_whitespace_only() {
        // Given/When
        let result = StreamPrefix::try_new("   ");

        // Then
        assert!(
            result.is_err(),
            "StreamPrefix should reject whitespace-only string (trimmed to empty)"
        );
    }

    #[test]
    fn stream_prefix_rejects_string_over_255_chars() {
        // Given
        let too_long = "a".repeat(256);

        // When
        let result = StreamPrefix::try_new(&too_long);

        // Then
        assert!(
            result.is_err(),
            "StreamPrefix should reject strings over 255 characters"
        );
    }

    #[test]
    fn stream_prefix_trims_whitespace() {
        // Given/When
        let result = StreamPrefix::try_new(" account- ");

        // Then
        assert!(result.is_ok(), "StreamPrefix should accept valid string");
        assert_eq!(
            result.expect("valid stream prefix").as_ref(),
            "account-",
            "StreamPrefix should trim leading and trailing whitespace"
        );
    }
}
