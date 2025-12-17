use crate::Event;
use crate::validation::no_glob_metacharacters;
use nutype::nutype;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

/// Collection of events to write, organized by stream.
///
/// StreamWrites represents the output of a command's handle() method,
/// ready to be persisted atomically across multiple streams.
///
/// Uses type erasure to store events of different types in the same collection.
/// Events are boxed as `Box<dyn Any>` for storage and later downcast when reading.
///
/// # Builder API
///
/// StreamWrites supports two builder patterns:
///
/// **Result-based (existing):** Each method returns `Result`, requiring `.and_then()` chains:
/// ```ignore
/// StreamWrites::new()
///     .register_stream(stream_id, version)
///     .and_then(|w| w.append(event))
///     .expect("failed")
/// ```
///
/// **Fluent builder (recommended):** Methods return `Self` for clean chaining:
/// ```ignore
/// StreamWrites::new()
///     .with_stream(stream_id, version)
///     .with_event(event)
///     .build()?
/// ```
///
/// The fluent builder accumulates errors internally and validates at `build()` time.
#[derive(Debug)]
pub struct StreamWrites {
    entries: Vec<StreamWriteEntry>,
    expected_versions: HashMap<StreamId, StreamVersion>,
    /// Accumulated errors from fluent builder methods (with_stream, with_event).
    /// Checked and returned as error from build().
    builder_errors: Vec<EventStoreError>,
}

#[derive(Debug)]
pub struct StreamWriteEntry {
    pub stream_id: StreamId,
    pub event: Box<dyn std::any::Any + Send>,
    pub event_type: &'static str,
    pub event_type_name: crate::command::EventTypeName,
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
            builder_errors: Vec::new(),
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

        let event_type_name = event.event_type_name();

        let entry = StreamWriteEntry {
            stream_id,
            event: Box::new(event),
            event_type: std::any::type_name::<E>(),
            event_type_name,
            event_data,
        };
        writes.entries.push(entry);

