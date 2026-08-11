use crate::command::Event;
use crate::validation::{is_valid_glob_pattern, no_glob_metacharacters};
use futures::Stream;
use futures::stream::StreamExt;
use nutype::nutype;
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

/// Collection of events to write, organized by stream.
///
/// StreamWrites represents the output of a command's handle() method,
/// ready to be persisted atomically across multiple streams.
///
/// Uses type erasure to store events of different types in the same collection.
/// Events are boxed as `Box<dyn Any>` for storage and later downcast when reading.
#[derive(Debug)]
pub struct StreamWrites {
    entries: Vec<StreamWriteEntry>,
    expected_versions: HashMap<StreamId, StreamVersion>,
}

#[derive(Debug)]
pub struct StreamWriteEntry {
    pub stream_id: StreamId,
    pub event: Box<dyn std::any::Any + Send>,
    pub event_type: &'static str,
    /// Event payload serialized to JSON exactly once, at append time. Backends
    /// bind this pre-serialized form directly (verbatim into JSONB/TEXT) rather
    /// than re-serializing a `serde_json::Value`.
    pub event_data: Box<RawValue>,
}

impl StreamWrites {
    /// Create a new empty collection of stream writes.
    ///
    /// Returns an empty StreamWrites ready to have events appended via builder pattern.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            expected_versions: HashMap::new(),
        }
    }

    /// Register a stream and its expected version prior to appending events.
    ///
    /// Commands must declare their optimistic concurrency expectation for each
    /// stream before appending events. The stream identifier must match one of
    /// the streams declared by the command's workflow.
    pub fn register_stream(
        self,
        stream_id: StreamId,
        expected_version: StreamVersion,
    ) -> Result<Self, EventStoreError> {
        use std::collections::hash_map::Entry;

        let mut writes = self;

        match writes.expected_versions.entry(stream_id.clone()) {
            Entry::Vacant(entry) => {
                let _ = entry.insert(expected_version);
                Ok(writes)
            }
            Entry::Occupied(entry) => {
                let first_version = *entry.get();

                if first_version != expected_version {
                    Err(EventStoreError::ConflictingExpectedVersions {
                        stream_id,
                        first_version,
                        second_version: expected_version,
                    })
                } else {
                    Ok(writes)
                }
            }
        }
    }

    /// Append an event to a previously registered stream using builder pattern.
    ///
    /// This method consumes self and returns a new StreamWrites with the event added.
    /// It must only be called after the stream has been registered via
    /// [`StreamWrites::register_stream`]. If the stream has not been registered,
    /// the method returns [`EventStoreError::UndeclaredStream`].
    pub fn append<E: Event>(self, event: E) -> Result<Self, EventStoreError> {
        let mut writes = self;
        let stream_id = event.stream_id().clone();

        if !writes.expected_versions.contains_key(&stream_id) {
            return Err(EventStoreError::UndeclaredStream { stream_id });
        }

        let event_data = serde_json::value::to_raw_value(&event).map_err(|error| {
            EventStoreError::SerializationFailed {
                stream_id: stream_id.clone(),
                detail: error.to_string(),
            }
        })?;

        let entry = StreamWriteEntry {
            stream_id,
            event: Box::new(event),
            event_type: E::event_type_name(),
            event_data,
        };
        writes.entries.push(entry);

        Ok(writes)
    }

    pub fn expected_versions(&self) -> &HashMap<StreamId, StreamVersion> {
        &self.expected_versions
    }

    pub fn into_entries(self) -> Vec<StreamWriteEntry> {
        self.entries
    }
}

