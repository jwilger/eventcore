//! Backend-independent vocabulary and source contract for transactional projections.
//!
//! These types describe a lossless, source-scoped delivery sequence without
//! depending on a particular storage adapter. PostgreSQL-specific mechanics
//! belong in `eventcore-postgres`.

use crate::{BatchSize, StreamId, StreamPattern, StreamPrefix, StreamVersion};
use serde_json::value::RawValue;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::num::NonZeroU64;
use uuid::Uuid;

/// Error returned when a delivery identity contains no non-whitespace text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind} cannot be blank")]
pub struct DeliveryIdentityError {
    kind: &'static str,
}

impl DeliveryIdentityError {
    fn blank(kind: &'static str) -> Self {
        Self { kind }
    }
}

macro_rules! delivery_identity {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            /// Creates an identity after trimming leading and trailing whitespace.
            ///
            /// Returns [`DeliveryIdentityError`] if no non-whitespace text remains.
            pub fn try_new(value: impl Into<String>) -> Result<Self, DeliveryIdentityError> {
                let value = value.into();
                let value = value.trim();

                if value.is_empty() {
                    return Err(DeliveryIdentityError::blank(stringify!($name)));
                }

                Ok(Self(value.to_owned()))
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

delivery_identity!(
    DeliverySourceId,
    "Stable identity for the event source that owns a delivery sequence."
);
delivery_identity!(
    ProjectorName,
    "Stable application-supplied identity for a transactional projector."
);
delivery_identity!(
    ProjectionSelectionId,
    "Stable application-supplied identity for a projection selection contract."
);
delivery_identity!(
    EventTypeName,
    "Persisted discriminator for an event type selected for projection."
);

/// A positive, source-scoped position in the global delivery sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeliveryPosition(NonZeroU64);

impl DeliveryPosition {
    /// Creates a position from a backend value that is already known to be positive.
    pub fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// Returns the positive integer value of this delivery position.
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

/// Stable persisted event identity, distinct from the delivery position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PersistedEventId(Uuid);

impl PersistedEventId {
    /// Creates an event identity from a backend UUID value.
    pub fn new(value: Uuid) -> Self {
        Self(value)
    }

    /// Returns the UUID value of this persisted event identity.
    pub fn get(self) -> Uuid {
        self.0
    }
}

/// Optional inclusive upper bound for a delivery page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryUpperBound {
    /// Include events through this delivery position.
    Inclusive(DeliveryPosition),
    /// Read without an upper bound.
    Unbounded,
}

/// Stream selector applied to a projection source read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionStreamFilter {
    /// Select events from every stream.
    All,
    /// Select events from streams with a literal prefix.
    Prefix(StreamPrefix),
    /// Select events from streams matching a glob pattern.
    Pattern(StreamPattern),
}

/// Error returned when a projection selection is structurally invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProjectionSelectionError {
    /// A selection must declare at least one persisted event type.
    #[error("projection selection must include at least one event type")]
    EmptyEventTypes,
    /// A selection must not name the same persisted event type more than once.
    #[error("projection selection contains duplicate event type `{0}`")]
    DuplicateEventType(EventTypeName),
}

/// Stable selection contract used to read persisted events for a projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionSelection {
    id: ProjectionSelectionId,
    stream_filter: ProjectionStreamFilter,
    event_types: Vec<EventTypeName>,
}

impl ProjectionSelection {
    /// Creates a selection with one or more distinct persisted event types.
    ///
    /// Returns [`ProjectionSelectionError`] when the event-type list is empty
    /// or contains the same type more than once.
    pub fn try_new(
        id: ProjectionSelectionId,
        stream_filter: ProjectionStreamFilter,
        event_types: Vec<EventTypeName>,
    ) -> Result<Self, ProjectionSelectionError> {
        if event_types.is_empty() {
            return Err(ProjectionSelectionError::EmptyEventTypes);
        }

        for (index, event_type) in event_types.iter().enumerate() {
            if event_types[..index].contains(event_type) {
                return Err(ProjectionSelectionError::DuplicateEventType(
                    event_type.clone(),
                ));
            }
        }

        Ok(Self {
            id,
            stream_filter,
            event_types,
        })
    }

    /// Returns the stable identity of this selection contract.
    pub fn id(&self) -> &ProjectionSelectionId {
        &self.id
    }

    /// Returns the stream selector for this selection.
    pub fn stream_filter(&self) -> &ProjectionStreamFilter {
        &self.stream_filter
    }

    /// Returns the persisted event types selected for delivery.
    pub fn event_types(&self) -> &[EventTypeName] {
        &self.event_types
    }
}

/// Opaque persisted event data delivered by a [`ProjectionSource`].
#[derive(Debug)]
pub struct PersistedEventEnvelope {
    source_id: DeliverySourceId,
    position: DeliveryPosition,
    event_id: PersistedEventId,
    stream_id: StreamId,
    stream_version: StreamVersion,
    event_type: EventTypeName,
    payload: Box<RawValue>,
    metadata: Box<RawValue>,
}

