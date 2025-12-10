use crate::command::Event;
use futures::Stream;
use nutype::nutype;
use std::future::Future;
use std::pin::Pin;

/// Stream prefix for filtering subscriptions.
#[nutype(derive(Debug, Clone, AsRef))]
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
}

impl SubscriptionQuery {
    /// Create a query that subscribes to all events.
    ///
    /// Returns events from all streams in EventId (UUIDv7) order.
    pub fn all() -> Self {
        Self {
            stream_prefix: None,
        }
    }

    /// Filter events to only those from streams matching the given prefix.
    ///
    /// Returns a composable query that can be chained with additional filters.
    pub fn filter_stream_prefix(self, prefix: StreamPrefix) -> Self {
        Self {
            stream_prefix: Some(prefix),
        }
    }

    /// Get the stream prefix filter, if any.
    pub fn stream_prefix(&self) -> Option<&StreamPrefix> {
        self.stream_prefix.as_ref()
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
