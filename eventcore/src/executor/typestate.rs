//! Enhanced type-safe command execution with comprehensive state machine
//!
//! This module extends the existing typestate pattern with additional states
//! and stronger compile-time guarantees for the complete command lifecycle.

use crate::command::{Command, CommandResult, ReadStreams, StreamResolver, StreamWrite};
use crate::errors::CommandError;
use crate::event_store::{EventStore, EventToWrite, ExpectedVersion, StreamData, StreamEvents};
use crate::executor::{ExecutionContext, ExecutionOptions};
use crate::types::{EventId, StreamId};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::time::{Duration, Instant};

/// Enhanced marker types for execution states
pub mod states {
    /// Command initialized but not yet validated
    pub struct Initialized;

    /// Command input validated and streams identified
    pub struct Validated;

    /// Streams have been read from event store
    pub struct StreamsRead;

    /// State has been reconstructed from events
    pub struct StateReconstructed;

    /// Command business logic has been executed
    pub struct CommandExecuted;

    /// Events have been prepared for writing
    pub struct EventsPrepared;

    /// Events have been written to event store
    pub struct EventsWritten;

    /// Command execution failed and can be retried
    pub struct Retryable;

    /// Command execution completed (success or final failure)
    pub struct Completed;
}

/// Execution metrics collected during command processing
#[derive(Debug, Clone)]
pub struct ExecutionMetrics {
    /// Time spent reading streams
    pub stream_read_duration: Duration,
    /// Time spent reconstructing state
    pub state_reconstruction_duration: Duration,
    /// Time spent in command business logic
    pub command_execution_duration: Duration,
    /// Time spent preparing events
    pub event_preparation_duration: Duration,
    /// Time spent writing events
    pub event_write_duration: Duration,
    /// Number of streams read
    pub streams_read: usize,
    /// Number of events processed
    pub events_processed: usize,
    /// Number of events written
    pub events_written: usize,
    /// Number of retry attempts
    pub retry_attempts: u32,
}

impl Default for ExecutionMetrics {
    fn default() -> Self {
        Self {
            stream_read_duration: Duration::ZERO,
            state_reconstruction_duration: Duration::ZERO,
            command_execution_duration: Duration::ZERO,
            event_preparation_duration: Duration::ZERO,
            event_write_duration: Duration::ZERO,
            streams_read: 0,
            events_processed: 0,
            events_written: 0,
            retry_attempts: 0,
        }
    }
}

/// Enhanced command execution context with comprehensive state tracking
pub struct CommandExecution<State, C, ES>
where
    C: Command,
    ES: EventStore,
{
    /// The command being executed
    command: C,
    /// Execution options
    options: ExecutionOptions,
    /// Execution metrics
    metrics: ExecutionMetrics,
    /// Event store reference
    event_store: ES,
    /// Current state marker
    _state: PhantomData<State>,
}

/// Initial state - command created but not validated
impl<C, ES> CommandExecution<states::Initialized, C, ES>
where
    C: Command,
    ES: EventStore,
{
    /// Create a new command execution
    pub fn new(command: C, event_store: ES, options: ExecutionOptions) -> Self {
        Self {
            command,
            options,
            metrics: ExecutionMetrics::default(),
            event_store,
            _state: PhantomData,
        }
    }

    /// Validate command input and identify required streams
    /// This transitions from Initialized → Validated
    pub fn validate(self) -> CommandResult<ValidatedExecution<C, ES>> {
        // Get the streams this command needs
        let stream_ids = self.command.read_streams();

        // Validate stream count limits
        if stream_ids.is_empty() {
            return Err(CommandError::ValidationFailed(
                "Command must read at least one stream".to_string(),
            ));
        }

        if stream_ids.len() > 100 {
            return Err(CommandError::ValidationFailed(format!(
                "Command reads too many streams: {} (max 100)",
                stream_ids.len()
            )));
        }

        Ok(ValidatedExecution {
            inner: CommandExecution {
                command: self.command,
                options: self.options,
                metrics: self.metrics,
                event_store: self.event_store,
                _state: PhantomData,
            },
            stream_ids,
            stream_resolver: StreamResolver::new(),
        })
    }
}

/// Validated state with identified streams
pub struct ValidatedExecution<C: Command, ES: EventStore> {
    inner: CommandExecution<states::Validated, C, ES>,
    stream_ids: Vec<StreamId>,
    stream_resolver: StreamResolver,
}

/// Wrapper for stream data with associated state
pub struct StreamsReadExecution<C: Command, ES: EventStore> {
    inner: CommandExecution<states::StreamsRead, C, ES>,
    stream_data: StreamData<ES::Event>,
    stream_ids: Vec<StreamId>,
}