        Ok(writes)
    }

    // -------------------------------------------------------------------------
    // Fluent builder API (recommended for clean chaining)
    // -------------------------------------------------------------------------

    /// Register a stream and its expected version using fluent builder pattern.
    ///
    /// This method always returns `Self` for clean chaining. Errors are accumulated
    /// internally and checked when [`build()`](Self::build) is called.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let writes = StreamWrites::new()
    ///     .with_stream(stream_id, StreamVersion::new(0))
    ///     .with_event(event)
    ///     .build()?;
    /// ```
    pub fn with_stream(mut self, stream_id: StreamId, expected_version: StreamVersion) -> Self {
        use std::collections::hash_map::Entry;

        match self.expected_versions.entry(stream_id.clone()) {
            Entry::Vacant(entry) => {
                let _ = entry.insert(expected_version);
            }
            Entry::Occupied(entry) => {
                let first_version = *entry.get();
                if first_version != expected_version {
                    self.builder_errors
                        .push(EventStoreError::ConflictingExpectedVersions {
                            stream_id,
                            first_version,
                            second_version: expected_version,
                        });
                }
            }
        }
        self
    }

    /// Append an event to a previously registered stream using fluent builder pattern.
    ///
    /// This method always returns `Self` for clean chaining. Errors are accumulated
    /// internally and checked when [`build()`](Self::build) is called.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let writes = StreamWrites::new()
    ///     .with_stream(stream_id, StreamVersion::new(0))
    ///     .with_event(event1)
    ///     .with_event(event2)
    ///     .build()?;
    /// ```
    pub fn with_event<E: Event>(mut self, event: E) -> Self {
        let stream_id = event.stream_id().clone();

        if !self.expected_versions.contains_key(&stream_id) {
            self.builder_errors
                .push(EventStoreError::UndeclaredStream { stream_id });
            return self;
        }

        match serde_json::to_value(&event) {
            Ok(event_data) => {
                let event_type_name = event.event_type_name();
                let entry = StreamWriteEntry {
                    stream_id,
                    event: Box::new(event),
                    event_type: std::any::type_name::<E>(),
                    event_type_name,
                    event_data,
                };
                self.entries.push(entry);
            }
            Err(error) => {
                self.builder_errors
                    .push(EventStoreError::SerializationFailed {
                        stream_id,
                        detail: error.to_string(),
                    });
            }
        }
        self
    }

    /// Finalize the fluent builder and return the StreamWrites if valid.
    ///
    /// Returns the first accumulated error if any errors occurred during
    /// [`with_stream()`](Self::with_stream) or [`with_event()`](Self::with_event) calls.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let writes = StreamWrites::new()
    ///     .with_stream(stream_id, StreamVersion::new(0))
    ///     .with_event(event)
    ///     .build()?;
    ///
    /// store.append_events(writes).await?;
    /// ```
    ///
    /// # Errors
    ///
    /// Returns the first error encountered during builder method calls:
    /// - [`EventStoreError::ConflictingExpectedVersions`] if a stream was registered with
    ///   different expected versions
    /// - [`EventStoreError::UndeclaredStream`] if an event was added for an unregistered stream
    /// - [`EventStoreError::SerializationFailed`] if an event could not be serialized
    pub fn build(mut self) -> Result<Self, EventStoreError> {
        if let Some(error) = self.builder_errors.pop() {
            return Err(error);
        }
        Ok(self)
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
    StoreFailure { operation: &'static str },

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

// (event, event_type_name, event_data, global_sequence_number)
type PersistedEvent = (
    Box<dyn std::any::Any + Send>,
    crate::command::EventTypeName,
    Vec<u8>,
    u64,
);
type StreamData = (Vec<PersistedEvent>, StreamVersion);

/// Broadcast message for live subscription delivery.
///
/// Contains the minimal information needed for subscribers to filter and deserialize events.
#[derive(Clone, Debug)]
struct BroadcastEvent {
    stream_id: StreamId,
    event_type_name: crate::command::EventTypeName,
    event_data: Vec<u8>,
    sequence: u64,
}

/// In-memory event store for development and testing.
///
/// `InMemoryEventStore` implements both [`EventStore`] and [`EventSubscription`] traits,
/// providing a complete event sourcing backend that requires no external infrastructure.
///
/// # Intended Use
///
/// This store is designed for:
/// - **Development**: Rapid iteration without database setup
/// - **Testing**: Unit and integration tests with deterministic behavior
/// - **Prototyping**: Exploring event sourcing patterns before committing to infrastructure
///
/// For production workloads, use [`PostgresEventStore`] from the `eventcore-postgres` crate,
/// which provides durability, concurrent access across processes, and better performance
/// at scale.
///
/// # Performance Characteristics
///
/// - **Memory-bound**: All events are held in memory; large event volumes will exhaust RAM
/// - **Lock contention**: Uses `std::sync::Mutex` for thread safety; high concurrency may
///   experience contention during append or subscription operations
/// - **Subscription catch-up**: Historical events are loaded into memory before streaming;
///   for large event stores, this causes latency on subscription creation
/// - **Broadcast buffer**: Live subscription events use a bounded channel (1024 events);
///   slow consumers may miss events if the buffer overflows
///
/// # Subscription Behavior
///
/// Subscriptions created via [`EventSubscription::subscribe`] deliver:
/// 1. **Historical events**: All matching events that exist at subscription creation time
/// 2. **Live events**: New events appended after subscription creation (via broadcast channel)
///
/// Events are delivered in temporal order based on monotonic sequence numbers assigned
/// at append time, ensuring consistent ordering across streams.
///
/// [`EventStore`]: crate::EventStore
/// [`EventSubscription`]: crate::subscription::EventSubscription
/// [`PostgresEventStore`]: https://docs.rs/eventcore-postgres
pub struct InMemoryEventStore {
    streams: std::sync::Mutex<HashMap<StreamId, StreamData>>,
    next_sequence: std::sync::Mutex<u64>,
    broadcast_tx: tokio::sync::broadcast::Sender<BroadcastEvent>,
}

impl InMemoryEventStore {
    /// Create a new in-memory event store.
    ///
    /// Returns an empty event store ready for command execution.
    /// All streams start at version 0 (no events).
    pub fn new() -> Self {
        let (broadcast_tx, _) = tokio::sync::broadcast::channel(1024);
        Self {
            streams: std::sync::Mutex::new(HashMap::new()),
            next_sequence: std::sync::Mutex::new(0),
            broadcast_tx,
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
        let streams = self
            .streams
            .lock()
            .map_err(|_| EventStoreError::StoreFailure { operation: "read" })?;
        let events = streams
            .get(&stream_id)
            .map(|(persisted_events, _version)| {
                persisted_events
                    .iter()
                    .filter_map(|(boxed, _type_name, _data, _seq)| boxed.downcast_ref::<E>())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        Ok(EventStreamReader::new(events))
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        // Collect broadcast events to send after releasing locks
        let broadcast_events: Vec<BroadcastEvent>;

        {
            let mut streams = self
                .streams
                .lock()
                .map_err(|_| EventStoreError::StoreFailure {
                    operation: "append",
                })?;
            let expected_versions = writes.expected_versions().clone();

            // Check all version constraints before writing any events
            for (stream_id, expected_version) in &expected_versions {
                let current_version = streams
                    .get(stream_id)
                    .map(|(_events, version)| *version)
                    .unwrap_or_else(|| StreamVersion::new(0));

                if current_version != *expected_version {
                    return Err(EventStoreError::VersionConflict);
                }
            }

            // All versions match - proceed with writes
            let mut next_seq =
                self.next_sequence
                    .lock()
                    .map_err(|_| EventStoreError::StoreFailure {
                        operation: "append",
                    })?;

            let mut events_to_broadcast = Vec::new();

            for entry in writes.into_entries() {
                let StreamWriteEntry {
                    stream_id,
                    event,
                    event_type_name,
                    event_data,
                    ..
                } = entry;
                let sequence = increment_and_get_sequence(&mut next_seq);

                // Serialize event_data to bytes for subscription deserialization
                let event_bytes = serde_json::to_vec(&event_data).map_err(|e| {
                    EventStoreError::SerializationFailed {
                        stream_id: stream_id.clone(),
                        detail: e.to_string(),
                    }
                })?;

                // Collect broadcast event before storing
                events_to_broadcast.push(BroadcastEvent {
                    stream_id: stream_id.clone(),
                    event_type_name: event_type_name.clone(),
                    event_data: event_bytes.clone(),
                    sequence,
                });

                let (events, version) = streams
                    .entry(stream_id)
                    .or_insert_with(|| (Vec::new(), StreamVersion::new(0)));
                events.push((event, event_type_name, event_bytes, sequence));
                *version = version.increment();
            }

            broadcast_events = events_to_broadcast;
        } // Release locks before broadcasting

        // Broadcast events to live subscribers (ignore send errors - no receivers is OK)
        for event in broadcast_events {
            let _ = self.broadcast_tx.send(event);
        }

        Ok(EventStreamSlice)
    }
}

// --------------------------------------------------------------------------------------
// Mutation-testing helpers
// These small functions isolate timeout logic that would cause test hangs when mutated.
// --------------------------------------------------------------------------------------

/// Receive from broadcast channel with optional timeout.
/// Skipped from mutation testing as timeout mutations cause test hangs.
#[cfg_attr(test, mutants::skip)]
async fn recv_with_optional_timeout<T>(
    rx: &mut tokio::sync::broadcast::Receiver<T>,
    timeout: Option<Duration>,
) -> Option<Result<T, tokio::sync::broadcast::error::RecvError>>
where
    T: Clone,
{
    if let Some(duration) = timeout {
        tokio::time::timeout(duration, rx.recv()).await.ok()
    } else {
        Some(rx.recv().await)
    }
}

/// Increment sequence counter and return the pre-increment value.
/// Skipped from mutation testing as arithmetic mutations cause infinite hangs.
#[cfg_attr(test, mutants::skip)]
fn increment_and_get_sequence(next_seq: &mut u64) -> u64 {
    let current = *next_seq;
    *next_seq += 1;
    current
}

/// Calculate the maximum sequence number for catchup deduplication.
/// Skipped from mutation testing as arithmetic mutations cause hangs or panics.
#[cfg_attr(test, mutants::skip)]
fn calculate_catchup_max_seq(next_seq: u64) -> u64 {
    if next_seq == 0 { 0 } else { next_seq - 1 }
}

/// Check if stream should be skipped based on prefix filter.
/// Skipped from mutation testing as boolean inversions cause indefinite hangs.
#[cfg_attr(test, mutants::skip)]
fn should_skip_stream_prefix(stream_id: &str, prefix: Option<&str>) -> bool {
    match prefix {
        Some(p) => !stream_id.starts_with(p),
        None => false,
    }
}

/// Check if event should be skipped based on subscribable types.
/// Skipped from mutation testing as boolean inversions cause indefinite hangs.
#[cfg_attr(test, mutants::skip)]
fn should_skip_event_type(
    event_type_name: &crate::EventTypeName,
    subscribable_names: &[crate::EventTypeName],
) -> bool {
    !subscribable_names.contains(event_type_name)
}

/// Check if event should be skipped based on type name filter.
/// Skipped from mutation testing as comparison inversions cause indefinite hangs.
#[cfg_attr(test, mutants::skip)]
fn should_skip_event_type_filter(
    stored_type_name: &crate::EventTypeName,
    filter: Option<&crate::EventTypeName>,
) -> bool {
    match filter {
        Some(expected) => stored_type_name != expected,
        None => false,
    }
}

impl crate::subscription::EventSubscription for InMemoryEventStore {
    #[tracing::instrument(
        name = "subscribe",
        skip(self),
        fields(
            stream_prefix = query.stream_prefix().map(|p| p.as_ref()).unwrap_or(""),
            event_type_filter = query.event_type_name_filter().map(|n| n.as_ref()).unwrap_or("")
        )
    )]
    async fn subscribe<E: crate::subscription::Subscribable>(
        &self,
        query: crate::subscription::SubscriptionQuery,
    ) -> Result<crate::subscription::SubscriptionStream<E>, crate::subscription::SubscriptionError>
    {
        tracing::info!("creating subscription");

        // Get the set of type names that E can deserialize
        let subscribable_type_names = E::subscribable_type_names();

        // PHASE 1: Subscribe to broadcast channel BEFORE reading historical events
        // This ensures we don't miss events appended between read and subscribe
        let mut broadcast_rx = self.broadcast_tx.subscribe();

        // Capture current max sequence number for deduplication at transition
        let catchup_max_seq = {
            let next_seq = self.next_sequence.lock().map_err(|_| {
                crate::subscription::SubscriptionError::Generic("mutex poisoned".to_string())
            })?;
            calculate_catchup_max_seq(*next_seq)
        };

        // Collect historical events from all streams with their sequence numbers
        let streams = self.streams.lock().map_err(|_| {
            crate::subscription::SubscriptionError::Generic("mutex poisoned".to_string())
        })?;
        let mut all_events: Vec<(Result<E, crate::subscription::SubscriptionError>, u64)> =
            Vec::new();

        for (stream_id, (events, _version)) in streams.iter() {
            // Filter by stream prefix if specified
            if should_skip_stream_prefix(
                stream_id.as_ref(),
                query.stream_prefix().map(|p| p.as_ref()),
            ) {
                continue;
            }

            for (_boxed_event, stored_type_name, event_data, seq) in events {
                // Check if the stored event type name matches any of the subscribable type names
                if should_skip_event_type(stored_type_name, &subscribable_type_names) {
                    continue;
                }

                // Filter by event type name if specified in query
                if should_skip_event_type_filter(stored_type_name, query.event_type_name_filter()) {
                    continue;
                }

                // Use try_from_stored to deserialize the event
                match E::try_from_stored(stored_type_name, event_data) {
                    Ok(event) => all_events.push((Ok(event), *seq)),
                    Err(e) => all_events.push((Err(e), *seq)),
                }
            }
        }
        drop(streams); // Release lock before creating stream

        // Sort by sequence number to get temporal order
        all_events.sort_by_key(|(_, seq)| *seq);

        // Extract just the events (discard sequence numbers)
        let historical_events: Vec<Result<E, crate::subscription::SubscriptionError>> =
            all_events.into_iter().map(|(result, _)| result).collect();

        // Clone query for use in async stream
        let query_clone = query.clone();
        let subscribable_type_names_clone = subscribable_type_names.clone();

        // PHASE 2: Create combined stream - historical events first, then live events
        let idle_timeout = query.idle_timeout();
        let stream = async_stream::stream! {
            // Yield all historical events first (catch-up phase)
            for result in historical_events {
                yield result;
            }

            // Then yield live events from broadcast channel
            // If idle_timeout is set, stream terminates after that duration with no events
            // If idle_timeout is None, stream waits indefinitely for new events
            loop {
                let recv_result = recv_with_optional_timeout(&mut broadcast_rx, idle_timeout).await;

                match recv_result {
                    Some(Ok(broadcast_event)) => {
                        // Skip events we already delivered in catch-up phase
                        if broadcast_event.sequence <= catchup_max_seq {
                            continue;
                        }

                        // Apply stream prefix filter
                        if should_skip_stream_prefix(
                            broadcast_event.stream_id.as_ref(),
                            query_clone.stream_prefix().map(|p| p.as_ref()),
                        ) {
                            continue;
                        }

                        // Check if event type is in subscribable types
                        if should_skip_event_type(&broadcast_event.event_type_name, &subscribable_type_names_clone) {
                            continue;
                        }

                        // Apply event type name filter
                        if should_skip_event_type_filter(&broadcast_event.event_type_name, query_clone.event_type_name_filter()) {
                            continue;
                        }

                        // Deserialize and yield the event (or error)
                        yield E::try_from_stored(&broadcast_event.event_type_name, &broadcast_event.event_data);
                    }
                    Some(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {
                        // Subscriber fell behind - continue receiving (at-least-once semantics)
                        continue;
                    }
                    Some(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                        // Channel closed - end stream
                        break;
                    }
                    None => {
                        // Timeout expired (only happens when idle_timeout is Some)
                        break;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
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
    use crate::EventTypeName;
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

        fn event_type_name(&self) -> EventTypeName {
            "TestEvent".try_into().expect("valid event type name")
        }

        fn all_type_names() -> Vec<EventTypeName> {
            vec!["TestEvent".try_into().expect("valid event type name")]
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
        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(event.clone()))
            .expect("append should succeed");

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

        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(event))
            .expect("append should succeed");

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
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(first_event))
            .and_then(|writes| writes.append(second_event))
            .expect("append chain should succeed");

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

    #[tokio::test]
    async fn stream_writes_registers_stream_before_appending_multiple_events() {
        let store = InMemoryEventStore::new();
        let stream_id =
            StreamId::try_new("registered-stream".to_string()).expect("valid stream id");

        let first_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "first-registered-event".to_string(),
        };

        let second_event = TestEvent {
            stream_id: stream_id.clone(),
            data: "second-registered-event".to_string(),
        };

        let writes = StreamWrites::new()
            .register_stream(stream_id.clone(), StreamVersion::new(0))
            .and_then(|writes| writes.append(first_event))
            .and_then(|writes| writes.append(second_event))
            .expect("registered stream should accept events");

        let result = store.append_events(writes).await;

        assert!(
            result.is_ok(),
            "append should succeed when stream registered before events"
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

    /// Unit test: StreamId rejects asterisk glob metacharacter
    ///
    /// Per ADR-017, StreamId must reject glob metacharacters (*, ?, [, ])
    /// to enable future pattern matching without ambiguity or escaping complexity.
    ///
    /// This test verifies that StreamId::try_new() returns an error when
    /// the asterisk metacharacter is present in the identifier.
    #[test]
    fn stream_id_rejects_asterisk_metacharacter() {
        // When: Developer attempts to create StreamId with asterisk metacharacter
        let result = StreamId::try_new("account-*");

        // Then: StreamId construction fails
        assert!(
            result.is_err(),
            "StreamId should reject asterisk glob metacharacter"
        );
    }

    #[test]
    fn stream_id_rejects_question_mark_metacharacter() {
        // When: Developer attempts to create StreamId with question mark metacharacter
        let result = StreamId::try_new("account-?");

        // Then: StreamId construction fails
        assert!(result.is_err());
    }

    #[test]
    fn stream_id_rejects_open_bracket_metacharacter() {
        // When: Developer attempts to create StreamId with open bracket metacharacter
        let result = StreamId::try_new("account-[");

        // Then: StreamId construction fails
        assert!(result.is_err());
    }

    #[test]
    fn stream_id_rejects_close_bracket_metacharacter() {
        // When: Developer attempts to create StreamId with close bracket metacharacter
        let result = StreamId::try_new("account-]");

        // Then: StreamId construction fails
        assert!(result.is_err());
    }

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn subscribe_emits_tracing_span() {
        use crate::EventTypeName;
        use crate::subscription::{EventSubscription, StreamPrefix, SubscriptionQuery};

        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        struct DummyEvent {
            id: String,
        }

        impl Event for DummyEvent {
            fn stream_id(&self) -> &StreamId {
                unimplemented!()
            }
            fn event_type_name(&self) -> EventTypeName {
                "DummyEvent".try_into().expect("valid")
            }
            fn all_type_names() -> Vec<EventTypeName> {
                vec!["DummyEvent".try_into().expect("valid")]
            }
        }

        let store = InMemoryEventStore::new();
        let query = SubscriptionQuery::all()
            .filter_stream_prefix(StreamPrefix::try_new("account-").expect("valid"));

        let _subscription = store
            .subscribe::<DummyEvent>(query)
            .await
            .expect("subscription should work");

        // Check if we can see the span
        assert!(logs_contain("subscribe"), "should emit subscribe span");
    }

    // -------------------------------------------------------------------------
    // Fluent builder API tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn fluent_builder_api_appends_events_successfully() {
        let store = InMemoryEventStore::new();
        let stream_id = StreamId::try_new("fluent-stream").expect("valid stream id");

        let event = TestEvent {
            stream_id: stream_id.clone(),
            data: "fluent event".to_string(),
        };

        let writes = StreamWrites::new()
            .with_stream(stream_id.clone(), StreamVersion::new(0))
            .with_event(event.clone())
            .build()
            .expect("build should succeed");

        let _ = store
            .append_events(writes)
            .await
            .expect("append to succeed");

        let reader = store
            .read_stream::<TestEvent>(stream_id)
            .await
            .expect("read to succeed");

        assert_eq!(reader.len(), 1);
        assert_eq!(reader.first().unwrap().data, "fluent event");
    }

    #[test]
    fn fluent_builder_chains_multiple_events() {
        let stream_id = StreamId::try_new("fluent-multi").expect("valid stream id");

        let event1 = TestEvent {
            stream_id: stream_id.clone(),
            data: "first".to_string(),
        };
        let event2 = TestEvent {
            stream_id: stream_id.clone(),
            data: "second".to_string(),
        };

        let writes = StreamWrites::new()
            .with_stream(stream_id, StreamVersion::new(0))
            .with_event(event1)
            .with_event(event2)
            .build()
            .expect("build should succeed");

        let entries = writes.into_entries();
        let observed: Vec<_> = entries
            .iter()
            .map(|e| e.event.downcast_ref::<TestEvent>().unwrap().data.clone())
            .collect();

        assert_eq!(observed, vec!["first".to_string(), "second".to_string()]);
    }

    #[test]
    fn fluent_builder_build_returns_error_for_unregistered_stream() {
        let stream_id = StreamId::try_new("unregistered-fluent").expect("valid stream id");

        let event = TestEvent {
            stream_id: stream_id.clone(),
            data: "orphan event".to_string(),
        };

        let result = StreamWrites::new().with_event(event).build();

        let error = result.expect_err("build should fail for undeclared stream");
        assert!(matches!(
            error,
            EventStoreError::UndeclaredStream { stream_id: ref actual } if *actual == stream_id
        ));
    }

    #[test]
    fn fluent_builder_build_returns_error_for_conflicting_versions() {
        let stream_id = StreamId::try_new("conflict-fluent").expect("valid stream id");

        let result = StreamWrites::new()
            .with_stream(stream_id.clone(), StreamVersion::new(0))
            .with_stream(stream_id.clone(), StreamVersion::new(1))
            .build();

        let error = result.expect_err("build should fail for conflicting versions");
        assert!(matches!(
            error,
            EventStoreError::ConflictingExpectedVersions {
                stream_id: ref actual,
                first_version,
                second_version
            } if *actual == stream_id
                && first_version == StreamVersion::new(0)
                && second_version == StreamVersion::new(1)
        ));
    }

    #[test]
    fn fluent_builder_with_stream_same_version_is_idempotent() {
        let stream_id = StreamId::try_new("idempotent-fluent").expect("valid stream id");

        let event = TestEvent {
            stream_id: stream_id.clone(),
            data: "idempotent".to_string(),
        };

        // Registering the same stream with the same version multiple times should succeed
        let result = StreamWrites::new()
            .with_stream(stream_id.clone(), StreamVersion::new(0))
            .with_stream(stream_id.clone(), StreamVersion::new(0))
            .with_event(event)
            .build();

        assert!(
            result.is_ok(),
            "duplicate registration with same version should succeed"
        );
    }
}