impl PersistedEventEnvelope {
    /// Creates an envelope from validated persisted event fields and raw JSON.
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        source_id: DeliverySourceId,
        position: DeliveryPosition,
        event_id: PersistedEventId,
        stream_id: StreamId,
        stream_version: StreamVersion,
        event_type: EventTypeName,
        payload: Box<RawValue>,
        metadata: Box<RawValue>,
    ) -> Self {
        Self {
            source_id,
            position,
            event_id,
            stream_id,
            stream_version,
            event_type,
            payload,
            metadata,
        }
    }

    /// Returns the source that produced this envelope.
    pub fn source_id(&self) -> &DeliverySourceId {
        &self.source_id
    }

    /// Returns this envelope's source-scoped delivery position.
    pub fn position(&self) -> DeliveryPosition {
        self.position
    }

    /// Returns this envelope's stable persisted event identity.
    pub fn event_id(&self) -> PersistedEventId {
        self.event_id
    }

    /// Returns the stream that contains this event.
    pub fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    /// Returns this event's version within its stream.
    pub fn stream_version(&self) -> StreamVersion {
        self.stream_version
    }

    /// Returns the persisted event type discriminator.
    pub fn event_type(&self) -> &EventTypeName {
        &self.event_type
    }

    /// Returns the persisted event payload without parsing or reserializing it.
    pub fn payload(&self) -> &RawValue {
        &self.payload
    }

    /// Returns the persisted event metadata without parsing or reserializing it.
    pub fn metadata(&self) -> &RawValue {
        &self.metadata
    }
}

/// Backend-independent source of lossless, globally ordered persisted events.
///
/// A source owns a delivery sequence identified by a stable [`DeliverySourceId`]. Within that
/// sequence, every persisted event has a unique, immutable, source-scoped [`DeliveryPosition`],
/// and global delivery order preserves each stream's event order. Only committed events are
/// visible. Once the source reports a committed frontier, no event may later appear at or below
/// that frontier.
pub trait ProjectionSource: Sync {
    /// Error returned when the source cannot read its committed delivery sequence.
    type Error: Error + Send + Sync + 'static;

    /// Returns this source's stable delivery identity.
    ///
    /// The identity must remain the same across runs that use the same delivery sequence and
    /// position semantics. It must change when those semantics identify a different sequence.
    fn source_id(&self) -> &DeliverySourceId;

    /// Returns the committed high-water mark for the entire delivery sequence.
    ///
    /// The result is independent of any [`ProjectionSelection`]. An empty committed source
    /// returns `None`; otherwise the result is the greatest committed delivery position. After a
    /// position is returned, the source must not later reveal a delivery at or below it that was
    /// not already committed and readable.
    fn high_watermark(
        &self,
    ) -> impl Future<Output = Result<Option<DeliveryPosition>, Self::Error>> + Send;

    /// Reads one selected page in ascending delivery order.
    ///
    /// `after` is exclusive. [`DeliveryUpperBound::Inclusive`] is inclusive, while
    /// [`DeliveryUpperBound::Unbounded`] applies no upper position bound. Returned positions must
    /// be unique and strictly ascending. The source must apply the stream and event-type
    /// selection before applying `limit`, and it must return no more than `limit` envelopes. A
    /// zero limit returns an empty page. With a positive limit, an empty page means the selected
    /// committed sequence is exhausted within the requested position range. When `through` is a
    /// previously observed high-water mark, the source's no-late-delivery guarantee makes that
    /// exhaustion stable through the bound.
    ///
    /// Event types excluded by `selection` are intentionally omitted. A selected envelope with a
    /// malformed or unrepresentable field must instead produce an explicit error; it must not be
    /// silently skipped. Each returned [`PersistedEventEnvelope`] must preserve the source
    /// identity, delivery position, persisted event identity, stream identity and version, event
    /// type, raw payload, and raw metadata. Decoding that envelope into an application event is
    /// the application's responsibility, not the source's.
    fn read_envelopes(
        &self,
        selection: &ProjectionSelection,
        after: Option<DeliveryPosition>,
        through: DeliveryUpperBound,
        limit: BatchSize,
    ) -> impl Future<Output = Result<Vec<PersistedEventEnvelope>, Self::Error>> + Send;
}

impl<T: ProjectionSource + ?Sized> ProjectionSource for &T {
    type Error = T::Error;

    fn source_id(&self) -> &DeliverySourceId {
        T::source_id(*self)
    }

    fn high_watermark(
        &self,
    ) -> impl Future<Output = Result<Option<DeliveryPosition>, Self::Error>> + Send {
        T::high_watermark(*self)
    }

    fn read_envelopes(
        &self,
        selection: &ProjectionSelection,
        after: Option<DeliveryPosition>,
        through: DeliveryUpperBound,
        limit: BatchSize,
    ) -> impl Future<Output = Result<Vec<PersistedEventEnvelope>, Self::Error>> + Send {
        T::read_envelopes(*self, selection, after, through, limit)
    }
}
