use crate::errors::CommandError;

/// Trait defining the business logic of a command.
///
/// Commands encapsulate business operations that read from event streams,
/// reconstruct state, validate business rules, and produce events.
///
/// This trait focuses solely on domain logic. Infrastructure concerns
/// (stream management, event persistence) are handled by the executor and
/// the `CommandStreams`.
///
/// # Type Parameters
///
/// * `State` - The state type reconstructed from events via `apply()`
pub trait CommandLogic {
    /// The state type accumulated from event history.
    ///
    /// This type represents the reconstructed state needed to validate
    /// business rules and produce events. It's rebuilt from scratch for
    /// each command execution by applying events via `apply()`.
    type State: Default;

    /// Reconstruct state by applying a single event.
    ///
    /// This method is called once per event in the stream(s) to rebuild
    /// the complete state needed for command execution. It implements the
    /// left-fold pattern: `events.fold(State::default(), apply)`.
    ///
    /// # Parameters
    ///
    /// * `state` - The accumulated state so far
    /// * `event` - The next event to apply
    ///
    /// # Returns
    ///
    /// The updated state after applying the event
    fn apply(&self, state: Self::State, event: Event) -> Self::State;

    /// Execute business logic and produce events.
    ///
    /// This method validates business rules using the reconstructed state
    /// and returns events to be persisted. It's a pure function that
    /// makes domain decisions without performing I/O or side effects.
    ///
    /// This is the core domain logic: given current state, what events
    /// should occur? The executor handles infrastructure concerns
    /// (persistence, atomicity, concurrency control) separately.
    ///
    /// # Parameters
    ///
    /// * `state` - The reconstructed state from all events
    ///
    /// # Returns
    ///
    /// * `Ok(NewEvents)` if business rules pass and events produced
    /// * `Err(CommandError)` if business rules violated
    fn handle(&self, state: Self::State) -> Result<NewEvents, CommandError>;
}

/// Placeholder for collection of new events produced by a command.
///
/// This type represents the output of `CommandLogic::handle()` - the
/// events that should be persisted as a result of command execution.
///
/// The exact structure will be refined when we understand actual needs:
/// - Just a collection of events?
/// - Events organized by target stream?
/// - Additional metadata for conflict detection?
///
/// For now, this stub defers the decision until requirements are clear.
///
/// TODO: Refine based on actual event emission and multi-stream needs.
pub struct NewEvents;

/// Placeholder for event type.
///
/// Events represent immutable facts that have occurred in the system.
/// The exact structure will be refined in future increments based on
/// ADR-005 (Event Metadata Structure).
///
/// TODO: Full implementation with type, data, and metadata.
pub struct Event;
