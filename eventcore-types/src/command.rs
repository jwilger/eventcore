use std::collections::HashSet;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;

use crate::errors::CommandError;
use crate::snapshot::CommandStateSnapshotId;
use crate::store::StreamId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamDeclarations {
    streams: Vec<StreamId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StreamDeclarationsError {
    #[error("commands must declare at least one stream")]
    Empty,
    #[error("duplicate stream declared: {duplicate:?}")]
    DuplicateStream { duplicate: StreamId },
}

impl StreamDeclarations {
    pub fn try_from_streams<I>(streams: I) -> Result<Self, StreamDeclarationsError>
    where
        I: IntoIterator<Item = StreamId>,
    {
        let mut seen = HashSet::new();
        let mut collected = Vec::new();

        for stream in streams.into_iter() {
            if !seen.insert(stream.clone()) {
                return Err(StreamDeclarationsError::DuplicateStream { duplicate: stream });
            }

            collected.push(stream);
        }

        if collected.is_empty() {
            return Err(StreamDeclarationsError::Empty);
        }

        Ok(Self { streams: collected })
    }

    pub fn single(stream: StreamId) -> Self {
        Self {
            streams: vec![stream],
        }
    }

    pub fn with_participant(self, participant: StreamId) -> Result<Self, StreamDeclarationsError> {
        let mut streams = self.streams;
        streams.push(participant);
        Self::try_from_streams(streams)
    }

    /// Returns true if the declaration contains no streams.
    ///
    /// Note: Valid instances constructed via `single()`, `try_from_streams()`, or
    /// `with_participant()` are never empty. This method exists for API completeness
    /// (e.g., to satisfy `clippy::len_without_is_empty`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.streams.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.streams.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &StreamId> {
        self.streams.iter()
    }
}

/// Infrastructure trait describing the streams required to execute a command.
///
/// Per ADR-006, stream declarations are generated or implemented separately from
/// the business logic so infrastructure can evolve independently. Commands
/// typically use [`StreamDeclarations::single`] for single-stream workflows or
/// [`StreamDeclarations::try_from_streams`] when coordinating multiple streams.
pub trait CommandStreams {
    fn stream_declarations(&self) -> StreamDeclarations;
}

/// Trait for runtime stream discovery when static declarations are insufficient.
///
/// Commands implement this trait when related streams cannot be known at compile
/// time (for example, when a customer ID needs to be resolved from reconstructed
/// state). Executors call this hook after reading declared streams so commands
/// can request additional stream IDs to load before running business logic.
///
/// Implementations should return unique stream IDs; the executor deduplicates
/// defensively but redundant IDs still waste I/O. Streams listed here are folded
/// into the state reconstruction pass and participate in optimistic concurrency
/// along with the statically declared streams.
pub trait StreamResolver<State> {
    /// Discovers additional stream IDs to load based on reconstructed state.
    fn discover_related_streams(&self, state: &State) -> Vec<StreamId>;
}

/// Event trait for domain-first event sourcing.
///
/// Per ADR-012, domain types implement this trait to become events. The trait provides
/// the minimal infrastructure contract: events must know their stream identity
/// (aggregate ID) and support necessary operations for storage and async handling.
///
/// # Trait Bounds
///
/// * `Clone` - Required for state reconstruction (apply method may need events multiple times)
/// * `Send` - Required for async storage backends and cross-thread event handling
/// * `'static` - Required for type erasure in storage and async trait boundaries
pub trait Event: Clone + Send + Serialize + DeserializeOwned + 'static {
    /// Returns the stream this event belongs to.
    ///
    /// The stream ID represents the aggregate identity in Domain-Driven Design.
    /// Each domain event knows which aggregate instance it belongs to.
    fn stream_id(&self) -> &StreamId;

    /// Stable event type name for storage metadata.
    ///
    /// Returns a name that identifies this event type independently of
    /// the Rust module path. Used for the `event_type` column in backends
    /// (auditing/debugging — not used for deserialization).
    ///
    /// Choose a name that won't change when the type moves between modules.
    fn event_type_name() -> &'static str
    where
        Self: Sized;
}

