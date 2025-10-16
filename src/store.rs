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

/// Placeholder for stream identifier type.
///
/// StreamId uniquely identifies an event stream. Will be refined to use
/// nutype with validation (not_empty, length limits).
///
/// TODO: Replace with validated nutype domain type.
pub struct StreamId;

/// Placeholder for error type returned by event store operations.
///
/// EventStoreError represents failures during read or append operations.
/// Will be refined with specific variants for different failure modes.
///
/// TODO: Implement full error hierarchy per ADR-004.
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

/// Placeholder for event stream slice type.
///
/// `EventStreamSlice` is a consecutive set of `StoredEvent` that represents a fixed number of
/// events that can start and/or end at any points short of the start and the end of the set. This
/// is primarily used to contain the resuling `StoredEvent` instances from a success call to
/// `EventStore.append_events()`.
///
/// TODO: Refine with actual metadata returned after successful append.
pub struct EventStreamSlice;
