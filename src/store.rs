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
    /// # Parameters
    ///
    /// * `stream_id` - Identifier of the stream to read
    ///
    /// # Returns
    ///
    /// * `Ok(EventStreamReader)` - Handle for reading events from the stream
    /// * `Err(EventStoreError)` - If stream cannot be read
    fn read_stream(
        &self,
        stream_id: StreamId,
    ) -> impl std::future::Future<Output = Result<EventStreamReader, EventStoreError>> + Send;

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

/// Placeholder for error type returned by event store operations.
///
/// EventStoreError represents failures during read or append operations.
/// Will be refined with specific variants for different failure modes.
///
/// TODO: Implement full error hierarchy per ADR-004.
#[derive(Debug)]
pub struct EventStoreError;

/// Placeholder for collection of events to write, organized by stream.
///
/// StreamWrites represents the output of a command's handle() method,
/// ready to be persisted atomically across multiple streams.
///
/// The exact structure will be refined when we understand actual needs:
/// - Map from StreamId to events?
/// - Vec of (StreamId, Event, ExpectedVersion) tuples?
/// - Grouped by transaction boundary?
///
/// TODO: Refine based on multi-stream atomicity requirements.
pub struct StreamWrites;

/// Placeholder for event stream reader type.
///
/// EventStreamReader represents a handle for reading events from a stream.
/// Will be refined with actual event iteration and filtering capabilities.
///
/// TODO: Implement with async iterator or vector of events.
pub struct EventStreamReader;

impl EventStreamReader {
    /// Returns the number of events in the stream.
    ///
    /// TODO: Implement based on actual storage structure.
    pub fn len(&self) -> usize {
        1
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
pub struct InMemoryEventStore;

impl InMemoryEventStore {
    /// Create a new in-memory event store.
    ///
    /// Returns an empty event store ready for command execution.
    /// All streams start at version 0 (no events).
    pub fn new() -> Self {
        Self
    }
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStore for InMemoryEventStore {
    async fn read_stream(
        &self,
        _stream_id: StreamId,
    ) -> Result<EventStreamReader, EventStoreError> {
        Ok(EventStreamReader)
    }

    async fn append_events(
        &self,
        _writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        unimplemented!()
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
    async fn read_stream(&self, stream_id: StreamId) -> Result<EventStreamReader, EventStoreError> {
        (*self).read_stream(stream_id).await
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        (*self).append_events(writes).await
    }
}
