use crate::Event;
use nutype::nutype;
use std::future::Future;

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
///
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 255),
    derive(Debug, Clone, PartialEq, Eq, Hash, AsRef, Deref)
)]
pub struct StreamId(String);

/// Stream version domain type.
///
/// StreamVersion represents the version (event count) of an event stream.
/// Versions start at 0 (empty stream) and increment with each event.
#[nutype(derive(Clone, Copy, PartialEq))]
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

/// Error type returned by event store operations.
///
/// EventStoreError represents failures during read or append operations.
/// Will be refined with specific variants for different failure modes.
///
/// TODO: Implement full error hierarchy per ADR-004.
#[derive(thiserror::Error, Debug)]
pub enum EventStoreError {
    /// Version conflict during optimistic concurrency control.
    ///
    /// Returned when append_events detects that the expected version
    /// does not match the current stream version, indicating a concurrent
    /// modification occurred between read and write.
    #[error("version conflict detected")]
    VersionConflict,
}

/// Placeholder for collection of events to write, organized by stream.
///
/// StreamWrites represents the output of a command's handle() method,
/// ready to be persisted atomically across multiple streams.
///
/// Uses type erasure to store events of different types in the same collection.
/// Events are boxed as `Box<dyn Any>` for storage and later downcast when reading.
///
/// TODO: Refine based on multi-stream atomicity requirements.
pub struct StreamWrites {
    events: Vec<(StreamId, Box<dyn std::any::Any + Send>)>,
    expected_versions: std::collections::HashMap<StreamId, StreamVersion>,
}

impl StreamWrites {
    /// Create a new empty collection of stream writes.
    ///
    /// Returns an empty StreamWrites ready to have events appended via builder pattern.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            expected_versions: std::collections::HashMap::new(),
        }
    }

    /// Append an event to a stream using builder pattern.
    ///
    /// This method consumes self and returns a new StreamWrites with the event added.
    /// Follows immutable builder pattern for type-safe construction.
    ///
    /// The stream ID is extracted from the event itself via the Event trait's
    /// stream_id() method. This ensures type safety: events know their own
    /// aggregate identity and cannot be appended to the wrong stream.
    ///
    /// # Parameters
    ///
    /// * `event` - The event to append (must implement Event trait)
    /// * `expected_version` - The version this write expects the stream to be at
    ///
    /// # Returns
    ///
    /// New StreamWrites instance with the event added
    pub fn append<E: Event>(mut self, event: E, expected_version: StreamVersion) -> Self {
        let stream_id = event.stream_id().clone();
        let _ = self
            .expected_versions
            .insert(stream_id.clone(), expected_version);
        self.events.push((stream_id, Box::new(event)));
        self
    }
}