/// Trait defining the business logic of a command.
///
/// Commands encapsulate business operations that read from event streams,
/// reconstruct state, validate business rules, and produce events.
///
/// Stream declarations are provided separately via [`CommandStreams`] so that
/// infrastructure (such as proc-macros defined in ADR-006) can evolve
/// independently while this trait focuses purely on domain behavior.
///
/// Per ADR-012, commands use an associated type for their event type rather than
/// a generic parameter, providing better type inference and cleaner APIs.
///
/// # Associated Types
///
/// * `Event` - The domain event type implementing the Event trait
/// * `State` - The state type reconstructed from events via `apply()`
pub trait CommandLogic: CommandStreams + Send + Sync {
    /// The domain event type this command produces.
    ///
    /// Must implement the Event trait to provide stream identity and
    /// required infrastructure capabilities.
    type Event: Event;

    /// The state type accumulated from event history.
    ///
    /// This type represents the reconstructed state needed to validate
    /// business rules and produce events. It's rebuilt from scratch for
    /// each command execution by applying events via `apply()`.
    type State: Default + Send;

    /// Reconstruct state by applying a single event.
    ///
    /// This method is called once per event in the stream(s) to rebuild
    /// the complete state needed for command execution. It implements the
    /// left-fold pattern: `events.fold(State::default(), apply)`.
    ///
    /// # Parameters
    ///
    /// * `state` - The accumulated state so far
    /// * `event` - The next event to apply (borrowed reference)
    ///
    /// # Returns
    ///
    /// The updated state after applying the event
    fn apply(&self, state: Self::State, event: &Self::Event) -> Self::State;

    /// Execute business logic and produce events.
    ///
    /// This method validates business rules using the reconstructed state
    /// and returns events to be persisted. It's a pure function that
    /// makes domain decisions without performing I/O or side effects.
    ///
    /// # Parameters
    ///
    /// * `state` - The reconstructed state from all events
    ///
    /// # Returns
    ///
    /// * `Ok(NewEvents<Self::Event>)` if business rules pass and events produced
    /// * `Err(CommandError)` if business rules violated
    fn handle(&self, state: Self::State) -> Result<NewEvents<Self::Event>, CommandError>;

    /// Returns a runtime stream resolver when the command needs dynamic discovery.
    ///
    /// Commands that implement [`StreamResolver`] can return `Some(self)` or a
    /// dedicated resolver type so the executor loads additional streams after
    /// reconstructing state. The default implementation returns `None`, meaning
    /// the command relies solely on static [`CommandStreams`] declarations.
    fn stream_resolver(&self) -> Option<&(dyn StreamResolver<Self::State> + Sync)> {
        None
    }

    /// Identifies an opt-in durable projection of this command's reconstructed
    /// state. Commands that return `None` retain ordinary full replay.
    fn command_state_snapshot_id(&self) -> Option<CommandStateSnapshotId> {
        None
    }

    /// Serializes reconstructed state for a durable command-state projection.
    fn serialize_command_state_snapshot(
        &self,
        _state: &Self::State,
    ) -> Result<Value, CommandError> {
        Err(CommandError::ValidationError(
            "command declared a snapshot id without serializing its state".to_string(),
        ))
    }

    /// Restores reconstructed state from a durable command-state projection.
    fn deserialize_command_state_snapshot(
        &self,
        _state: Value,
    ) -> Result<Self::State, CommandError> {
        Err(CommandError::ValidationError(
            "command declared a snapshot id without deserializing its state".to_string(),
        ))
    }
}

/// Collection of new events produced by a command.
///
/// This type represents the output of `CommandLogic::handle()` - the
/// events that should be persisted as a result of command execution.
///
/// Per ADR-012, this works with domain event types that implement the Event trait.
pub struct NewEvents<E: Event> {
    events: Vec<E>,
}

impl<E: Event> From<Vec<E>> for NewEvents<E> {
    fn from(events: Vec<E>) -> Self {
        Self { events }
    }
}

impl<E: Event> From<NewEvents<E>> for Vec<E> {
    fn from(new_events: NewEvents<E>) -> Self {
        new_events.events
    }
}

impl<E: Event> Default for NewEvents<E> {
    fn default() -> Self {
        Self { events: Vec::new() }
    }
}

#[cfg(test)]
#[path = "command.test.rs"]
mod tests;