impl Default for StreamWrites {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait defining the contract for event store implementations.
///
/// Event stores provide two core operations:
/// 1. Read events from streams for state reconstruction
/// 2. Atomically append events to streams with version checking
///
/// The EventStore trait hides implementation details of how backends achieve
/// atomicity (PostgreSQL uses ACID transactions, in-memory uses locks, etc.).
/// Library consumers interact with simple read/append operations.
///
/// Implementations include:
/// - `eventcore-postgres`: Production PostgreSQL backend with ACID guarantees
/// - `eventcore-memory`: In-memory backend for testing
///
/// # Immutability Guarantee
///
/// Event stores are append-only. Once an event is written, it must never be
/// modified or deleted. This is a fundamental invariant of event sourcing:
/// events represent facts that have already occurred in the business domain.
///
/// Implementations MUST ensure this immutability through whatever mechanisms
/// their storage backend provides:
/// - SQL databases: Use triggers/rules to reject UPDATE/DELETE operations
/// - Purpose-built event stores (e.g., Kurrent/EventStoreDB): Rely on native
///   append-only semantics
/// - In-memory stores: May omit enforcement (test-only, ephemeral)
///
/// The `eventcore-postgres` backend enforces immutability via database triggers
/// that raise errors on any attempt to UPDATE or DELETE event records.
pub trait EventStore {
    /// Read all events from a stream.
    ///
    /// Loads the complete event history from a stream for state reconstruction.
    /// Events are returned in stream version order (oldest to newest).
    ///
    /// The generic type parameter T is the consumer's event payload type.
    /// Callers must specify what event type they expect from the stream.
    ///
    /// # Parameters
    ///
    /// * `stream_id` - Identifier of the stream to read
    ///
    /// # Returns
    ///
    /// * `Ok(EventStream<T>)` - An async stream yielding each event in
    ///   stream-version order. Opening the stream may fail up front (e.g.
    ///   connection/setup); per-event decode failures surface as `Err` items
    ///   yielded by the stream.
    /// * `Err(EventStoreError)` - If the stream cannot be opened
    ///
    /// Callers that want the whole history materialized as a `Vec` should use
    /// the [`collect_events`] convenience helper.
    fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> impl Future<Output = Result<EventStream<E>, EventStoreError>> + Send;

    /// Atomically append events to multiple streams with optimistic concurrency control.
    ///
    /// This method provides the core write operation for event sourcing. It atomically
    /// appends events to one or more streams while enforcing version constraints to
    /// prevent concurrent modification conflicts.
    ///
    /// # Atomicity Guarantee
    ///
    /// All events in the write batch are committed atomically - either all events are
    /// persisted or none are. If any stream's version check fails, the entire operation
    /// is rolled back and no events are written.
    ///
    /// # Optimistic Concurrency Control
    ///
    /// Each stream write includes an expected version. The store verifies that each
    /// stream's current version matches the expected version before writing. If any
    /// version mismatch is detected, the operation fails with `EventStoreError::VersionConflict`.
    ///
    /// This prevents lost updates when multiple commands attempt to modify the same
    /// stream(s) concurrently. The caller should retry the entire command execution
    /// (reload state, re-validate, re-generate events) when conflicts occur.
    ///
    /// # Parameters
    ///
    /// * `writes` - Collection of events to append, organized by stream with expected versions
    ///
    /// # Returns
    ///
    /// * `Ok(EventStreamSlice)` - Events successfully appended to all streams
    /// * `Err(EventStoreError::VersionConflict)` - One or more streams had version mismatches
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let writes = StreamWrites::new()
    ///     .register_stream(stream_id.clone(), StreamVersion::new(0))
    ///     .and_then(|writes| writes.append(event1))
    ///     .and_then(|writes| writes.append(event2))
    ///     .expect("builder pattern should succeed");
    ///
    /// match store.append_events(writes).await {
    ///     Ok(_) => println!("Events persisted"),
    ///     Err(EventStoreError::VersionConflict { .. }) => println!("Concurrent modification detected"),
    /// }
    /// ```
    fn append_events(
        &self,
        writes: StreamWrites,
    ) -> impl Future<Output = Result<EventStreamSlice, EventStoreError>> + Send;
}

/// Stream identifier domain type.
///
/// StreamId uniquely identifies an event stream within the event store.
/// Uses nutype for compile-time validation ensuring all stream IDs are:
/// - Non-empty (trimmed strings with at least 1 character)
/// - Within reasonable length (max 255 characters)
/// - Sanitized (leading/trailing whitespace removed)
/// - Free of glob metacharacters (`*`, `?`, `[`, `]`) per ADR-017
///
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255, predicate = no_glob_metacharacters),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        Into,
        AsRef,
        Deref,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct StreamId(String);

/// Stream prefix domain type for filtering events by stream ID prefix.
///
/// StreamPrefix represents a literal prefix string used to filter events from
/// streams whose IDs start with this prefix. It is used in subscription
/// queries to select a subset of streams (e.g., all streams starting with
/// "account-") via simple "starts with" matching on StreamId.
///
/// Uses nutype for validation ensuring all prefixes are:
/// - Non-empty (at least 1 character after trimming)
/// - Within reasonable length (max 255 characters)
/// - Sanitized (leading/trailing whitespace removed)
///
/// Note: StreamPrefix performs literal prefix matching only. Per ADR-017 it
/// rejects glob metacharacters (`*`, `?`, `[`, `]`) at construction so that literal
/// prefixes can never be confused with glob patterns. Glob pattern matching is
/// provided by the dedicated [`StreamPattern`] type (ADR-0047).
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255, predicate = no_glob_metacharacters),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        Into,
        AsRef,
        Deref,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct StreamPrefix(String);

