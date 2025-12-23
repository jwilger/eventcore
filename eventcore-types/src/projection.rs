//! Projection types and traits for building read models from event streams.
//!
//! This module provides the core abstractions for event projection:
//! - `Projector`: Trait for transforming events into read model updates
//! - `EventReader`: Trait for reading events globally for projections
//! - `StreamPosition`: Global position in the event stream

use crate::store::StreamPrefix;
use nutype::nutype;
use std::future::Future;

/// Context provided to error handler when event processing fails.
///
/// This struct bundles together all the information needed to make
/// informed decisions about how to handle a projection failure.
///
/// # Type Parameters
///
/// - `E`: The error type returned by the projector's `apply()` method
///
/// # Fields
///
/// - `error`: Reference to the error that occurred
/// - `position`: Global stream position where the failure occurred
/// - `retry_count`: Number of times this event has been retried (0 on first failure)
#[derive(Debug)]
pub struct FailureContext<'a, E> {
    /// Reference to the error that occurred during event processing.
    pub error: &'a E,
    /// Global stream position of the event that failed to process.
    pub position: StreamPosition,
    /// Number of retry attempts so far (0 on initial failure).
    pub retry_count: u32,
}

/// Strategy for handling event processing failures.
///
/// When a projector's `apply()` method returns an error, the `on_error()`
/// callback determines how the projection runner should respond. This enum
/// represents the available failure strategies.
///
/// # Variants
///
/// - `Fatal`: Stop processing immediately and return the error
/// - `Skip`: Log the error and continue processing the next event
/// - `Retry`: Attempt to reprocess the event according to retry configuration
///
/// # Example
///
/// ```ignore
/// fn on_error(
///     &mut self,
///     ctx: FailureContext<Self::Error>,
/// ) -> FailureStrategy {
///     match ctx.error {
///         MyError::Transient(_) if ctx.retry_count < 3 => FailureStrategy::Retry,
///         MyError::PoisonEvent(_) => FailureStrategy::Skip,
///         _ => FailureStrategy::Fatal,
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureStrategy {
    /// Stop processing immediately and return the error to the caller.
    ///
    /// Use this when:
    /// - The error is unrecoverable (e.g., database schema mismatch)
    /// - The projector requires manual intervention
    /// - Continuing would corrupt the read model
    Fatal,

    /// Skip this event and continue processing the next one.
    ///
    /// Use this when:
    /// - The event is malformed or invalid (poison event)
    /// - Processing this event is not critical
    /// - Continuing without this event is acceptable
    Skip,

    /// Retry processing this event according to retry configuration.
    ///
    /// Use this when:
    /// - The error is likely transient (e.g., network timeout)
    /// - Retrying might succeed
    /// - The event is important and should not be skipped
    Retry,
}

/// Global stream position representing a location in the ordered event log.
///
/// StreamPosition uniquely identifies a position in the global event stream
/// across all individual streams. Used by projectors to track progress and
/// enable resumable event processing.
///
/// Positions are 0-indexed: position 0 is the first event ever appended.
#[nutype(derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Display))]
pub struct StreamPosition(u64);

/// Trait for transforming events into read model updates.
///
/// Projectors consume events from the event store and update read models.
/// They implement the "Q" (Query) side of CQRS by building denormalized
/// views optimized for reading.
///
/// # Type Parameters
///
/// - `Event`: The domain event type this projector handles
/// - `Error`: The error type returned when projection fails
/// - `Context`: Shared context for database connections, caches, etc.
///
/// # Required Methods
///
/// - `apply`: Process a single event and update the read model
/// - `name`: Return a unique identifier for this projector
///
/// # Example
///
/// ```ignore
/// struct AccountBalanceProjector {
///     balances: HashMap<AccountId, Money>,
/// }
///
/// impl Projector for AccountBalanceProjector {
///     type Event = AccountEvent;
///     type Error = Infallible;
///     type Context = ();
///
///     fn apply(
///         &mut self,
///         event: Self::Event,
///         _position: StreamPosition,
///         _ctx: &mut Self::Context,
///     ) -> Result<(), Self::Error> {
///         match event {
///             AccountEvent::Deposited { account_id, amount } => {
///                 *self.balances.entry(account_id).or_default() += amount;
///             }
///             AccountEvent::Withdrawn { account_id, amount } => {
///                 *self.balances.entry(account_id).or_default() -= amount;
///             }
///         }
///         Ok(())
///     }
///
///     fn name(&self) -> &str {
///         "account-balance"
///     }
/// }
/// ```
pub trait Projector {
    /// The domain event type this projector handles.
    type Event;

    /// The error type returned when projection fails.
    type Error;

    /// Shared context for database connections, caches, etc.
    type Context;