impl<C, ES> ValidatedExecution<C, ES>
where
    C: Command,
    ES: EventStore,
{
    /// Read the required streams from event store
    /// This transitions from Validated → StreamsRead
    pub async fn read_streams(self) -> Result<StreamsReadExecution<C, ES>, CommandError>
    where
        ES::Event: Clone,
    {
        let start = Instant::now();

        // Read streams from event store
        let stream_data = self
            .inner
            .event_store
            .read_streams(
                &self.stream_ids,
                &crate::event_store::ReadOptions::default(),
            )
            .await
            .map_err(CommandError::EventStore)?;

        let mut metrics = self.inner.metrics;
        metrics.stream_read_duration = start.elapsed();
        metrics.streams_read = self.stream_ids.len();

        Ok(StreamsReadExecution {
            inner: CommandExecution {
                command: self.inner.command,
                options: self.inner.options,
                metrics,
                event_store: self.inner.event_store,
                _state: PhantomData,
            },
            stream_data,
            stream_ids: self.stream_ids,
        })
    }

    /// Get the stream resolver for dynamic stream discovery
    pub fn stream_resolver(&mut self) -> &mut StreamResolver {
        &mut self.stream_resolver
    }
}

/// Streams read state - ready for state reconstruction
impl<C, ES> StreamsReadExecution<C, ES>
where
    C: Command,
    ES: EventStore,
{
    /// Reconstruct command state from events
    /// This transitions from StreamsRead → StateReconstructed
    pub fn reconstruct_state(mut self) -> StateReconstructedExecution<C, ES>
    where
        C::Event: for<'a> TryFrom<&'a ES::Event> + PartialEq + Eq,
        for<'a> <C::Event as TryFrom<&'a ES::Event>>::Error: std::fmt::Display,
    {
        let start = Instant::now();
        let mut state = C::State::default();
        let mut events_processed = 0;

        // Apply events to reconstruct state
        for event in self.stream_data.events() {
            if let Ok(command_event) = C::Event::try_from(&event.payload) {
                let stored_event = crate::event_store::StoredEvent::new(
                    event.event_id,
                    event.stream_id.clone(),
                    event.event_version,
                    event.timestamp,
                    command_event,
                    event.metadata.clone(),
                );
                self.inner.command.apply(&mut state, &stored_event);
                events_processed += 1;
            }
        }

        self.inner.metrics.state_reconstruction_duration = start.elapsed();
        self.inner.metrics.events_processed = events_processed;

        StateReconstructedExecution {
            inner: CommandExecution {
                command: self.inner.command,
                options: self.inner.options,
                metrics: self.inner.metrics,
                event_store: self.inner.event_store,
                _state: PhantomData,
            },
            state,
            stream_data: self.stream_data,
            stream_ids: self.stream_ids,
        }
    }

    /// Get reference to the stream data
    pub const fn stream_data(&self) -> &StreamData<ES::Event> {
        &self.stream_data
    }
}

/// State reconstructed - ready for command execution
pub struct StateReconstructedExecution<C: Command, ES: EventStore> {
    inner: CommandExecution<states::StateReconstructed, C, ES>,
    state: C::State,
    stream_data: StreamData<ES::Event>,
    stream_ids: Vec<StreamId>,
}

impl<C, ES> StateReconstructedExecution<C, ES>
where
    C: Command,
    ES: EventStore,
{
    /// Execute the command's business logic
    /// This transitions from StateReconstructed → CommandExecuted
    pub async fn execute_command(
        mut self,
        stream_resolver: &mut StreamResolver,
    ) -> CommandResult<CommandExecutedExecution<C, ES>> {
        let start = Instant::now();

        let read_streams = ReadStreams::new(self.stream_ids.clone());
        let stream_writes = self
            .inner
            .command
            .handle(read_streams, self.state, stream_resolver)
            .await?;

        self.inner.metrics.command_execution_duration = start.elapsed();

        Ok(CommandExecutedExecution {
            inner: CommandExecution {
                command: self.inner.command,
                options: self.inner.options,
                metrics: self.inner.metrics,
                event_store: self.inner.event_store,
                _state: PhantomData,
            },
            stream_writes,
            stream_data: self.stream_data,
        })
    }
}

/// Command executed - ready to prepare events
pub struct CommandExecutedExecution<C: Command, ES: EventStore> {
    inner: CommandExecution<states::CommandExecuted, C, ES>,
    stream_writes: Vec<StreamWrite<C::StreamSet, C::Event>>,
    stream_data: StreamData<ES::Event>,
}