/// Stream pattern domain type for filtering events by POSIX glob pattern.
///
/// StreamPattern represents a POSIX glob pattern (per ADR-017 and ADR-0047)
/// used to filter events from streams whose IDs match the pattern. Unlike
/// [`StreamPrefix`], which performs literal "starts with" matching, a
/// StreamPattern matches the *entire* stream ID against glob syntax:
///
/// - `*` matches any sequence of characters, including the stream separator
///   `/` (the `glob` crate's `Pattern::matches` treats `/` as an ordinary
///   character — there is no `require_literal_separator` distinction)
/// - `?` matches exactly one character
/// - `[...]` matches one character from the bracketed set or range
///   (e.g. `[0-9]`, `[a-z]`)
///
/// Uses nutype for validation ensuring all patterns are:
/// - Non-empty (at least 1 character after trimming)
/// - Within reasonable length (max 255 characters)
/// - Sanitized (leading/trailing whitespace removed)
/// - Compilable as a `glob::Pattern` (parse-don't-validate: an invalid pattern
///   such as an unclosed character class can never be constructed)
///
/// # Examples
///
/// ```no_run
/// use eventcore_types::StreamPattern;
///
/// let pattern = StreamPattern::try_new("account-*").unwrap();
/// assert!(pattern.matches("account-123"));
/// assert!(!pattern.matches("order-1"));
/// ```
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255, predicate = is_valid_glob_pattern),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        Into,
        AsRef,
        Deref,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct StreamPattern(String);

impl StreamPattern {
    /// Test whether the given stream ID matches this glob pattern.
    ///
    /// The whole `stream_id` is matched against the pattern. The wildcard `*`
    /// matches across the stream separator `/`, consistent with the `glob`
    /// crate's default `Pattern::matches` behavior.
    ///
    /// Construction validation guarantees the pattern compiles, so the
    /// theoretically-impossible compile failure is treated as "no match"
    /// rather than panicking (per the no-panics-in-production rule).
    pub fn matches(&self, stream_id: &str) -> bool {
        match glob::Pattern::new(self.as_ref()) {
            Ok(pattern) => pattern.matches(stream_id),
            Err(_) => false,
        }
    }
}

/// Stream version domain type.
///
/// StreamVersion represents the version (event count) of an event stream.
/// Versions start at 0 (empty stream) and increment with each event.
#[nutype(derive(Clone, Copy, PartialEq, Debug, Display, Into))]
pub struct StreamVersion(usize);

impl StreamVersion {
    /// Increment the version by 1.
    ///
    /// Returns a new StreamVersion with the incremented value.
    /// This is used when appending events to advance the stream version.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use eventcore_types::StreamVersion;
    /// let v0 = StreamVersion::new(0);
    /// let v1 = v0.increment();
    /// assert_eq!(v1, StreamVersion::new(1));
    /// ```
    pub fn increment(self) -> Self {
        let inner: usize = self.into();
        Self::new(inner + 1)
    }
}

/// Identifies the event store operation that failed.
///
/// Used by `EventStoreError::StoreFailure` to provide strongly-typed
/// identification of which operation encountered an infrastructure failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Reading events from a stream.
    ReadStream,
    /// Appending events to streams.
    AppendEvents,
    /// Beginning a database transaction.
    BeginTransaction,
    /// Setting expected versions for optimistic concurrency control.
    SetExpectedVersions,
    /// Committing a database transaction.
    CommitTransaction,
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Operation::ReadStream => write!(f, "read_stream"),
            Operation::AppendEvents => write!(f, "append_events"),
            Operation::BeginTransaction => write!(f, "begin_transaction"),
            Operation::SetExpectedVersions => write!(f, "set_expected_versions"),
            Operation::CommitTransaction => write!(f, "commit_transaction"),
        }
    }
}