    /// Process a single event and update the read model.
    ///
    /// This method is called for each event in stream order. Implementations
    /// should update their read model state based on the event content.
    ///
    /// # Parameters
    ///
    /// - `event`: The domain event to process
    /// - `position`: The global stream position of this event
    /// - `ctx`: Mutable reference to shared context
    ///
    /// # Returns
    ///
    /// - `Ok(())`: Event was successfully processed
    /// - `Err(Self::Error)`: Projection failed (triggers error handling)
    fn apply(
        &mut self,
        event: Self::Event,
        position: StreamPosition,
        ctx: &mut Self::Context,
    ) -> Result<(), Self::Error>;

    /// Return a unique identifier for this projector.
    ///
    /// The name is used for:
    /// - Logging and tracing
    /// - Checkpoint storage (to resume from last position)
    /// - Coordination (leader election key)
    ///
    /// Names should be stable across deployments. Changing a projector's
    /// name will cause it to reprocess all events from the beginning.
    fn name(&self) -> &str;

    /// Handle event processing errors and determine failure strategy.
    ///
    /// Called when `apply()` returns an error. The projector can inspect
    /// the error context and decide how the runner should respond.
    ///
    /// # Parameters
    ///
    /// - `ctx`: Context containing the error, position, and retry count
    ///
    /// # Returns
    ///
    /// The failure strategy the runner should use:
    /// - `FailureStrategy::Fatal`: Stop processing and return error
    /// - `FailureStrategy::Skip`: Skip this event and continue
    /// - `FailureStrategy::Retry`: Retry processing this event
    ///
    /// # Default Implementation
    ///
    /// Returns `FailureStrategy::Fatal` for all errors. This is the safest
    /// default - projectors that need different behavior should override
    /// this method.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn on_error(
    ///     &mut self,
    ///     ctx: FailureContext<Self::Error>,
    /// ) -> FailureStrategy {
    ///     match ctx.error {
    ///         MyError::Transient(_) if ctx.retry_count < 3 => FailureStrategy::Retry,
    ///         MyError::PoisonEvent(_) => FailureStrategy::Skip,
    ///         _ => FailureStrategy::Fatal,
    ///     }
    /// }
    /// ```
    fn on_error(&mut self, _ctx: FailureContext<'_, Self::Error>) -> FailureStrategy {
        FailureStrategy::Fatal
    }
}

/// Batch size domain type for limiting query results.
///
/// BatchSize represents the maximum number of events to return in a single
/// query. Callers are responsible for choosing appropriate batch sizes based
/// on their memory constraints and use case requirements.
///
/// A batch size of zero is valid and will return an empty result set.
///
/// # Examples
///
/// ```ignore
/// use eventcore_types::projection::BatchSize;
///
/// let small = BatchSize::new(100);
/// let large = BatchSize::new(1_000_000);
/// let empty = BatchSize::new(0);  // Valid - returns no events
/// ```
#[nutype(derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Display))]
pub struct BatchSize(usize);

/// Pagination parameters for reading events.
///
/// EventPage bundles together the cursor position and page size for paginating
/// through events. This separates pagination concerns from filtering concerns.
///
/// # Examples
///
/// ```ignore
/// use eventcore_types::projection::{EventPage, BatchSize};
///
/// // First page
/// let page = EventPage::first(BatchSize::new(100));
/// let events = reader.read_events(filter, page).await?;
///
/// // Next page using the last event's position
/// if let Some(next_page) = page.next_from_results(&events) {
///     let more = reader.read_events(filter, next_page).await?;
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventPage {
    after_position: Option<StreamPosition>,
    limit: BatchSize,
}

impl EventPage {
    /// Create the first page with the given limit.
    ///
    /// Starts reading from the beginning of the event stream.
    pub fn first(limit: BatchSize) -> Self {
        Self {
            after_position: None,
            limit,
        }
    }

    /// Create a page starting after the given position.
    ///
    /// Only events with position > `after_position` will be returned.
    pub fn after(position: StreamPosition, limit: BatchSize) -> Self {
        Self {
            after_position: Some(position),
            limit,
        }
    }

    /// Create the next page using the last position from previous results.
    ///
    /// This is a convenience method for the common pagination pattern.
    /// Returns `Some(next_page)` if events were returned, `None` if empty.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let mut page = EventPage::first(BatchSize::new(100));
    /// loop {
    ///     let events = reader.read_events(filter, page).await?;
    ///     if events.is_empty() {
    ///         break;
    ///     }
    ///     // Process events...
    ///
    ///     // Get next page
    ///     page = match page.next_from_results(&events) {
    ///         Some(next) => next,
    ///         None => break,
    ///     };
    /// }
    /// ```
    pub fn next_from_results<E>(&self, events: &[(E, StreamPosition)]) -> Option<Self> {
        events.last().map(|(_, pos)| Self {
            after_position: Some(*pos),
            limit: self.limit,
        })
    }