impl<C, ES> CommandExecutedExecution<C, ES>
where
    C: Command,
    ES: EventStore,
{
    /// Prepare events for writing to event store
    /// This transitions from CommandExecuted → EventsPrepared
    pub fn prepare_events(mut self) -> EventsPreparedExecution<C, ES>
    where
        C::Event: serde::Serialize,
        ES::Event: From<C::Event>,
    {
        let start = Instant::now();

        // Group events by stream
        let mut events_by_stream: HashMap<StreamId, Vec<EventToWrite<ES::Event>>> = HashMap::new();

        for write in self.stream_writes {
            let (stream_id, command_event) = write.into_parts();
            let storage_event: ES::Event = command_event.into();

            // Create event metadata from execution context
            let mut metadata = crate::metadata::EventMetadata::new();
            if let Ok(correlation_id) =
                uuid::Uuid::parse_str(&self.inner.options.context.correlation_id)
            {
                if let Ok(cid) = crate::metadata::CorrelationId::try_new(correlation_id) {
                    metadata = metadata.with_correlation_id(cid);
                }
            }
            if let Some(ref user_id) = self.inner.options.context.user_id {
                if let Ok(uid) = crate::metadata::UserId::try_new(user_id.clone()) {
                    metadata = metadata.with_user_id(Some(uid));
                }
            }

            let event_to_write =
                EventToWrite::with_metadata(EventId::new(), storage_event, metadata);

            events_by_stream
                .entry(stream_id)
                .or_default()
                .push(event_to_write);
        }

        // Create StreamEvents with proper versioning
        let stream_events: Vec<StreamEvents<ES::Event>> = events_by_stream
            .into_iter()
            .map(|(stream_id, events)| {
                let expected_version = determine_expected_version(&self.stream_data, &stream_id);
                StreamEvents::new(stream_id, expected_version, events)
            })
            .collect();

        self.inner.metrics.event_preparation_duration = start.elapsed();
        self.inner.metrics.events_written = stream_events.iter().map(|se| se.events.len()).sum();

        EventsPreparedExecution {
            inner: CommandExecution {
                command: self.inner.command,
                options: self.inner.options,
                metrics: self.inner.metrics,
                event_store: self.inner.event_store,
                _state: PhantomData,
            },
            stream_events,
        }
    }
}

/// Events prepared - ready to write
pub struct EventsPreparedExecution<C: Command, ES: EventStore> {
    inner: CommandExecution<states::EventsPrepared, C, ES>,
    stream_events: Vec<StreamEvents<ES::Event>>,
}

impl<C, ES> EventsPreparedExecution<C, ES>
where
    C: Command,
    ES: EventStore,
{
    /// Write events to the event store
    /// This transitions from EventsPrepared → EventsWritten
    pub async fn write_events(
        mut self,
    ) -> Result<CommandExecution<states::EventsWritten, C, ES>, CommandError>
    where
        ES::Event: Clone,
    {
        let start = Instant::now();

        let _versions = self
            .inner
            .event_store
            .write_events_multi(self.stream_events)
            .await
            .map_err(CommandError::EventStore)?;

        self.inner.metrics.event_write_duration = start.elapsed();

        Ok(CommandExecution {
            command: self.inner.command,
            options: self.inner.options,
            metrics: self.inner.metrics,
            event_store: self.inner.event_store,
            _state: PhantomData,
        })
    }
}

/// Events written - execution successful
impl<C, ES> CommandExecution<states::EventsWritten, C, ES>
where
    C: Command,
    ES: EventStore,
{
    /// Complete the execution successfully
    /// This transitions from EventsWritten → Completed
    pub fn complete(self) -> CompletedExecution<C, ES> {
        CompletedExecution {
            metrics: self.metrics,
            success: true,
            error: None,
            _phantom: PhantomData,
        }
    }
}

/// Completed execution with metrics and result
pub struct CompletedExecution<C: Command, ES: EventStore> {
    /// Execution metrics
    pub metrics: ExecutionMetrics,
    /// Whether execution succeeded
    pub success: bool,
    /// Error if execution failed
    pub error: Option<CommandError>,
    _phantom: PhantomData<(C, ES)>,
}

impl<C: Command, ES: EventStore> CompletedExecution<C, ES> {
    /// Create a failed completion
    pub const fn failed(metrics: ExecutionMetrics, error: CommandError) -> Self {
        Self {
            metrics,
            success: false,
            error: Some(error),
            _phantom: PhantomData,
        }
    }
}

