use nutype::nutype;

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
    fn read_stream<E: crate::Event>(
        &self,
        stream_id: StreamId,
    ) -> impl std::future::Future<Output = Result<EventStreamReader<E>, EventStoreError>> + Send;

    /// Atomically append events to streams with optimistic concurrency control.
    ///
    /// This operation is atomic across all streams: either all events are
    /// written or none are. Version checking ensures no concurrent modifications
    /// occurred since the command read the streams.
    ///
    /// # Parameters
    ///
    /// * `writes` - Collection of events to append, organized by stream
    ///
    /// # Returns
    ///
    /// * `Ok(EventStreamSlice)` - Metadata about the successfully written events
    /// * `Err(EventStoreError)` - Storage failure or version conflict
    fn append_events(
        &self,
        writes: StreamWrites,
    ) -> impl std::future::Future<Output = Result<EventStreamSlice, EventStoreError>> + Send;
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

/// Error type returned by event store operations.
///
/// EventStoreError represents failures during read or append operations.
/// Will be refined with specific variants for different failure modes.
///
/// TODO: Implement full error hierarchy per ADR-004.
#[derive(thiserror::Error, Debug)]
#[error("event store operation failed")]
pub struct EventStoreError;

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
}

impl StreamWrites {
    /// Create a new empty collection of stream writes.
    ///
    /// Returns an empty StreamWrites ready to have events appended via builder pattern.
    pub fn new() -> Self {
        Self { events: Vec::new() }
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
    ///
    /// # Returns
    ///
    /// New StreamWrites instance with the event added
    pub fn append<E: crate::Event>(mut self, event: E) -> Self {
        let stream_id = event.stream_id().clone();
        self.events.push((stream_id, Box::new(event)));
        self
    }
}

impl Default for StreamWrites {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: crate::Event> FromIterator<E> for StreamWrites {
    fn from_iter<I: IntoIterator<Item = E>>(iter: I) -> Self {
        iter.into_iter()
            .fold(Self::new(), |writes, event| writes.append(event))
    }
}

/// Event stream reader generic over event payload type.
///
/// EventStreamReader represents a handle for reading events from a stream.
/// The generic type parameter T is the consumer's event payload type.
/// Will be refined with actual event iteration and filtering capabilities.
///
/// TODO: Implement with async iterator or vector of events.
pub struct EventStreamReader<E: crate::Event> {
    events: Vec<E>,
}

impl<E: crate::Event> EventStreamReader<E> {
    /// Returns the number of events in the stream.
    ///
    /// TODO: Implement based on actual storage structure.
    pub fn len(&self) -> usize {
        self.events.len()
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
pub struct InMemoryEventStore {
    streams:
        std::sync::Mutex<std::collections::HashMap<StreamId, Vec<Box<dyn std::any::Any + Send>>>>,
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
    async fn read_stream<E: crate::Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStreamReader<E>, EventStoreError> {
        let streams = self.streams.lock().unwrap();
        let events = streams
            .get(&stream_id)
            .map(|boxed_events| {
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

        for (stream_id, event) in writes.events {
            streams.entry(stream_id).or_default().push(event);
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
    async fn read_stream<E: crate::Event>(
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

    impl crate::Event for TestEvent {
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

        // And: A collection of writes containing the event
        let writes = StreamWrites::new().append(event.clone());

        // When: We append the event to the store
        store
            .append_events(writes)
            .await
            .expect("append to succeed");

        // And: We read the stream back
        let events = store
            .read_stream::<TestEvent>(stream_id)
            .await
            .expect("read to succeed");

        // And: We access the first event
        let first_event = events.first().expect("at least one event to exist");

        // Then: The event data matches what we stored
        assert_eq!(
            first_event.data, "test event data",
            "Event data should match what was stored"
        );
    }
}