    /// Create the next page using an explicit position.
    ///
    /// Returns a new page that starts after the given position with the same limit.
    pub fn next(&self, last_position: StreamPosition) -> Self {
        Self {
            after_position: Some(last_position),
            limit: self.limit,
        }
    }

    /// Get the cursor position for this page.
    pub fn after_position(&self) -> Option<StreamPosition> {
        self.after_position
    }

    /// Get the page size limit.
    pub fn limit(&self) -> BatchSize {
        self.limit
    }
}

/// Filter criteria for selecting which events to read from the event store.
///
/// EventFilter specifies filtering criteria (e.g., stream prefix) separate from
/// pagination concerns (position and limit). Use `::all()` to match all events,
/// or `::prefix()` to filter by stream ID prefix.
///
/// # Examples
///
/// ```ignore
/// // Match all events
/// let filter = EventFilter::all();
///
/// // Filter by stream prefix
/// let filter = EventFilter::prefix("account-");
/// ```
#[derive(Debug, Clone)]
pub struct EventFilter {
    stream_prefix: Option<StreamPrefix>,
}

impl EventFilter {
    /// Create a filter that matches all events from all streams.
    ///
    /// This is the most permissive filter - it matches every event
    /// in the store.
    pub fn all() -> Self {
        Self {
            stream_prefix: None,
        }
    }

    /// Create a filter that matches events from streams with the given prefix.
    ///
    /// Only events whose stream ID starts with the specified prefix will match.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use eventcore_types::{EventFilter, StreamPrefix};
    ///
    /// let prefix = StreamPrefix::try_new("account-").unwrap();
    /// let filter = EventFilter::prefix(prefix);
    /// ```
    pub fn prefix(prefix: StreamPrefix) -> Self {
        Self {
            stream_prefix: Some(prefix),
        }
    }

    /// Get the stream prefix filter, if any.
    ///
    /// Returns `Some(&StreamPrefix)` if a prefix filter is set, or `None`
    /// if this filter matches all streams.
    pub fn stream_prefix(&self) -> Option<&StreamPrefix> {
        self.stream_prefix.as_ref()
    }
}

/// Trait for reading events globally for projections.
///
/// EventReader provides access to all events in global order, which is
/// required for building read models that aggregate data across streams.
///
/// # Pagination and Filtering
///
/// The `read_events` method requires explicit pagination via `EventPage`
/// to prevent accidental memory exhaustion. Filtering is specified via `EventFilter`.
///
/// # Type Safety
///
/// The method is generic over the event type, allowing the caller to specify
/// which event type to deserialize. Events that cannot be deserialized to the
/// requested type are skipped.
pub trait EventReader {
    /// Error type returned by read operations.
    type Error;

    /// Read events matching filter criteria with pagination.
    ///
    /// Returns a vector of tuples containing the event and its global position.
    /// Events are ordered by their append time (oldest first).
    ///
    /// # Type Parameters
    ///
    /// - `E`: The event type to deserialize events as
    ///
    /// # Parameters
    ///
    /// - `filter`: Filtering criteria (stream prefix, etc.)
    /// - `page`: Pagination parameters (cursor position and limit)
    ///
    /// # Returns
    ///
    /// - `Ok(Vec<(E, StreamPosition)>)`: Events with their positions
    /// - `Err(Self::Error)`: If the read operation fails
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let page = EventPage::first(BatchSize::new(100));
    /// let events = reader.read_events(EventFilter::all(), page).await?;
    /// ```
    fn read_events<E: crate::Event>(
        &self,
        filter: EventFilter,
        page: EventPage,
    ) -> impl Future<Output = Result<Vec<(E, StreamPosition)>, Self::Error>> + Send;
}

/// Blanket implementation allowing EventReader trait to work with references.
///
/// This is a trivial forwarding implementation that cannot be meaningfully tested
/// in isolation - mutations here would break all EventReader usage through references.
// cargo-mutants: skip (trivial forwarding impl)
impl<T: EventReader + Sync> EventReader for &T {
    type Error = T::Error;

    async fn read_events<E: crate::Event>(
        &self,
        filter: EventFilter,
        page: EventPage,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        (*self).read_events(filter, page).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_page_first_has_no_after_position() {
        let page = EventPage::first(BatchSize::new(100));
        assert_eq!(page.after_position(), None);
        assert_eq!(page.limit().into_inner(), 100);
    }

    #[test]
    fn event_page_after_has_correct_position() {
        let position = StreamPosition::new(42);
        let page = EventPage::after(position, BatchSize::new(50));
        assert_eq!(page.after_position(), Some(position));
        assert_eq!(page.limit().into_inner(), 50);
    }

    #[test]
    fn event_page_next_preserves_limit_and_updates_position() {
        let page = EventPage::first(BatchSize::new(100));
        let new_position = StreamPosition::new(99);
        let next_page = page.next(new_position);
        assert_eq!(next_page.after_position(), Some(new_position));
        assert_eq!(next_page.limit().into_inner(), 100);
    }
}
