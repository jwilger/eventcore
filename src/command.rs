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
/// * `EventPayload` - The consumer's event payload type (e.g., MoneyDeposited, AccountOpened)
/// * `State` - The state type reconstructed from events via `apply()`
pub trait CommandLogic<EventPayload> {
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
    fn apply(&self, state: Self::State, event: Event<EventPayload>) -> Self::State;

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
    /// * `Ok(NewEvents<EventPayload>)` if business rules pass and events produced
    /// * `Err(CommandError)` if business rules violated
    fn handle(&self, state: Self::State) -> Result<NewEvents<EventPayload>, CommandError>;
}

/// Collection of new events produced by a command.
///
/// This type represents the output of `CommandLogic::handle()` - the
/// events that should be persisted as a result of command execution.
///
/// # Type Parameters
///
/// * `T` - The consumer's event payload type (e.g., MoneyDeposited, AccountOpened)
pub struct NewEvents<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> Default for NewEvents<T> {
    fn default() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

/// Event wrapper for consumer event payloads.
///
/// Events represent immutable facts that have occurred in the system.
/// The generic type parameter T is the consumer's event payload type
/// (e.g., MoneyDeposited, AccountOpened, etc.). EventCore is generic
/// over the consumer's event types.
///
/// The exact structure will be refined in future increments based on
/// ADR-005 (Event Metadata Structure) to include metadata like:
/// - EventId (UUIDv7)
/// - Timestamp
/// - CorrelationId
/// - CausationId
///
/// For now, this is a minimal struct with just the payload field.
///
/// TODO: Add metadata fields per ADR-005.
#[derive(Clone)]
pub struct Event<T> {
    /// The consumer's event payload.
    pub payload: T,
}