/// Error type returned by event store operations.
///
/// EventStoreError represents failures during read or append operations,
/// covering serialization, deserialization, version conflicts, infrastructure
/// failures, and stream declaration violations.
#[derive(thiserror::Error, Debug, PartialEq)]
pub enum EventStoreError {
    /// Returned when a stream is assigned multiple different expected versions within the same write batch.
    #[error(
        "conflicting expected versions for stream {stream_id}: first={first_version}, second={second_version}"
    )]
    ConflictingExpectedVersions {
        stream_id: StreamId,
        first_version: StreamVersion,
        second_version: StreamVersion,
    },

    /// Returned when append attempts are made against a stream that has not been registered with an expected version.
    #[error("stream {stream_id} must be registered before appending events")]
    UndeclaredStream { stream_id: StreamId },

    /// Returned when event serialization fails prior to persistence.
    #[error("failed to serialize event for stream {stream_id}: {detail}")]
    SerializationFailed { stream_id: StreamId, detail: String },

    /// Returned when stored event payloads cannot be deserialized into the requested type.
    #[error("failed to deserialize event for stream {stream_id}: {detail}")]
    DeserializationFailed { stream_id: StreamId, detail: String },

    /// Represents infrastructure failures surfaced by the backing store (e.g., connection drops).
    #[error("{operation} operation failed")]
    StoreFailure { operation: Operation },

    /// Version conflict during optimistic concurrency control.
    ///
    /// Returned when append_events detects that the expected version
    /// does not match the current stream version, indicating a concurrent
    /// modification occurred between read and write.
    #[error("version conflict on stream {stream_id}: expected version {expected}, found {actual}")]
    VersionConflict {
        stream_id: StreamId,
        expected: StreamVersion,
        actual: StreamVersion,
    },
}

/// An async stream of events read from a single stream, generic over the
/// consumer's event payload type.
///
/// `EventStream` is a named newtype wrapper around a boxed, pinned
/// [`futures::Stream`]. It yields each event in stream-version order (oldest to
/// newest) without materializing the entire history in memory at once. Each
/// item is a `Result<E, EventStoreError>` so that per-event decode failures can
/// surface individually rather than aborting the whole read up front.
///
/// The executor folds these items into command state one at a time (the real
/// memory win for large streams). Callers that genuinely want every event as a
/// `Vec` should use [`collect_events`].
pub struct EventStream<E: Event> {
    inner: Pin<Box<dyn Stream<Item = Result<E, EventStoreError>> + Send>>,
}

impl<E: Event> EventStream<E> {
    /// Construct an `EventStream` from any `Send` stream of decode results.
    ///
    /// Backends produce their per-event results (deserializing rows, downcasting
    /// boxed events, etc.) and wrap the resulting stream here.
    pub fn new(stream: impl Stream<Item = Result<E, EventStoreError>> + Send + 'static) -> Self {
        Self {
            inner: Box::pin(stream),
        }
    }
}

impl<E: Event> Stream for EventStream<E> {
    type Item = Result<E, EventStoreError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

/// Collect every event from a stream into a `Vec`, in stream-version order.
///
/// This is the convenience path for callers that genuinely want the whole
/// history materialized at once (tests, small streams, ad-hoc inspection). The
/// executor does NOT use this — it folds events incrementally as they arrive.
///
/// The first `Err` item encountered (e.g. a per-event decode failure) is
/// returned immediately, matching the previous behavior where a single bad
/// event failed the entire read.
pub async fn collect_events<E, S>(stream: S) -> Result<Vec<E>, EventStoreError>
where
    E: Event,
    S: Stream<Item = Result<E, EventStoreError>>,
{
    let mut stream = Box::pin(stream);
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item?);
    }
    Ok(events)
}

/// Marker type returned by a successful `EventStore::append_events()` call.
///
/// Currently a unit struct confirming that the append operation committed
/// successfully. Future versions may carry metadata such as the assigned
/// stream versions or global positions of the written events.
pub struct EventStreamSlice;

/// Blanket implementation allowing EventStore trait to work with references.
///
/// This enables passing both owned and borrowed event stores to execute():
/// - `execute(store, command)` - owned value
/// - `execute(&store, command)` - borrowed reference
///
/// This is idiomatic Rust: traits that only need `&self` methods should work
/// with references to avoid forcing consumers to clone or move stores.
impl<T: EventStore + Sync> EventStore for &T {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStream<E>, EventStoreError> {
        (*self).read_stream(stream_id).await
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        (*self).append_events(writes).await
    }
}

#[cfg(test)]
#[path = "store.test.rs"]
mod tests;
