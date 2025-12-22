//! Projection types and traits for building read models from event streams.
//!
//! This module provides the core abstractions for event projection:
//! - `Projector`: Trait for transforming events into read model updates
//! - `EventReader`: Trait for reading events globally for projections
//! - `StreamPosition`: Global position in the event stream

use nutype::nutype;
use std::future::Future;

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
///     error: &Self::Error,
///     position: StreamPosition,
/// ) -> FailureStrategy {
///     match error {
///         MyError::Transient(_) => FailureStrategy::Retry,
///         MyError::PoisonEvent(_) => FailureStrategy::Skip,
///         MyError::Fatal(_) => FailureStrategy::Fatal,
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
    /// the error and decide how the runner should respond.
    ///
    /// # Parameters
    ///
    /// - `error`: Reference to the error returned by `apply()`
    /// - `position`: The global stream position of the event that failed
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
    ///     error: &Self::Error,
    ///     _position: StreamPosition,
    /// ) -> FailureStrategy {
    ///     match error {
    ///         MyError::Transient(_) => FailureStrategy::Retry,
    ///         MyError::PoisonEvent(_) => FailureStrategy::Skip,
    ///         MyError::Fatal(_) => FailureStrategy::Fatal,
    ///     }
    /// }
    /// ```
    fn on_error(&mut self, _error: &Self::Error, _position: StreamPosition) -> FailureStrategy {
        FailureStrategy::Fatal
    }
}

/// Trait for reading events globally for projections.
///
/// EventReader provides access to all events in global order, which is
/// required for building read models that aggregate data across streams.
///
/// # Type Safety
///
/// The `read_all` method is generic over the event type, allowing the
/// caller to specify which event type to deserialize. Events that cannot
/// be deserialized to the requested type are skipped.
pub trait EventReader {
    /// Error type returned by read operations.
    type Error;

    /// Read all events from the store in global order.
    ///
    /// Returns a vector of tuples containing the event and its global position.
    /// Events are ordered by their append time (oldest first).
    ///
    /// # Type Parameters
    ///
    /// - `E`: The event type to deserialize events as
    ///
    /// # Returns
    ///
    /// - `Ok(Vec<(E, StreamPosition)>)`: Events with their positions
    /// - `Err(Self::Error)`: If the read operation fails
    fn read_all<E: crate::Event>(
        &self,
    ) -> impl Future<Output = Result<Vec<(E, StreamPosition)>, Self::Error>> + Send;

    /// Read events after the given position in global order.
    ///
    /// Returns a vector of tuples containing the event and its global position.
    /// Only events with position > after_position are returned.
    ///
    /// # Type Parameters
    ///
    /// - `E`: The event type to deserialize events as
    ///
    /// # Returns
    ///
    /// - `Ok(Vec<(E, StreamPosition)>)`: Events with their positions
    /// - `Err(Self::Error)`: If the read operation fails
    fn read_after<E: crate::Event>(
        &self,
        after_position: StreamPosition,
    ) -> impl Future<Output = Result<Vec<(E, StreamPosition)>, Self::Error>> + Send;
}

/// Blanket implementation allowing EventReader trait to work with references.
impl<T: EventReader + Sync> EventReader for &T {
    type Error = T::Error;

    async fn read_all<E: crate::Event>(&self) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        (*self).read_all().await
    }

    async fn read_after<E: crate::Event>(
        &self,
        after_position: StreamPosition,
    ) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        (*self).read_after(after_position).await
    }
}
