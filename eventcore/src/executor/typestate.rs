//! Type-safe execution context using typestate pattern
//!
//! This module provides a type-safe API for command execution that makes it impossible
//! to use different StreamData for state reconstruction and event writing, preventing
//! race conditions at compile time.

use std::collections::HashMap;
use std::marker::PhantomData;

use crate::command::{Command, ReadStreams, StreamResolver, StreamWrite};
use crate::errors::CommandError;
use crate::event_store::{EventStore, EventToWrite, ExpectedVersion, StreamData, StreamEvents};
use crate::types::{EventId, StreamId};

use super::ExecutionContext;

/// Marker types for execution states
pub mod states {
    /// Initial state - streams have been read
    pub struct StreamsRead;

    /// State has been reconstructed from streams
    pub struct StateReconstructed;

    /// Command has been executed
    pub struct CommandExecuted;
}

/// Type-safe execution context that tracks the current state of execution
pub struct ExecutionScope<State, C, ES>
where
    C: Command,
    ES: EventStore,
{
    /// The stream data read at the beginning - immutable after creation
    stream_data: StreamData<ES::Event>,

    /// Stream IDs involved in this execution
    stream_ids: Vec<StreamId>,

    /// Execution context (correlation ID, user ID, etc.)
    context: ExecutionContext,

    /// Type marker for current state
    _state: PhantomData<State>,

    /// Command type marker
    _command: PhantomData<C>,

    /// Event store type marker
    _event_store: PhantomData<ES>,
}

/// Initial scope with streams read
impl<C, ES> ExecutionScope<states::StreamsRead, C, ES>
where
    C: Command,
    ES: EventStore,
    C::Event: Clone + PartialEq + Eq + for<'a> TryFrom<&'a ES::Event>,
    for<'a> <C::Event as TryFrom<&'a ES::Event>>::Error: std::fmt::Display,
{
    /// Create a new execution scope with freshly read stream data
    #[allow(clippy::missing_const_for_fn)]
    pub fn new(
        stream_data: StreamData<ES::Event>,
        stream_ids: Vec<StreamId>,
        context: ExecutionContext,
    ) -> Self {
        Self {
            stream_data,
            stream_ids,
            context,
            _state: PhantomData,
            _command: PhantomData,
            _event_store: PhantomData,
        }
    }

    /// Reconstruct state from the stream data
    /// This consumes the StreamsRead scope and returns a StateReconstructed scope
    pub fn reconstruct_state(self, command: &C) -> ExecutionScopeWithState<C, ES> {
        let mut state = C::State::default();

        // Apply all events to reconstruct state
        for event in self.stream_data.events() {
            // Convert ES::Event to C::Event using TryFrom
            if let Ok(command_event) = C::Event::try_from(&event.payload) {
                let stored_event = crate::event_store::StoredEvent::new(
                    event.event_id,
                    event.stream_id.clone(),
                    event.event_version,
                    event.timestamp,
                    command_event,
                    event.metadata.clone(),
                );
                command.apply(&mut state, &stored_event);
            }
            // Skip events that don't convert - they're for other commands
        }

        ExecutionScope {
            stream_data: self.stream_data,
            stream_ids: self.stream_ids,
            context: self.context,
            _state: PhantomData,
            _command: PhantomData,
            _event_store: PhantomData,
        }
        .with_state(state)
    }
}

/// Scope with state reconstructed
pub struct ExecutionScopeWithState<C: Command, ES: EventStore> {
    /// The execution scope
    scope: ExecutionScope<states::StateReconstructed, C, ES>,

    /// The reconstructed state
    state: C::State,
}

impl<C: Command, ES: EventStore> ExecutionScope<states::StateReconstructed, C, ES> {
    /// Attach the reconstructed state
    #[allow(clippy::missing_const_for_fn)]
    fn with_state(self, state: C::State) -> ExecutionScopeWithState<C, ES> {
        ExecutionScopeWithState { scope: self, state }
    }
}

impl<C: Command, ES: EventStore> ExecutionScopeWithState<C, ES> {
    /// Execute the command with the reconstructed state
    /// This consumes the StateReconstructed scope and returns a CommandExecuted scope
    pub async fn execute_command(
        self,
        command: &C,
        input: C::Input,
        stream_resolver: &mut StreamResolver,
    ) -> Result<ExecutionScopeWithWrites<C, ES>, CommandError> {
        let read_streams = ReadStreams::new(self.scope.stream_ids.clone());

        let stream_writes = command
            .handle(read_streams, self.state, input, stream_resolver)
            .await?;

        Ok(ExecutionScope {
            stream_data: self.scope.stream_data,
            stream_ids: self.scope.stream_ids,
            context: self.scope.context,
            _state: PhantomData,
            _command: PhantomData,
            _event_store: PhantomData,
        }
        .with_writes(stream_writes))
    }