/// Retryable state for handling transient failures
impl<C, ES> CommandExecution<states::Retryable, C, ES>
where
    C: Command,
    ES: EventStore,
{
    /// Create a retryable execution from a failed state
    pub fn from_error(
        command: C,
        event_store: ES,
        options: ExecutionOptions,
        mut metrics: ExecutionMetrics,
        _error: CommandError,
    ) -> Self {
        metrics.retry_attempts += 1;
        Self {
            command,
            options,
            metrics,
            event_store,
            _state: PhantomData,
        }
    }

    /// Retry the command execution
    /// This transitions from Retryable → Initialized
    pub fn retry(self) -> CommandExecution<states::Initialized, C, ES> {
        CommandExecution {
            command: self.command,
            options: self.options,
            metrics: self.metrics,
            event_store: self.event_store,
            _state: PhantomData,
        }
    }
}

/// Helper function to determine expected version
fn determine_expected_version<E>(
    stream_data: &StreamData<E>,
    stream_id: &StreamId,
) -> ExpectedVersion {
    let max_version = stream_data
        .events()
        .filter(|e| &e.stream_id == stream_id)
        .map(|e| e.event_version)
        .max();

    max_version.map_or(ExpectedVersion::New, ExpectedVersion::Exact)
}

/// Legacy ExecutionScope for compatibility with existing executor
/// This provides a bridge to the new enhanced typestate system
pub struct ExecutionScope<State, C, ES>
where
    C: Command,
    ES: EventStore,
{
    /// The stream data read at the beginning
    stream_data: StreamData<ES::Event>,
    /// Stream IDs involved in this execution
    stream_ids: Vec<StreamId>,
    /// Execution context
    context: ExecutionContext,
    /// Type marker for current state
    _state: PhantomData<State>,
    /// Command type marker
    _command: PhantomData<C>,
    /// Event store type marker
    _event_store: PhantomData<ES>,
}

impl<C, ES> ExecutionScope<states::StreamsRead, C, ES>
where
    C: Command,
    ES: EventStore,
    C::Event: Clone + PartialEq + Eq + for<'a> TryFrom<&'a ES::Event>,
    for<'a> <C::Event as TryFrom<&'a ES::Event>>::Error: std::fmt::Display,
{
    /// Create a new execution scope with freshly read stream data
    pub const fn new(
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
    pub fn reconstruct_state(self, command: &C) -> ExecutionScopeWithState<C, ES> {
        let mut state = C::State::default();

        // Apply all events to reconstruct state
        for event in self.stream_data.events() {
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
    scope: ExecutionScope<states::StateReconstructed, C, ES>,
    state: C::State,
}

impl<C: Command, ES: EventStore> ExecutionScope<states::StateReconstructed, C, ES> {
    const fn with_state(self, state: C::State) -> ExecutionScopeWithState<C, ES> {
        ExecutionScopeWithState { scope: self, state }
    }
}

impl<C: Command, ES: EventStore> ExecutionScopeWithState<C, ES> {
    /// Execute the command with the reconstructed state
    pub async fn execute_command(
        self,
        command: &C,
        stream_resolver: &mut StreamResolver,
    ) -> Result<ExecutionScopeWithWrites<C, ES>, CommandError> {
        let read_streams = ReadStreams::new(self.scope.stream_ids.clone());

        let stream_writes = command
            .handle(read_streams, self.state, stream_resolver)
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
            .additional_streams
            .iter()
            .filter(|s| !self.scope.stream_ids.contains(s))
            .cloned()
            .collect()
    }
}

/// Scope with command executed and writes ready
pub struct ExecutionScopeWithWrites<C: Command, ES: EventStore> {
    scope: ExecutionScope<states::CommandExecuted, C, ES>,
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
            .additional_streams
            .iter()
            .filter(|s| !self.scope.stream_ids.contains(s))
            .cloned()
            .collect()
    }
}

impl<C: Command, ES: EventStore> ExecutionScope<states::CommandExecuted, C, ES> {
    const fn with_writes(
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
    pub fn prepare_stream_events(self) -> Vec<StreamEvents<ES::Event>> {
        // Group events by stream
        let mut events_by_stream: HashMap<StreamId, Vec<EventToWrite<ES::Event>>> = HashMap::new();
        let scope = self.scope;

        for write in self.writes {
            let (stream_id, command_event) = write.into_parts();
            let storage_event: ES::Event = command_event.into();

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

        // Create StreamEvents with proper versioning
        events_by_stream
            .into_iter()
            .map(|(stream_id, events)| {
                let expected_version = determine_expected_version(&scope.stream_data, &stream_id);
                StreamEvents::new(stream_id, expected_version, events)
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "typestate_tests.rs"]
mod typestate_tests;
