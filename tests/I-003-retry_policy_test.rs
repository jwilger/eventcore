use eventcore::{
    CommandError, CommandLogic, CommandStreams, Event, EventStore, EventStoreError,
    EventStreamReader, EventStreamSlice, InMemoryEventStore, MetricsHook, NewEvents, RetryContext,
    RetryPolicy, StreamDeclarations, StreamId, StreamWrites, execute,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// Test helper for creating stream IDs
fn test_stream_id() -> StreamId {
    StreamId::try_new(Uuid::now_v7().to_string()).expect("valid stream id")
}

/// Test-specific event type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestEvent {
    stream_id: StreamId,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name(&self) -> &'static str {
        "TestEvent"
    }
}

/// Simple command for testing
struct TestCommand {
    stream_id: StreamId,
}

impl CommandStreams for TestCommand {
    fn stream_declarations(&self) -> StreamDeclarations {
        StreamDeclarations::single(self.stream_id.clone())
    }
}

impl CommandLogic for TestCommand {
    type Event = TestEvent;
    type State = ();

    fn apply(&self, state: Self::State, _event: &Self::Event) -> Self::State {
        state
    }

    fn handle(&self, _state: Self::State) -> Result<NewEvents<Self::Event>, CommandError> {
        Ok(vec![TestEvent {
            stream_id: self.stream_id.clone(),
        }]
        .into())
    }
}

/// Event store that returns VersionConflict N times before succeeding.
///
/// This allows deterministic testing of retry behavior by controlling
/// exactly how many conflicts occur.
struct ConflictNTimesStore {
    inner: InMemoryEventStore,
    conflict_count: Arc<tokio::sync::Mutex<u32>>,
    conflicts_to_inject: u32,
}

impl ConflictNTimesStore {
    fn new(conflicts_to_inject: u32) -> Self {
        Self {
            inner: InMemoryEventStore::new(),
            conflict_count: Arc::new(tokio::sync::Mutex::new(0)),
            conflicts_to_inject,
        }
    }
}

impl EventStore for ConflictNTimesStore {
    async fn read_stream<E: Event>(
        &self,
        stream_id: StreamId,
    ) -> Result<EventStreamReader<E>, EventStoreError> {
        self.inner.read_stream(stream_id).await
    }

    async fn append_events(
        &self,
        writes: StreamWrites,
    ) -> Result<EventStreamSlice, EventStoreError> {
        let mut count = self.conflict_count.lock().await;
        if *count < self.conflicts_to_inject {
            // Inject conflict
            *count += 1;
            Err(EventStoreError::VersionConflict)
        } else {
            // Succeed normally
            self.inner.append_events(writes).await
        }
    }
}

/// Integration test: Verify MetricsHook receives correct RetryContext values.
///
/// This test validates that the retry attempt counter is correctly calculated
/// and passed to the metrics hook. Mutation testing found that changing
/// `attempt + 1` to `attempt * 1` was not caught, so this test specifically
/// verifies the attempt values are 1, 2, 3, etc.
#[tokio::test]
async fn metrics_hook_receives_correct_attempt_numbers() {
    // Given: A metrics hook that captures all RetryContext values
    struct ContextCapturingHook {
        contexts: Arc<Mutex<Vec<RetryContext>>>,
    }

    impl MetricsHook for ContextCapturingHook {
        fn on_retry_attempt(&self, ctx: &RetryContext) {
            self.contexts.lock().unwrap().push(ctx.clone());
        }
    }

    let captured_contexts = Arc::new(Mutex::new(Vec::new()));
    let hook = ContextCapturingHook {
        contexts: Arc::clone(&captured_contexts),
    };

    // And: A retry policy allowing up to 4 retries
    let policy = RetryPolicy::new().max_retries(4).with_metrics_hook(hook);

    // And: Event store that conflicts 3 times before succeeding
    let store = ConflictNTimesStore::new(3);

    // And: A test command
    let stream_id = test_stream_id();
    let command = TestCommand {
        stream_id: stream_id.clone(),
    };

    // When: Execute command that will retry 3 times
    let result = execute(&store, command, policy).await;

    // Then: Command succeeds after retries
    assert!(result.is_ok(), "command should succeed after 3 retries");

    // And: Metrics hook captured exactly 3 retry contexts
    let contexts = captured_contexts.lock().unwrap();
    assert_eq!(contexts.len(), 3, "should have captured 3 retry contexts");

    // And: First retry has attempt=1
    assert_eq!(contexts[0].attempt, 1, "first retry should have attempt=1");

    // And: Second retry has attempt=2
    assert_eq!(contexts[1].attempt, 2, "second retry should have attempt=2");

    // And: Third retry has attempt=3
    assert_eq!(contexts[2].attempt, 3, "third retry should have attempt=3");

    // And: All retries reference the declared streams
    assert_eq!(contexts[0].streams, vec![stream_id.clone()]);
    assert_eq!(contexts[1].streams, vec![stream_id.clone()]);
    assert_eq!(contexts[2].streams, vec![stream_id]);

    // And: All retries have non-zero delay_ms (exponential backoff applied)
    assert!(contexts[0].delay_ms > 0, "first retry should have delay");
    assert!(contexts[1].delay_ms > 0, "second retry should have delay");
    assert!(contexts[2].delay_ms > 0, "third retry should have delay");

    // Note: We do NOT assert exponential backoff progression here because:
    // 1. Jitter (±20%) makes this assertion flaky - worst case is delay1=12ms, delay2=16ms
    //    giving ratio of 1.33x which fails a 1.6x threshold
    // 2. Exponential backoff arithmetic is thoroughly tested in unit tests (jitter_tests module)
    // 3. This integration test's purpose is to verify MetricsHook receives correct RetryContext,
    //    not to re-test jitter arithmetic
    //
    // The fact that delay_ms > 0 confirms backoff is being calculated and passed through.
}
