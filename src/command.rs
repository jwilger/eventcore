use std::collections::HashSet;

use thiserror::Error;

use crate::errors::CommandError;
use crate::store::StreamId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandStreams {
    streams: Vec<StreamId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandStreamsError {
    #[error("commands must declare at least one stream")]
    Empty,
    #[error("duplicate stream declared: {duplicate:?}")]
    DuplicateStream { duplicate: StreamId },
}

impl CommandStreams {
    pub fn try_from_streams<I>(streams: I) -> Result<Self, CommandStreamsError>
    where
        I: IntoIterator<Item = StreamId>,
    {
        let mut seen = HashSet::new();
        let mut collected = Vec::new();

        for stream in streams.into_iter() {
            if !seen.insert(stream.clone()) {
                return Err(CommandStreamsError::DuplicateStream { duplicate: stream });
            }

            collected.push(stream);
        }

        if collected.is_empty() {
            return Err(CommandStreamsError::Empty);
        }

        Ok(Self { streams: collected })
    }

    pub fn single(stream: StreamId) -> Self {
        Self {
            streams: vec![stream],
        }
    }

    pub fn with_participant(self, participant: StreamId) -> Result<Self, CommandStreamsError> {
        let mut streams = self.streams;
        streams.push(participant);
        Self::try_from_streams(streams)
    }

    pub fn len(&self) -> usize {
        self.streams.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &StreamId> {
        self.streams.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(id: &str) -> StreamId {
        StreamId::try_new(id.to_owned()).expect("valid stream id")
    }

    #[test]
    fn try_from_streams_succeeds_with_unique_streams() {
        let result = CommandStreams::try_from_streams(vec![
            stream("accounts::primary"),
            stream("accounts::secondary"),
        ]);

        assert!(result.is_ok());
    }

    #[test]
    fn try_from_streams_rejects_empty_collections() {
        let result = CommandStreams::try_from_streams(Vec::new());

        assert_eq!(Err(CommandStreamsError::Empty), result);
    }

    #[test]
    fn try_from_streams_rejects_duplicate_streams() {
        let duplicate = stream("accounts::primary");
        let result = CommandStreams::try_from_streams(vec![duplicate.clone(), duplicate.clone()]);

        assert_eq!(
            Err(CommandStreamsError::DuplicateStream {
                duplicate: duplicate.clone(),
            }),
            result,
        );
    }

    #[test]
    fn with_participant_rejects_duplicate_streams() {
        let existing = stream("accounts::primary");
        let streams = CommandStreams::single(existing.clone());
        let result = streams.with_participant(existing.clone());

        assert_eq!(
            Err(CommandStreamsError::DuplicateStream {
                duplicate: existing,
            }),
            result,
        );
    }
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
pub trait Event: Clone + Send + 'static {
    /// Returns the stream this event belongs to.
    ///
    /// The stream ID represents the aggregate identity in Domain-Driven Design.
    /// Each domain event knows which aggregate instance it belongs to.
    fn stream_id(&self) -> &crate::StreamId;
}

/// Trait defining the business logic of a command.
///
/// Commands encapsulate business operations that read from event streams,
/// reconstruct state, validate business rules, and produce events.
///
/// This trait focuses solely on domain logic. Infrastructure concerns
/// (stream management, event persistence) are handled by the executor.
///
/// Per ADR-012, commands use an associated type for their event type rather than
/// a generic parameter, providing better type inference and cleaner APIs.
///
/// # Associated Types
///
/// * `Event` - The domain event type implementing the Event trait
/// * `State` - The state type reconstructed from events via `apply()`
pub trait CommandLogic {
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
    type State: Default;

    /// Declares the ordered set of streams the command will interact with
    /// during execution.
    ///
    /// Commands must always include at least one stream. Single-stream
    /// commands can return [`CommandStreams::single`], while multi-stream
    /// commands should construct a `CommandStreams` instance using the
    /// fallible APIs to guarantee uniqueness.
    fn streams(&self) -> CommandStreams;

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
