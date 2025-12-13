use std::collections::HashSet;

use nutype::nutype;
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::errors::CommandError;
use crate::store::StreamId;

/// Domain type representing an event type name.
///
/// Uses nutype for compile-time validation ensuring all event type names are:
/// - Non-empty (trimmed strings with at least 1 character)
/// - Within reasonable length (max 255 characters)
/// - Sanitized (leading/trailing whitespace removed)
///
/// This type enforces the parse-don't-validate principle: once constructed,
/// an EventTypeName is guaranteed to be valid for event filtering and type discovery.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        Serialize,
        Deserialize,
        AsRef,
        TryFrom
    )
)]
pub struct EventTypeName(String);

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

    /// Returns the event type name for filtering.
    ///
    /// This method enables filtering events by type name, which is necessary
    /// for distinguishing enum variants that share the same TypeId. For struct
    /// event types, return the struct name. For enum event types, return the
    /// variant name.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Struct event type
    /// impl Event for MoneyDeposited {
    ///     fn event_type_name(&self) -> EventTypeName {
    ///         "MoneyDeposited".try_into().expect("valid event type name")
    ///     }
    /// }
    ///
    /// // Enum event type
    /// impl Event for AccountEvent {
    ///     fn event_type_name(&self) -> EventTypeName {
    ///         match self {
    ///             AccountEvent::Deposited { .. } => "Deposited".try_into().expect("valid event type name"),
    ///             AccountEvent::Withdrawn { .. } => "Withdrawn".try_into().expect("valid event type name"),
    ///         }
    ///     }
    /// }
    /// ```
    fn event_type_name(&self) -> EventTypeName;

    /// Returns all possible event type names for this Event type.
    ///
    /// This associated function provides compile-time discovery of all type names
    /// that instances of this Event type can return from `event_type_name()`.
    /// For struct event types, this returns a single-element vector containing
    /// the struct name. For enum event types, this returns all variant names.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Struct event type
    /// impl Event for MoneyDeposited {
    ///     fn all_type_names() -> Vec<EventTypeName> {
    ///         vec!["MoneyDeposited".try_into().expect("valid event type name")]
    ///     }
    /// }
    ///
    /// // Enum event type
    /// impl Event for AccountEvent {
    ///     fn all_type_names() -> Vec<EventTypeName> {
    ///         vec![
    ///             "Deposited".try_into().expect("valid event type name"),
    ///             "Withdrawn".try_into().expect("valid event type name"),
    ///         ]
    ///     }
    /// }
    /// ```
    fn all_type_names() -> Vec<EventTypeName>;
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
pub trait CommandLogic: CommandStreams {
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
    fn stream_resolver(&self) -> Option<&dyn StreamResolver<Self::State>> {
        None
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
mod tests {
    use super::*;

    fn stream(id: &str) -> StreamId {
        StreamId::try_new(id.to_owned()).expect("valid stream id")
    }

    #[test]
    fn try_from_streams_succeeds_with_unique_streams() {
        let result = StreamDeclarations::try_from_streams(vec![
            stream("accounts::primary"),
            stream("accounts::secondary"),
        ]);

        assert!(result.is_ok());
    }

    #[test]
    fn try_from_streams_rejects_empty_collections() {
        let result = StreamDeclarations::try_from_streams(Vec::new());

        assert_eq!(Err(StreamDeclarationsError::Empty), result);
    }

    #[test]
    fn try_from_streams_rejects_duplicate_streams() {
        let duplicate = stream("accounts::primary");
        let result =
            StreamDeclarations::try_from_streams(vec![duplicate.clone(), duplicate.clone()]);

        assert_eq!(
            Err(StreamDeclarationsError::DuplicateStream {
                duplicate: duplicate.clone(),
            }),
            result,
        );
    }

    #[test]
    fn with_participant_rejects_duplicate_streams() {
        let existing = stream("accounts::primary");
        let streams = StreamDeclarations::single(existing.clone());
        let result = streams.with_participant(existing.clone());

        assert_eq!(
            Err(StreamDeclarationsError::DuplicateStream {
                duplicate: existing,
            }),
            result,
        );
    }

    #[test]
    fn len_returns_number_of_declared_streams() {
        let streams = StreamDeclarations::try_from_streams(vec![
            stream("accounts::primary"),
            stream("audit::shadow"),
        ])
        .expect("multi-stream declaration should succeed");

        assert_eq!(2, streams.len());
    }

    #[test]
    fn is_empty_returns_true_for_empty_construction() {
        let result = StreamDeclarations::try_from_streams(Vec::<StreamId>::new());

        assert!(matches!(result, Err(StreamDeclarationsError::Empty)));
    }

    #[test]
    fn is_empty_returns_false_for_single_stream() {
        let streams = StreamDeclarations::single(stream("accounts::primary"));

        assert!(!streams.is_empty());
    }

    #[test]
    fn is_empty_returns_false_for_multi_stream() {
        let streams = StreamDeclarations::try_from_streams(vec![
            stream("accounts::primary"),
            stream("audit::shadow"),
        ])
        .expect("multi-stream declaration should succeed");

        assert!(!streams.is_empty());
    }

    #[test]
    fn stream_declarations_len_and_is_empty_consistency() {
        let primary = stream("accounts::primary");
        let secondary = stream("audit::shadow");

        let single = StreamDeclarations::single(primary.clone());
        let multi = StreamDeclarations::try_from_streams(vec![primary, secondary])
            .expect("multi-stream declaration should succeed");
        let empty_error = StreamDeclarations::try_from_streams(Vec::<StreamId>::new())
            .expect_err("empty set rejected");
        let invariant_empty = StreamDeclarations {
            streams: Vec::new(),
        };

        let observed = (
            single.len(),
            single.is_empty(),
            multi.len(),
            multi.is_empty(),
            matches!(empty_error, StreamDeclarationsError::Empty),
            invariant_empty.is_empty(),
        );

        assert_eq!(observed, (1, false, 2, false, true, true));
    }
}
