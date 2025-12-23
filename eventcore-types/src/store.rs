use crate::command::Event;
use crate::validation::no_glob_metacharacters;
use nutype::nutype;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;

/// Placeholder for collection of events to write, organized by stream.
///
/// StreamWrites represents the output of a command's handle() method,
/// ready to be persisted atomically across multiple streams.
///
/// Uses type erasure to store events of different types in the same collection.
/// Events are boxed as `Box<dyn Any>` for storage and later downcast when reading.
///
/// TODO: Refine based on multi-stream atomicity requirements.
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
    pub event_data: Value,
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

        let event_data =
            serde_json::to_value(&event).map_err(|error| EventStoreError::SerializationFailed {
                stream_id: stream_id.clone(),
                detail: error.to_string(),
            })?;

        let entry = StreamWriteEntry {
            stream_id,
            event: Box::new(event),
            event_type: std::any::type_name::<E>(),
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
    /// * `Ok(EventStreamReader<T>)` - Handle for reading events from the stream
    /// * `Err(EventStoreError)` - If stream cannot be read
    fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> impl Future<Output = Result<EventStreamReader<E>, EventStoreError>> + Send;

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
    ///     Err(EventStoreError::VersionConflict) => println!("Concurrent modification detected"),
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
/// - Free of glob metacharacters (*, ?, [, ]) per ADR-017
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
/// StreamPrefix represents a prefix string used to filter events from streams
/// whose IDs start with this prefix. Used in subscription queries to select
/// a subset of streams (e.g., all streams starting with "account-").
///
/// Uses nutype for validation ensuring all prefixes are:
/// - Non-empty (at least 1 character after trimming)
/// - Within reasonable length (max 255 characters)
/// - Sanitized (leading/trailing whitespace removed)
///
/// Note: Unlike StreamId, prefixes may contain glob metacharacters as they
/// represent partial stream identifiers used for filtering.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        Hash,
        AsRef,
        Deref,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct StreamPrefix(String);

/// Stream version domain type.
///
/// StreamVersion represents the version (event count) of an event stream.
/// Versions start at 0 (empty stream) and increment with each event.
#[nutype(derive(Clone, Copy, PartialEq, Debug, Display))]
pub struct StreamVersion(usize);

impl StreamVersion {
    /// Increment the version by 1.
    ///
    /// Returns a new StreamVersion with the incremented value.
    /// This is used when appending events to advance the stream version.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let v0 = StreamVersion::new(0);
    /// let v1 = v0.increment();
    /// assert_eq!(v1, StreamVersion::new(1));
    /// ```
    pub fn increment(self) -> Self {
        Self::new(self.into_inner() + 1)
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
/// EventStoreError represents failures during read or append operations.
/// Will be refined with specific variants for different failure modes.
///
/// TODO: Implement full error hierarchy per ADR-004.
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
    #[error("version conflict detected")]
    VersionConflict,
}

/// Event stream reader generic over event payload type.
///
/// EventStreamReader represents a handle for reading events from a stream.
/// The generic type parameter T is the consumer's event payload type.
/// Will be refined with actual event iteration and filtering capabilities.
///
/// TODO: Implement with async iterator or vector of events.
pub struct EventStreamReader<E: Event> {
    events: Vec<E>,
}

impl<E: Event> EventStreamReader<E> {
    pub fn new(events: Vec<E>) -> Self {
        Self { events }
    }

    /// Returns the number of events in the stream.
    ///
    /// TODO: Implement based on actual storage structure.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Returns true if the stream contains no events.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns the first event in the stream, if any.
    ///
    /// This is a convenience method for accessing the first event when
    /// reconstructing state or validating command results.
    ///
    /// # Returns
    ///
    /// * `Some(&E)` - Reference to the first event in the stream
    /// * `None` - If the stream is empty
    ///
    /// TODO: Implement based on actual storage structure.
    pub fn first(&self) -> Option<&E> {
        self.events.first()
    }

    /// Returns an iterator over the events for state reconstruction.
    ///
    /// Events are returned in stream version order (oldest to newest).
    /// This is used by the executor to fold events into state via `CommandLogic::apply()`.
    pub fn iter(&self) -> impl Iterator<Item = &E> {
        self.events.iter()
    }
}

impl<E: Event> IntoIterator for EventStreamReader<E> {
    type Item = E;
    type IntoIter = std::vec::IntoIter<E>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}

/// Placeholder for event stream slice type.
///
/// `EventStreamSlice` is a consecutive set of `StoredEvent` that represents a fixed number of
/// events that can start and/or end at any points short of the start and the end of the set. This
/// is primarily used to contain the resuling `StoredEvent` instances from a success call to
/// `EventStore.append_events()`.
///
/// TODO: Refine with actual metadata returned after successful append.
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
    ) -> Result<EventStreamReader<E>, EventStoreError> {
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
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    /// Test-specific domain event type for unit testing storage operations.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestEvent {
        stream_id: StreamId,
        data: String,
    }

    impl Event for TestEvent {
        fn stream_id(&self) -> &StreamId {
            &self.stream_id
        }
    }

    #[test]
    fn stream_writes_accepts_duplicate_stream_with_same_expected_version() {
        let stream_id = StreamId::try_new("duplicate-stream-same-version".to_string())
            .expect("valid stream id");

        let first_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "first-event".to_string(),
        };

        let second_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "second-event".to_string(),
        };

        let writes_result = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(first_event))
            .and_then(|writes| writes.append(second_event));

        assert!(writes_result.is_ok());
    }

    #[test]
    fn stream_writes_rejects_duplicate_stream_with_conflicting_expected_versions() {
        let stream_id =
            StreamId::try_new("duplicate-stream-conflict".to_string()).expect("valid stream id");

        let first_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "first-event-conflict".to_string(),
        };

        let second_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "second-event-conflict".to_string(),
        };

        let conflict = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(first_event))
            .and_then(|writes| writes.register_stream(stream_id.clone(), StreamVersion::new(1)))
            .and_then(|writes| writes.append(second_event));

        let message = conflict.unwrap_err().to_string();

        assert_eq!(
            message,
            "conflicting expected versions for stream duplicate-stream-conflict: first=0, second=1"
        );
    }

    #[test]
    fn stream_writes_rejects_appends_for_unregistered_streams() {
        let stream_id =
            StreamId::try_new("unregistered-stream".to_string()).expect("valid stream id");

        let event = TestEvent {
            stream_id: stream_id.clone(),
            data: "unregistered-event".to_string(),
        };

        let error = StreamWrites::new()
            .append(event)
            .expect_err("append without prior registration should fail");

        assert!(matches!(
            error,
            EventStoreError::UndeclaredStream { stream_id: ref actual } if *actual == stream_id
        ));
    }

    #[test]
    fn expected_versions_returns_registered_streams_and_versions() {
        let stream_a = StreamId::try_new("stream-a").expect("valid stream id");
        let stream_b = StreamId::try_new("stream-b").expect("valid stream id");

        let writes = StreamWrites::new()
            .register_stream(stream_a.clone(), StreamVersion::new(0))
            .and_then(|w| w.register_stream(stream_b.clone(), StreamVersion::new(5)))
            .expect("registration should succeed");

        let versions = writes.expected_versions();

        assert_eq!(versions.len(), 2);
        assert_eq!(versions.get(&stream_a), Some(&StreamVersion::new(0)));
        assert_eq!(versions.get(&stream_b), Some(&StreamVersion::new(5)));
    }

    #[test]
    fn stream_id_rejects_asterisk_metacharacter() {
        let result = StreamId::try_new("account-*");
        assert!(
            result.is_err(),
            "StreamId should reject asterisk glob metacharacter"
        );
    }

    #[test]
    fn stream_id_rejects_question_mark_metacharacter() {
        let result = StreamId::try_new("account-?");
        assert!(
            result.is_err(),
            "StreamId should reject question mark glob metacharacter"
        );
    }

    #[test]
    fn stream_id_rejects_open_bracket_metacharacter() {
        let result = StreamId::try_new("account-[");
        assert!(
            result.is_err(),
            "StreamId should reject open bracket glob metacharacter"
        );
    }

    #[test]
    fn stream_id_rejects_close_bracket_metacharacter() {
        let result = StreamId::try_new("account-]");
        assert!(
            result.is_err(),
            "StreamId should reject close bracket glob metacharacter"
        );
    }

    #[test]
    fn into_entries_returns_appended_events() {
        let stream_id = StreamId::try_new("into-entries-test").expect("valid stream id");
        let event = TestEvent {
            stream_id: stream_id.clone(),
            data: "test-data".to_string(),
        };

        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|w| w.append(event))
            .expect("append should succeed");

        let entries = writes.into_entries();

        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn stream_version_increment_adds_one() {
        let v0 = StreamVersion::new(5);

        let v1 = v0.increment();

        assert_eq!(v1, StreamVersion::new(6));
    }

    #[test]
    fn event_stream_reader_len_returns_event_count() {
        let stream_id = StreamId::try_new("reader-len-test").expect("valid stream id");
        let events = vec![
            TestEvent {
                stream_id: stream_id.clone(),
                data: "first".to_string(),
            },
            TestEvent {
                stream_id: stream_id.clone(),
                data: "second".to_string(),
            },
            TestEvent {
                stream_id: stream_id.clone(),
                data: "third".to_string(),
            },
        ];

        let reader = EventStreamReader::new(events);

        assert_eq!(reader.len(), 3);
    }

    #[test]
    fn event_stream_reader_is_empty_returns_true_for_empty() {
        let reader: EventStreamReader<TestEvent> = EventStreamReader::new(vec![]);

        assert!(reader.is_empty());
    }

    #[test]
    fn event_stream_reader_is_empty_returns_false_for_nonempty() {
        let stream_id = StreamId::try_new("reader-nonempty-test").expect("valid stream id");
        let events = vec![TestEvent {
            stream_id: stream_id.clone(),
            data: "event".to_string(),
        }];

        let reader = EventStreamReader::new(events);

        assert!(!reader.is_empty());
    }

    #[test]
    fn event_stream_reader_first_returns_first_event() {
        let stream_id = StreamId::try_new("reader-first-test").expect("valid stream id");
        let first_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "first".to_string(),
        };
        let events = vec![
            first_event.clone(),
            TestEvent {
                stream_id: stream_id.clone(),
                data: "second".to_string(),
            },
        ];

        let reader = EventStreamReader::new(events);

        assert_eq!(reader.first(), Some(&first_event));
    }

    #[test]
    fn event_stream_reader_iter_yields_all_events() {
        let stream_id = StreamId::try_new("reader-iter-test").expect("valid stream id");
        let events = vec![
            TestEvent {
                stream_id: stream_id.clone(),
                data: "first".to_string(),
            },
            TestEvent {
                stream_id: stream_id.clone(),
                data: "second".to_string(),
            },
        ];

        let reader = EventStreamReader::new(events.clone());
        let collected: Vec<&TestEvent> = reader.iter().collect();

        assert_eq!(collected, events.iter().collect::<Vec<_>>());
    }

    #[test]
    fn event_stream_reader_into_iter_yields_all_events() {
        let stream_id = StreamId::try_new("reader-into-iter-test").expect("valid stream id");
        let events = vec![
            TestEvent {
                stream_id: stream_id.clone(),
                data: "first".to_string(),
            },
            TestEvent {
                stream_id: stream_id.clone(),
                data: "second".to_string(),
            },
        ];

        let reader = EventStreamReader::new(events.clone());
        let collected: Vec<TestEvent> = reader.into_iter().collect();

        assert_eq!(collected, events);
    }
}
