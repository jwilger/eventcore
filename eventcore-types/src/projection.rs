//! Projection types and traits for building read models from event streams.
//!
//! This module provides the core abstractions for event projection:
//! - `Projector`: Trait for transforming events into read model updates
//! - `EventReader`: Trait for reading events globally for projections
//! - `StreamPosition`: Global position in the event stream

use nutype::nutype;
use std::future::Future;

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
}

/// Blanket implementation allowing EventReader trait to work with references.
impl<T: EventReader + Sync> EventReader for &T {
    type Error = T::Error;

    async fn read_all<E: crate::Event>(&self) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
        (*self).read_all().await
    }
}
