use std::time::Duration;

use eventcore_types::{
    Event, EventStoreError, EventStreamReader, EventStreamSlice, StreamId, StreamWrites,
};

/// Internal effect yielded by the execute pipeline state machine.
///
/// These effects describe store operations that the shell loop dispatches
/// to the `EventStore` trait. The state machine never performs I/O directly.
#[derive(Debug)]
pub(crate) enum StoreEffect {
    /// Read all events from a stream for state reconstruction.
    ReadStream { stream_id: StreamId },
    /// Atomically append events to one or more streams.
    AppendEvents { writes: StreamWrites },
    /// Sleep for a retry backoff duration.
    Sleep { duration: Duration },
}

/// Result of dispatching a `StoreEffect` to the backend.
pub(crate) enum StoreEffectResult<E: Event> {
    /// Result of a `ReadStream` effect.
    StreamRead(Result<EventStreamReader<E>, EventStoreError>),
    /// Result of an `AppendEvents` effect.
    EventsAppended(Result<EventStreamSlice, EventStoreError>),
    /// Sleep completed.
    Slept,
}
