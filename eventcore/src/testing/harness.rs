//! Command test harness for end-to-end testing.
//!
//! This module provides a test harness that simplifies testing commands
//! with various scenarios including success cases, error cases, and
//! concurrent execution.

use crate::command::{Command, CommandResult};
use crate::errors::{CommandError, EventStoreError};
use crate::event_store::{
    EventStore, EventToWrite, ExpectedVersion, ReadOptions, StreamData, StreamEvents,
};
use crate::executor::RetryConfig;
use crate::types::{EventId, EventVersion, StreamId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A test harness for testing commands end-to-end.
///
/// The harness provides a fluent interface for setting up test scenarios,
/// executing commands, and verifying results.
///
/// # Example
/// ```rust,ignore
/// use eventcore::testing::harness::CommandTestHarness;
///
/// let harness = CommandTestHarness::new()
///     .with_events("account-1", vec![/* existing events */])
///     .with_command(TransferCommand)
///     .with_input(TransferInput { /* ... */ });
///
/// let result = harness.execute().await;
/// assert!(result.is_ok());
/// ```
pub struct CommandTestHarness<C, E, S>
where
    C: Command,
    E: EventStore,
    S: Clone,
{
    event_store: E,
    command: Option<C>,
    existing_events: HashMap<StreamId, Vec<EventToWrite<C::Event>>>,
    expected_versions: HashMap<StreamId, ExpectedVersion>,
    _phantom: std::marker::PhantomData<S>,
}

impl<C, E> CommandTestHarness<C, E, E::Event>
where
    C: Command + 'static,
    C::Input: 'static,
    E: EventStore<Event = C::Event> + Clone + 'static,
    C::Event: Clone + Send + Sync + 'static,
{
    /// Creates a new test harness with the given event store.
    pub fn with_store(event_store: E) -> Self {
        Self {
            event_store,
            command: None,
            existing_events: HashMap::new(),
            expected_versions: HashMap::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Sets the command to test.
    #[must_use]
    pub fn with_command(mut self, command: C) -> Self {
        self.command = Some(command);
        self
    }

    /// Adds existing events to a stream.
    ///
    /// These events will be written to the event store before executing the command.
    #[must_use]
    pub fn with_events(mut self, stream_id: impl Into<String>, events: Vec<C::Event>) -> Self {
        let stream_id = StreamId::try_new(stream_id.into()).expect("Invalid stream ID");
        let event_writes = events
            .into_iter()
            .map(|payload| EventToWrite::new(EventId::new(), payload))
            .collect();
        self.existing_events.insert(stream_id, event_writes);
        self
    }

    /// Sets the expected version for a stream.
    ///
    /// This is used to test optimistic concurrency control.
    #[must_use]
    pub fn expect_version(
        mut self,
        stream_id: impl Into<String>,
        version: ExpectedVersion,
    ) -> Self {
        let stream_id = StreamId::try_new(stream_id.into()).expect("Invalid stream ID");
        self.expected_versions.insert(stream_id, version);
        self
    }

    /// Prepares the test by writing existing events to the store.
    async fn prepare(&self) -> Result<(), EventStoreError> {
        if self.existing_events.is_empty() {
            return Ok(());
        }

        let stream_events: Vec<_> = self
            .existing_events
            .iter()
            .map(|(stream_id, events)| {
                StreamEvents::new(stream_id.clone(), ExpectedVersion::Any, events.clone())
            })
            .collect();

        self.event_store.write_events_multi(stream_events).await?;
        Ok(())
    }

    /// Executes the command and returns the result.
    pub async fn execute(self, input: C::Input) -> CommandResult<Vec<(StreamId, C::Event)>> {
        // Prepare the test environment
        self.prepare().await.map_err(CommandError::EventStore)?;

        let command = self.command.expect("Command not set");

        // Read the streams needed by the command
        let stream_ids = command.read_streams(&input);
        let stream_data = self
            .event_store
            .read_streams(&stream_ids, &ReadOptions::default())
            .await
            .map_err(CommandError::EventStore)?;

        // Reconstruct state by folding events
        let mut state = C::State::default();
        for event in stream_data.events {
            command.apply(&mut state, &event.payload);
        }

        // Execute the command
        command.handle(state, input).await
    }

    /// Executes the command multiple times concurrently.
    ///
    /// Returns a vector of results from each execution.
    pub async fn execute_concurrent(
        self,
        inputs: Vec<C::Input>,
    ) -> Vec<CommandResult<Vec<(StreamId, C::Event)>>> {
        // Prepare the test environment
        if let Err(e) = self.prepare().await {
            return vec![Err(CommandError::EventStore(e)); inputs.len()];
        }

        let command = Arc::new(self.command.expect("Command not set"));
        let event_store = Arc::new(self.event_store);

        let mut handles = Vec::new();

        for input in inputs {
            let command = command.clone();
            let event_store = event_store.clone();

            let handle = tokio::spawn(async move {
                // Read the streams needed by the command
                let stream_ids = command.read_streams(&input);
                let stream_data = match event_store
                    .read_streams(&stream_ids, &ReadOptions::default())
                    .await
                {
                    Ok(data) => data,
                    Err(e) => return Err(CommandError::EventStore(e)),
                };

                // Reconstruct state by folding events
                let mut state = C::State::default();
                for event in stream_data.events {
                    command.apply(&mut state, &event.payload);
                }

                // Execute the command
                command.handle(state, input).await
            });

            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(_) => results.push(Err(CommandError::Internal("Task panicked".to_string()))),
            }
        }

        results
    }

    /// Executes the command and verifies it fails with the expected error.
    pub async fn execute_expecting_error(self, input: C::Input) -> CommandError {
        match self.execute(input).await {
            Ok(_) => panic!("Expected command to fail, but it succeeded"),
            Err(e) => e,
        }
    }
}

/// Builder for creating a command test scenario.
///
/// This provides a more structured way to set up complex test scenarios.
pub struct CommandTestScenarioBuilder<C>
where
    C: Command,
{
    stream_states: HashMap<StreamId, StreamState<C::Event>>,
    command: Option<C>,
    retry_config: Option<RetryConfig>,
}

/// State of a stream in the test scenario.
struct StreamState<E> {
    events: Vec<E>,
    expected_version: Option<ExpectedVersion>,
}

impl<C> CommandTestScenarioBuilder<C>
where
    C: Command + 'static,
    C::Input: 'static,
{
    /// Creates a new scenario builder.
    pub fn new() -> Self {
        Self {
            stream_states: HashMap::new(),
            command: None,
            retry_config: None,
        }
    }

    /// Sets the command to test.
    #[must_use]
    pub fn command(mut self, command: C) -> Self {
        self.command = Some(command);
        self
    }

    /// Adds a stream with events to the scenario.
    #[must_use]
    pub fn stream(mut self, stream_id: impl Into<String>, events: Vec<C::Event>) -> Self {
        let stream_id = StreamId::try_new(stream_id.into()).expect("Invalid stream ID");
        self.stream_states.insert(
            stream_id,
            StreamState {
                events,
                expected_version: None,
            },
        );
        self
    }

    /// Sets the expected version for a stream.
    #[must_use]
    pub fn with_expected_version(
        mut self,
        stream_id: impl Into<String>,
        version: ExpectedVersion,
    ) -> Self {
        let stream_id = StreamId::try_new(stream_id.into()).expect("Invalid stream ID");
        if let Some(state) = self.stream_states.get_mut(&stream_id) {
            state.expected_version = Some(version);
        }
        self
    }

    /// Sets retry configuration for the command executor.
    #[must_use]
    pub const fn with_retry(mut self, config: RetryConfig) -> Self {
        self.retry_config = Some(config);
        self
    }

    /// Builds the test harness with the provided event store.
    pub fn build<E>(self, event_store: E) -> CommandTestHarness<C, E, C::Event>
    where
        E: EventStore<Event = C::Event> + Clone + 'static,
        C::Event: Clone + Send + Sync + 'static,
    {
        let mut harness = CommandTestHarness::with_store(event_store)
            .with_command(self.command.expect("Command not set"));

        // Add existing events
        for (stream_id, state) in self.stream_states {
            harness = harness.with_events(stream_id.as_ref(), state.events);

            if let Some(version) = state.expected_version {
                harness = harness.expect_version(stream_id.as_ref(), version);
            }
        }

        // Note: CommandExecutor uses a different EventStore trait (placeholder)
        // so we can't configure it here. This will be fixed when the real
        // EventStore trait is integrated with CommandExecutor.

        harness
    }
}

impl<C> Default for CommandTestScenarioBuilder<C>
where
    C: Command + 'static,
    C::Input: 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

/// A mock event store for testing specific scenarios.
///
/// This store can be configured to fail or succeed in specific ways.
#[derive(Clone)]
pub struct MockEventStore<E> {
    read_behavior: Arc<Mutex<ReadBehavior<E>>>,
    write_behavior: Arc<Mutex<WriteBehavior>>,
}

#[derive(Clone)]
enum ReadBehavior<E> {
    Success(Vec<crate::event_store::StoredEvent<E>>),
    Failure(EventStoreError),
}

#[derive(Clone)]
enum WriteBehavior {
    Success,
    Failure(EventStoreError),
}

impl<E> MockEventStore<E> {
    /// Creates a new mock event store that succeeds all operations.
    pub fn new() -> Self {
        Self {
            read_behavior: Arc::new(Mutex::new(ReadBehavior::Success(vec![]))),
            write_behavior: Arc::new(Mutex::new(WriteBehavior::Success)),
        }
    }

    /// Configures the store to return specific events on read.
    pub async fn will_return_events(&self, events: Vec<crate::event_store::StoredEvent<E>>) {
        *self.read_behavior.lock().await = ReadBehavior::Success(events);
    }

    /// Configures the store to fail on read with a specific error.
    pub async fn will_fail_read(&self, error: EventStoreError) {
        *self.read_behavior.lock().await = ReadBehavior::Failure(error);
    }

    /// Configures the store to fail on write with a specific error.
    pub async fn will_fail_write(&self, error: EventStoreError) {
        *self.write_behavior.lock().await = WriteBehavior::Failure(error);
    }
}

impl<E> Default for MockEventStore<E> {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl<E> EventStore for MockEventStore<E>
where
    E: Clone + Send + Sync + 'static,
{
    type Event = E;

    async fn read_streams(
        &self,
        stream_ids: &[StreamId],
        _options: &ReadOptions,
    ) -> crate::errors::EventStoreResult<StreamData<Self::Event>> {
        match &*self.read_behavior.lock().await {
            ReadBehavior::Success(events) => {
                let filtered_events: Vec<_> = events
                    .iter()
                    .filter(|e| stream_ids.contains(&e.stream_id))
                    .cloned()
                    .collect();

                let mut stream_versions = HashMap::new();
                for event in &filtered_events {
                    stream_versions.insert(event.stream_id.clone(), event.event_version);
                }

                Ok(StreamData::new(filtered_events, stream_versions))
            }
            ReadBehavior::Failure(error) => Err(error.clone()),
        }
    }

    async fn write_events_multi(
        &self,
        stream_events: Vec<StreamEvents<Self::Event>>,
    ) -> crate::errors::EventStoreResult<HashMap<StreamId, EventVersion>> {
        match &*self.write_behavior.lock().await {
            WriteBehavior::Success => {
                let mut versions = HashMap::new();
                for stream in stream_events {
                    let version = EventVersion::try_new(stream.events.len() as u64).unwrap();
                    versions.insert(stream.stream_id, version);
                }
                Ok(versions)
            }
            WriteBehavior::Failure(error) => Err(error.clone()),
        }
    }

    async fn stream_exists(&self, _stream_id: &StreamId) -> crate::errors::EventStoreResult<bool> {
        Ok(true)
    }

    async fn get_stream_version(
        &self,
        _stream_id: &StreamId,
    ) -> crate::errors::EventStoreResult<Option<EventVersion>> {
        Ok(Some(EventVersion::initial()))
    }

    async fn subscribe(
        &self,
        _options: crate::subscription::SubscriptionOptions,
    ) -> crate::errors::EventStoreResult<
        Box<dyn crate::subscription::Subscription<Event = Self::Event>>,
    > {
        let subscription = crate::subscription::SubscriptionImpl::new();
        Ok(Box::new(subscription))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixtures::{TestAction, TestCommand, TestCommandInput, TestEvent};

    #[tokio::test]
    async fn test_mock_event_store() {
        let mock_store: MockEventStore<TestEvent> = MockEventStore::new();

        // Test successful read
        let result = mock_store.read_streams(&[], &ReadOptions::new()).await;
        assert!(result.is_ok());

        // Configure to fail
        mock_store
            .will_fail_read(EventStoreError::Unavailable("Test error".to_string()))
            .await;
        let result = mock_store.read_streams(&[], &ReadOptions::new()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_harness_with_mock_store() {
        let mock_store: MockEventStore<TestEvent> = MockEventStore::new();
        let harness = CommandTestHarness::with_store(mock_store).with_command(TestCommand);

        let input = TestCommandInput {
            stream_id: StreamId::try_new("test").unwrap(),
            action: TestAction::Increment { amount: 10 },
        };

        let result = harness.execute(input).await;
        assert!(result.is_ok());

        let events = result.unwrap();
        assert_eq!(events.len(), 1);
        matches!(events[0].1, TestEvent::Incremented { amount: 10 });
    }

    #[tokio::test]
    async fn test_scenario_builder_with_mock() {
        let mock_store: MockEventStore<TestEvent> = MockEventStore::new();
        let scenario = CommandTestScenarioBuilder::new()
            .command(TestCommand)
            .stream(
                "test-stream",
                vec![TestEvent::Created {
                    id: "item-1".to_string(),
                    name: "Test Item".to_string(),
                }],
            )
            .build(mock_store);

        let input = TestCommandInput {
            stream_id: StreamId::try_new("test-stream").unwrap(),
            action: TestAction::Create {
                id: "item-2".to_string(),
                name: "Another Item".to_string(),
            },
        };

        let result = scenario.execute(input).await;
        assert!(result.is_ok());
    }
}
