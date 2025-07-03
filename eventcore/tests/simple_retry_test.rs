//! Simple test to isolate the retry bug
//!
//! This test creates a deterministic scenario where we can verify that
//! when a `command.handle()` method returns an error during retry,
//! the executor properly propagates that error instead of continuing execution.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use eventcore::{
    CommandError, CommandExecutor, CommandResult, EventStore, EventStoreError, EventVersion,
    ExecutionOptions, ReadStreams, StoredEvent, StreamId, StreamResolver, StreamWrite,
};
use eventcore_memory::InMemoryEventStore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum TestEvent {
    Created { id: usize },
}

// Required for EventStore implementation
impl<'a> TryFrom<&'a Self> for TestEvent {
    type Error = &'static str;

    fn try_from(value: &'a Self) -> Result<Self, Self::Error> {
        Ok(value.clone())
    }
}

#[derive(Debug, Default, Clone)]
struct TestState {
    exists: bool,
}

/// A command that fails with `BusinessRuleViolation` after the first successful call
#[derive(Debug, Clone)]
struct FailOnRetryCommand {
    stream_id: StreamId,
    handle_call_count: Arc<AtomicUsize>,
}

impl FailOnRetryCommand {
    fn new(stream_id: StreamId) -> Self {
        Self {
            stream_id,
            handle_call_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn handle_call_count(&self) -> usize {
        self.handle_call_count.load(Ordering::SeqCst)
    }
}

impl eventcore::CommandStreams for FailOnRetryCommand {
    type StreamSet = ();

    fn read_streams(&self) -> Vec<StreamId> {
        vec![self.stream_id.clone()]
    }
}

#[async_trait::async_trait]
impl eventcore::CommandLogic for FailOnRetryCommand {
    type State = TestState;
    type Event = TestEvent;

    fn apply(&self, state: &mut Self::State, _event: &StoredEvent<Self::Event>) {
        state.exists = true;
    }

    async fn handle(
        &self,
        read_streams: ReadStreams<Self::StreamSet>,
        state: Self::State,
        _resolver: &mut StreamResolver,
    ) -> CommandResult<Vec<StreamWrite<Self::StreamSet, Self::Event>>> {
        let call_count = self.handle_call_count.fetch_add(1, Ordering::SeqCst) + 1;
        eprintln!(
            "FailOnRetryCommand handle() called #{}, state.exists={}",
            call_count, state.exists
        );

        if state.exists {
            eprintln!("FailOnRetryCommand: Stream exists, failing with BusinessRuleViolation");
            return Err(CommandError::BusinessRuleViolation(
                "Stream already exists".to_string(),
            ));
        }

        eprintln!("FailOnRetryCommand: Creating event");
        Ok(vec![StreamWrite::new(
            &read_streams,
            self.stream_id.clone(),
            TestEvent::Created { id: 1 },
        )?])
    }
}

/// Event store that fails the first write with version conflict, then succeeds
struct ConflictOnFirstWriteStore {
    inner: InMemoryEventStore<TestEvent>,
    write_attempt: Arc<AtomicUsize>,
}

impl ConflictOnFirstWriteStore {
    fn new() -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            write_attempt: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Clone for ConflictOnFirstWriteStore {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            write_attempt: self.write_attempt.clone(),
        }
    }
}

#[async_trait::async_trait]
impl EventStore for ConflictOnFirstWriteStore {
    type Event = TestEvent;

    async fn read_streams(
        &self,
        stream_ids: &[StreamId],
        options: &eventcore::ReadOptions,
    ) -> Result<eventcore::StreamData<Self::Event>, EventStoreError> {
        self.inner.read_streams(stream_ids, options).await
    }

    async fn write_events_multi(
        &self,
        streams: Vec<eventcore::StreamEvents<Self::Event>>,
    ) -> Result<std::collections::HashMap<StreamId, EventVersion>, EventStoreError> {
        let attempt = self.write_attempt.fetch_add(1, Ordering::SeqCst) + 1;
        eprintln!("ConflictOnFirstWriteStore: write attempt #{}", attempt);

        if attempt == 1 {
            eprintln!("ConflictOnFirstWriteStore: Injecting version conflict on first write");
            // Add an event to the inner store to simulate concurrent write
            let concurrent_event = vec![eventcore::StreamEvents::new(
                streams[0].stream_id.clone(),
                eventcore::ExpectedVersion::New,
                vec![eventcore::EventToWrite::new(
                    eventcore::EventId::new(),
                    TestEvent::Created { id: 999 },
                )],
            )];
            self.inner
                .write_events_multi(concurrent_event)
                .await
                .unwrap();

            // Return version conflict
            return Err(EventStoreError::VersionConflict {
                stream: streams[0].stream_id.clone(),
                expected: EventVersion::initial(),
                current: EventVersion::try_new(1).unwrap(),
            });
        }

        eprintln!("ConflictOnFirstWriteStore: Proceeding with normal write");
        self.inner.write_events_multi(streams).await
    }

    async fn subscribe(
        &self,
        options: eventcore::SubscriptionOptions,
    ) -> Result<Box<dyn eventcore::Subscription<Event = Self::Event>>, EventStoreError> {
        self.inner.subscribe(options).await
    }

    async fn stream_exists(&self, stream_id: &StreamId) -> Result<bool, EventStoreError> {
        self.inner.stream_exists(stream_id).await
    }

    async fn get_stream_version(
        &self,
        stream_id: &StreamId,
    ) -> Result<Option<EventVersion>, EventStoreError> {
        self.inner.get_stream_version(stream_id).await
    }
}

#[tokio::test]
async fn test_command_handle_error_during_retry_is_propagated() {
    // This test verifies that when command.handle() returns an error during retry,
    // the executor properly propagates that error instead of continuing execution

    let event_store = ConflictOnFirstWriteStore::new();
    let executor = CommandExecutor::new(event_store);
    let stream_id = StreamId::try_new("test-stream").unwrap();
    let command = FailOnRetryCommand::new(stream_id);

    let result = executor
        .execute(command.clone(), ExecutionOptions::default())
        .await;

    eprintln!("Final result: {result:?}");
    eprintln!(
        "Command handle() was called {} times",
        command.handle_call_count()
    );

    // Verify the command was called twice (initial + retry)
    assert_eq!(
        command.handle_call_count(),
        2,
        "Command handle() should be called twice (initial + retry)"
    );

    // Verify the result is a BusinessRuleViolation (from the retry)
    assert!(
        matches!(result, Err(CommandError::BusinessRuleViolation(ref msg)) if msg.contains("already exists")),
        "Result should be BusinessRuleViolation from retry, got: {result:?}"
    );
}