    /// Check if additional streams were requested
    pub fn needs_additional_streams(&self, stream_resolver: &StreamResolver) -> Vec<StreamId> {
        stream_resolver
            .additional_streams()
            .iter()
            .filter(|s| !self.scope.stream_ids.contains(s))
            .cloned()
            .collect()
    }
}

/// Scope with command executed and writes ready
pub struct ExecutionScopeWithWrites<C: Command, ES: EventStore> {
    /// The execution scope
    scope: ExecutionScope<states::CommandExecuted, C, ES>,

    /// The writes to perform
    writes: Vec<StreamWrite<C::StreamSet, C::Event>>,
}

impl<C: Command, ES: EventStore> ExecutionScopeWithWrites<C, ES> {
    /// Get the number of writes that will be performed
    pub fn write_count(&self) -> usize {
        self.writes.len()
    }

    /// Check if additional streams were requested
    pub fn needs_additional_streams(&self, stream_resolver: &StreamResolver) -> Vec<StreamId> {
        stream_resolver
            .additional_streams()
            .iter()
            .filter(|s| !self.scope.stream_ids.contains(s))
            .cloned()
            .collect()
    }
}

impl<C: Command, ES: EventStore> ExecutionScope<states::CommandExecuted, C, ES> {
    /// Attach the writes
    #[allow(clippy::missing_const_for_fn)]
    fn with_writes(
        self,
        writes: Vec<StreamWrite<C::StreamSet, C::Event>>,
    ) -> ExecutionScopeWithWrites<C, ES> {
        ExecutionScopeWithWrites {
            scope: self,
            writes,
        }
    }
}

impl<C, ES> ExecutionScopeWithWrites<C, ES>
where
    C: Command,
    ES: EventStore,
    C::Event: serde::Serialize + Send + Sync,
    ES::Event: From<C::Event>,
{
    /// Prepare stream events for writing
    /// This is the ONLY way to create stream events, ensuring version consistency
    pub fn prepare_stream_events(self) -> Vec<StreamEvents<ES::Event>> {
        // Group events by stream
        let mut events_by_stream: HashMap<StreamId, Vec<EventToWrite<ES::Event>>> = HashMap::new();
        let scope = self.scope; // Move scope out to avoid partial move

        for write in self.writes {
            let (stream_id, command_event) = write.into_parts();
            // Convert C::Event to ES::Event
            let storage_event: ES::Event = command_event.into();
            // Convert ExecutionContext to EventMetadata
            let correlation_id = uuid::Uuid::parse_str(&scope.context.correlation_id)
                .ok()
                .and_then(|uuid| crate::metadata::CorrelationId::try_new(uuid).ok());
            let user_id = scope
                .context
                .user_id
                .as_ref()
                .and_then(|uid| crate::metadata::UserId::try_new(uid.clone()).ok());

            let mut metadata = crate::metadata::EventMetadata::new();
            if let Some(cid) = correlation_id {
                metadata = metadata.with_correlation_id(cid);
            }
            metadata = metadata.with_user_id(user_id);

            let event_to_write =
                EventToWrite::with_metadata(EventId::new(), storage_event, metadata);

            events_by_stream
                .entry(stream_id)
                .or_default()
                .push(event_to_write);
        }

        // Create StreamEvents with proper versioning from the SAME stream data used for state
        events_by_stream
            .into_iter()
            .map(|(stream_id, events)| {
                let expected_version = determine_expected_version(&scope.stream_data, &stream_id);
                StreamEvents::new(stream_id, expected_version, events)
            })
            .collect()
    }
}

/// Determine the expected version for a stream based on the original stream data
fn determine_expected_version<E>(
    stream_data: &StreamData<E>,
    stream_id: &StreamId,
) -> ExpectedVersion {
    // Find the highest version for this stream in our original data
    let max_version = stream_data
        .events()
        .filter(|e| &e.stream_id == stream_id)
        .map(|e| e.event_version)
        .max();

    max_version.map_or(ExpectedVersion::New, ExpectedVersion::Exact)
}

#[cfg(test)]
mod tests {

    // This test won't compile if you try to use the API incorrectly
    #[test]
    fn test_typestate_prevents_misuse() {
        // This is just a compile-time test to ensure the API is used correctly
        // The actual test logic would go in integration tests
    }

    // Example of what SHOULDN'T compile:
    // fn bad_usage(scope: ExecutionScope<states::StreamsRead, TestCommand>) {
    //     // Can't prepare events without going through all states
    //     // scope.prepare_stream_events(); // COMPILE ERROR: method not found
    // }
}