impl Default for StreamWrites {
    fn default() -> Self {
        Self::new()
    }
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

/// In-memory event store implementation for testing.
///
/// InMemoryEventStore provides a lightweight, zero-dependency storage backend
/// for EventCore integration tests and development. It implements the EventStore
/// trait using standard library collections (HashMap, BTreeMap) with optimistic
/// concurrency control via version checking.
///
/// This implementation is included in the main eventcore crate (per ADR-011)
/// because it has zero heavyweight dependencies and is essential testing
/// infrastructure for all EventCore users.
///
/// # Example
///
/// ```ignore
/// use eventcore::InMemoryEventStore;
///
/// let store = InMemoryEventStore::new();
/// // Use store with execute() function
/// ```
///
/// # Thread Safety
///
/// InMemoryEventStore uses interior mutability for concurrent access.
/// TODO: Determine if Arc<Mutex<>> or other synchronization primitive needed.
type StreamData = (Vec<Box<dyn std::any::Any + Send>>, StreamVersion);

pub struct InMemoryEventStore {
    streams: std::sync::Mutex<std::collections::HashMap<StreamId, StreamData>>,
}

impl InMemoryEventStore {
    /// Create a new in-memory event store.
    ///
    /// Returns an empty event store ready for command execution.
    /// All streams start at version 0 (no events).
    pub fn new() -> Self {
        Self {
            streams: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStore for InMemoryEventStore {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStreamReader<E>, EventStoreError> {
        let streams = self.streams.lock().unwrap();
        let events = streams
            .get(&stream_id)
            .map(|(boxed_events, _version)| {
                boxed_events
                    .iter()
                    .filter_map(|boxed| boxed.downcast_ref::<E>())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        Ok(EventStreamReader { events })
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        let mut streams = self.streams.lock().unwrap();

        // Check all version constraints before writing any events
        for (stream_id, expected_version) in &writes.expected_versions {
            let current_version = streams
                .get(stream_id)
                .map(|(_events, version)| *version)
                .unwrap_or_else(|| StreamVersion::new(0));

            if current_version != *expected_version {
                return Err(EventStoreError::VersionConflict);
            }
        }

        // All versions match - proceed with writes
        for (stream_id, event) in writes.events {
            let (events, version) = streams
                .entry(stream_id)
                .or_insert_with(|| (Vec::new(), StreamVersion::new(0)));
            events.push(event);
            *version = version.increment();
        }

        Ok(EventStreamSlice)
    }
}

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

    /// Test-specific domain event type for unit testing storage operations.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestEvent {
        stream_id: StreamId,
        data: String,
    }

    impl Event for TestEvent {
        fn stream_id(&self) -> &StreamId {
            &self.stream_id
        }
    }

    /// Unit test: Verify InMemoryEventStore can append and retrieve a single event
    ///
    /// This test verifies the fundamental event storage capability:
    /// - Append an event to a stream
    /// - Read the stream back
    /// - Verify the event is retrievable with correct data
    ///
    /// This is a unit test drilling down from the failing integration test
    /// test_deposit_command_event_data_is_retrievable. We're testing the
    /// storage layer in isolation before testing the full command execution flow.
    #[tokio::test]
    async fn test_append_and_read_single_event() {
        // Given: An in-memory event store
        let store = InMemoryEventStore::new();

        // And: A stream ID
        let stream_id = StreamId::try_new("test-stream-123".to_string()).expect("valid stream id");

        // And: A domain event to store
        let event = TestEvent {
            stream_id: stream_id.clone(),
            data: "test event data".to_string(),
        };

        // And: A collection of writes containing the event (expected version 0 for empty stream)
        let writes = StreamWrites::new().append(event.clone(), StreamVersion::new(0));

        // When: We append the event to the store
        let _ = store
            .append_events(writes)
            .await
            .expect("append to succeed");

        let reader = store
            .read_stream::<TestEvent>(stream_id)
            .await
            .expect("read to succeed");

        let observed = (
            reader.is_empty(),
            reader.len(),
            reader.iter().next().is_none(),
        );

        assert_eq!(observed, (false, 1usize, false));
    }

    #[tokio::test]
    async fn event_stream_reader_is_empty_reflects_stream_population() {
        let store = InMemoryEventStore::new();
        let stream_id =
            StreamId::try_new("is-empty-observation".to_string()).expect("valid stream id");

        let initial_reader = store
            .read_stream::<TestEvent>(stream_id.clone())
            .await
            .expect("initial read to succeed");

        let event = TestEvent {
            stream_id: stream_id.clone(),
            data: "populated event".to_string(),
        };

        let writes = StreamWrites::new().append(event, StreamVersion::new(0));

        let _ = store
            .append_events(writes)
            .await
            .expect("append to succeed");

        let populated_reader = store
            .read_stream::<TestEvent>(stream_id)
            .await
            .expect("populated read to succeed");

        let observed = (
            initial_reader.is_empty(),
            initial_reader.len(),
            populated_reader.is_empty(),
            populated_reader.len(),
        );

        assert_eq!(observed, (true, 0usize, false, 1usize));
    }

    #[tokio::test]
    async fn read_stream_iterates_through_events_in_order() {
        let store = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("ordered-stream".to_string()).expect("valid stream id");

        let first_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "first".to_string(),
        };

        let second_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "second".to_string(),
        };

        let writes = StreamWrites::new()
            .append(first_event, StreamVersion::new(0))
            .append(second_event, StreamVersion::new(0));

        let _ = store
            .append_events(writes)
            .await
            .expect("append to succeed");

        let reader = store
            .read_stream::<TestEvent>(stream_id)
            .await
            .expect("read to succeed");

        let collected: Vec<String> = reader.iter().map(|event| event.data.clone()).collect();

        let observed = (reader.is_empty(), collected);

        assert_eq!(
            observed,
            (false, vec!["first".to_string(), "second".to_string()])
        );
    }
}
